# Changelog

## 1.0.8 — Fix UPDATE_INFORMATION to reference .zsync sidecar

- Per the AppImage update spec, the GitHub Releases zsync transport string
  must end in the `.zsync` sidecar filename, not the AppImage filename.
  `UPDATE_INFORMATION` in `build-appimage.sh` ended in
  `-x86_64.AppImage` instead of `-x86_64.AppImage.zsync`, which broke
  update detection in tools like Gear Lever even though the `.zsync`
  sidecar itself was already being generated and published correctly.
  Packaging-only fix, no application behavior changes.

## 1.0.7 — Fix phantom taskbar window on kernel removal

- The "Remove kernel X?" confirmation used `libadwaita::MessageDialog`,
  which is deprecated (libadwaita 1.2+, replaced by `AlertDialog`) and is
  itself a `Gtk.Window` subclass — a real separate top-level Wayland
  surface. On GNOME/Wayland that surface doesn't inherit the app's
  icon/app_id, so the compositor showed it in the dock as a second,
  unnamed, generic-icon window alongside MKI's own. Switched to
  `libadwaita::AlertDialog`, which is an `Adw.Dialog` and renders as a
  floating sheet inside the parent window's own surface, so no second
  top-level window is created. Requires the `v1_5` libadwaita feature.

## 1.0.6 — Generate .zsync directly with zsyncmake

- The 1.0.5 diagnostics showed zsync is installed and zsyncmake is on
  PATH and works — but appimagetool's own built-in zsync generation was
  still silently producing no .zsync file, with no warning either way.
  This appimagetool build (continuous, git 8c8c91f) most likely probes
  zsyncmake with a long-option flag (e.g. --version) to check it's
  usable before generating the sidecar; the installed zsyncmake build
  only understands old-style short options, so that probe fails and
  appimagetool quietly skips zsync generation instead of erroring.
  Rather than depend on appimagetool's internal detection, the .zsync
  is now built directly with a standalone `zsyncmake "$OUT"` call right
  after the AppImage is packed. This step is non-fatal: if it fails for
  any reason, the build still succeeds since the AppImage itself is
  already valid without it.

## 1.0.5 — Diagnostic-only release

- 1.0.4 made zsync's install unconditional, but the release still isn't
  producing a .zsync file. Added explicit diagnostic output right before
  the appimagetool invocation — dpkg status for the zsync package,
  `which zsyncmake`, and `zsyncmake --version` — to pin down definitively
  whether zsync is installed, whether zsyncmake is on PATH, and whether
  it actually runs on the CI runner. No functional change otherwise.

## 1.0.4 — Fix zsync still not installing in CI

- 1.0.3 added zsync to the build-dependencies apt-get install line, but
  that whole block is gated behind a check for whether cargo is already
  on PATH. In CI, a prior "Set up Rust Toolchain" step already installs
  cargo, so the guard evaluated false and the entire block — zsync
  included — was silently skipped, same as before 1.0.3. zsync is now
  installed unconditionally, ahead of the guarded block.

## 1.0.3 — Fix missing .zsync file

- The build runner never had zsync installed, so appimagetool silently
  skipped generating the .zsync file even though UPDATE_INFORMATION was
  already set in 1.0.2 — update-aware tools had nothing to delta-update
  against. zsync is now installed alongside the other build dependencies.

## 1.0.0 — Initial release

- Browse stable Ubuntu mainline kernel versions from kernel.ubuntu.com,
  with badges comparing each version against the running kernel and a
  filter field. Both the flat and amd64/ subdirectory page layouts are
  supported.
- Download the four generic amd64 packages (image-unsigned, modules,
  headers-generic, headers-all) with per-file progress, retries,
  cancellation, and SHA256 verification against the published CHECKSUMS
  file. A file that fails verification is deleted and the download
  aborts.
- Privileged install via polkit: dpkg install, then initramfs generation
  keyed on the kernel version read from .deb package metadata (never
  parsed from filenames), cross-checked against /lib/modules, then GRUB
  update. The install fails loudly if initrd.img does not appear in
  /boot, so a silently unbootable kernel cannot be produced.
- System tab lists every kernel in /boot with initrd and modules health
  checks; a window-wide warning banner appears whenever any installed
  kernel is missing its initramfs. Disk space on /boot and / is shown
  with low-space warnings.
- Old kernels can be removed from the System tab (packages purged, GRUB
  updated); the running kernel is never removable.
- AppImage-only distribution. First launch installs the privileged
  script and polkit policy to system paths via pkexec.

## 1.0.1 — Fix AppImage version metadata

- build-appimage.sh computed VERSION from Cargo.toml but never exported
  it to appimagetool, which fell back to a git commit hash. Package
  managers like Gear Lever showed "Mainline Kernel Installer (05706c)"
  instead of a real version. VERSION is now passed into appimagetool's
  environment alongside ARCH.

## 1.0.2 — Enable update checking

- Embedded UPDATE_INFORMATION in the AppImage so update-aware tools
  (Gear Lever, AppImageUpdate) can check GitHub Releases for newer
  versions and delta-update via zsync. CI now also uploads the .zsync
  file alongside the AppImage.
