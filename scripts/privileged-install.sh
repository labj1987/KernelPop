#!/usr/bin/env bash
# privileged-install.sh — runs as root via pkexec.
#
# THE WHOLE POINT OF THIS SCRIPT
# ------------------------------
# ukuu and hand-rolled scripts have repeatedly installed mainline kernels
# without generating the initramfs, because they parsed the kernel version
# out of a FILENAME with a regex and the mainline naming convention
# (7.1.3-070103-generic) broke the parse. One reboot later: VFS panic.
#
# This script never parses filenames. The kernel version is read from the
# .deb package metadata (dpkg-deb -f Package), cross-checked against the
# /lib/modules directory that actually appears after install, and the
# initramfs is VERIFIED to exist on disk before the script reports success.
# If the initramfs is missing, this script fails loudly instead of leaving
# an unbootable kernel behind.
#
# Usage:
#   privileged-install.sh --install <dir-of-debs>
#   privileged-install.sh --remove  <kernel-version-string>

set -uo pipefail

LOGFILE="/var/log/kernelpop.log"
log() {
    local msg="[kernelpop] $*"
    echo "$msg"
    echo "$(date '+%Y-%m-%d %H:%M:%S') $msg" >> "$LOGFILE" 2>/dev/null || true
}
die() { log "ERROR: $*"; exit 1; }

MODE="${1:-}"
ARG="${2:-}"

# ── Derive kernel version strings from .deb metadata ──────────────────
# The image/modules package NAME embeds the full version string:
#   linux-image-unsigned-7.1.3-070103-generic  ->  7.1.3-070103-generic
kver_from_debs() {
    local dir="$1"
    local deb pkg
    for deb in "$dir"/linux-image-*_amd64.deb "$dir"/linux-modules-*_amd64.deb; do
        [[ -f "$deb" ]] || continue
        pkg="$(dpkg-deb -f "$deb" Package 2>/dev/null)" || continue
        case "$pkg" in
            linux-image-unsigned-*) echo "${pkg#linux-image-unsigned-}"; return 0 ;;
            linux-image-*)          echo "${pkg#linux-image-}";          return 0 ;;
            linux-modules-*)        echo "${pkg#linux-modules-}";        return 0 ;;
        esac
    done
    return 1
}

do_install() {
    local dir="$1"
    [[ -d "$dir" ]] || die "Not a directory: $dir"

    local debs=("$dir"/*.deb)
    [[ -f "${debs[0]}" ]] || die "No .deb files found in $dir"

    log "==== Kernel install started ===="
    log "Package directory: $dir"

    # Version from package metadata, before anything is installed
    local kver
    kver="$(kver_from_debs "$dir")" || die "Could not read a kernel version from the package metadata"
    log "Kernel version (from package metadata): $kver"

    # Snapshot /lib/modules so the post-install cross-check is honest
    local before_modules
    before_modules="$(ls -1 /lib/modules 2>/dev/null || true)"

    log "Installing ${#debs[@]} packages…"
    if ! dpkg -i "${debs[@]}" >>"$LOGFILE" 2>&1; then
        log "dpkg -i reported errors — attempting to fix dependencies…"
        apt-get install -f -y >>"$LOGFILE" 2>&1 || die "dpkg install failed and apt-get -f could not repair it"
    fi
    log "Packages installed"

    # Cross-check: the modules directory for $kver must now exist
    if [[ ! -d "/lib/modules/$kver" ]]; then
        log "WARNING: /lib/modules/$kver not found after install."
        local new_dir
        new_dir="$(comm -13 <(echo "$before_modules") <(ls -1 /lib/modules) | head -1 || true)"
        if [[ -n "$new_dir" ]]; then
            log "Using newly appeared modules directory instead: $new_dir"
            kver="$new_dir"
        else
            die "No modules directory appeared for the installed kernel — aborting before initramfs"
        fi
    fi

    # ── Initramfs: generate AND verify ─────────────────────────────────
    log "Generating initramfs for $kver…"
    if [[ -f "/boot/initrd.img-$kver" ]]; then
        update-initramfs -u -k "$kver" >>"$LOGFILE" 2>&1 || die "update-initramfs -u failed for $kver"
    else
        update-initramfs -c -k "$kver" >>"$LOGFILE" 2>&1 || die "update-initramfs -c failed for $kver"
    fi

    [[ -f "/boot/initrd.img-$kver" ]] || die "initrd.img-$kver did NOT appear in /boot — DO NOT reboot into this kernel"
    log "Verified: /boot/initrd.img-$kver exists"

    log "Updating GRUB…"
    update-grub >>"$LOGFILE" 2>&1 || die "update-grub failed"

    log "==== Done. Kernel $kver installed with initramfs verified. Reboot when ready. ===="
}

do_remove() {
    local kver="$1"
    [[ -n "$kver" ]] || die "No kernel version specified"

    local running
    running="$(uname -r)"
    [[ "$kver" == "$running" ]] && die "Refusing to remove the RUNNING kernel ($running)"

    log "==== Kernel removal started: $kver ===="

    # Every package whose name embeds this exact version string
    local pkgs
    pkgs="$(dpkg-query -W -f '${Package}\n' "linux-*${kver}*" 2>/dev/null | sort -u || true)"
    # Also catch the versioned headers base package (no -generic suffix)
    local base="${kver%-generic}"
    local base_pkgs
    base_pkgs="$(dpkg-query -W -f '${Package}\n' "linux-headers-${base}" 2>/dev/null || true)"
    pkgs="$(printf '%s\n%s\n' "$pkgs" "$base_pkgs" | sort -u | sed '/^$/d')"

    if [[ -n "$pkgs" ]]; then
        log "Purging packages:"
        log "$pkgs"
        # shellcheck disable=SC2086
        apt-get purge -y $pkgs >>"$LOGFILE" 2>&1 || die "apt-get purge failed"
    else
        log "No packages own this kernel — removing files directly"
        rm -f "/boot/vmlinuz-$kver" "/boot/initrd.img-$kver" \
              "/boot/System.map-$kver" "/boot/config-$kver"
        rm -rf "/lib/modules/$kver"
    fi

    log "Updating GRUB…"
    update-grub >>"$LOGFILE" 2>&1 || die "update-grub failed"

    log "==== Done. Kernel $kver removed. ===="
}

case "$MODE" in
    --install) do_install "$ARG" ;;
    --remove)  do_remove  "$ARG" ;;
    *) die "Usage: $0 --install <dir-of-debs> | --remove <kernel-version>" ;;
esac

exit 0
