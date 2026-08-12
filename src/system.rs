//! system.rs — Inventory of installed kernels and their boot health.
//!
//! The core safety feature of this app lives here: every kernel found in
//! /boot is checked for a matching initrd.img and /lib/modules directory,
//! so a kernel that would VFS-panic on boot is visible BEFORE the reboot.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub running_kernel: String,
    pub kernels: Vec<InstalledKernel>,
    pub free_boot_bytes: Option<u64>,
    pub free_root_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct InstalledKernel {
    /// Full version string, e.g. "7.1.3-070103-generic"
    pub version: String,
    pub has_initrd: bool,
    pub has_modules: bool,
    pub running: bool,
}

impl InstalledKernel {
    /// A kernel is bootable when its initrd and modules are both in place.
    pub fn healthy(&self) -> bool {
        self.has_initrd && self.has_modules
    }
}

pub fn query_system() -> SystemInfo {
    let running_kernel = get_running_kernel();
    SystemInfo {
        kernels: get_installed_kernels(&running_kernel),
        running_kernel,
        free_boot_bytes: get_free_disk("/boot"),
        free_root_bytes: get_free_disk("/"),
    }
}

/// uname -r
fn get_running_kernel() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Every vmlinuz-* in /boot, with initrd and modules presence checks.
/// Sorted newest-looking first, with the running kernel pinned to the top.
fn get_installed_kernels(running: &str) -> Vec<InstalledKernel> {
    let mut kernels = vec![];

    let Ok(entries) = std::fs::read_dir("/boot") else { return kernels };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(version) = name.strip_prefix("vmlinuz-") else { continue };

        let has_initrd = Path::new(&format!("/boot/initrd.img-{}", version)).exists();
        let has_modules = Path::new(&format!("/lib/modules/{}", version)).exists();

        kernels.push(InstalledKernel {
            version: version.to_string(),
            has_initrd,
            has_modules,
            running: version == running,
        });
    }

    kernels.sort_by(|a, b| {
        let key = |k: &InstalledKernel| -> Vec<u32> {
            k.version
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|s| s.parse().ok())
                .collect()
        };
        b.running
            .cmp(&a.running)
            .then_with(|| key(b).cmp(&key(a)))
    });
    kernels
}

/// Free space at a mount point via df -B1.
fn get_free_disk(path: &str) -> Option<u64> {
    let out = Command::new("df")
        .args(["-B1", "--output=avail", path])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .nth(1)
        .and_then(|l| l.trim().parse::<u64>().ok())
}

/// Minimum free space to comfortably download + unpack a kernel set (bytes).
pub const MIN_ROOT_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB

/// /boot fills up fast with kernels; warn below this.
pub const MIN_BOOT_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

pub fn format_bytes(b: u64) -> String {
    if b >= 1_073_741_824 {
        format!("{:.1} GB", b as f64 / 1_073_741_824.0)
    } else if b >= 1_048_576 {
        format!("{:.1} MB", b as f64 / 1_048_576.0)
    } else {
        format!("{} KB", b / 1024)
    }
}

/// Compare the running kernel's numeric part against a mainline version
/// string like "7.1.3". Returns Newer/Same/Older from the candidate's
/// point of view, mirroring GreenLight's badge logic.
#[derive(PartialEq)]
pub enum VersionRelation { Newer, Same, Older, Unknown }

pub fn compare_to_running(running: &str, candidate: &str) -> VersionRelation {
    // Running looks like "7.1.3-070103-generic" or "7.0.0-27-generic";
    // candidate may be a plain "7.1.3" or an RC like "7.2-rc1" — either way
    // the leading dotted-numeric part is what's comparable.
    let head = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect()
    };
    let parse = |s: &str| -> Vec<u32> {
        s.split('.').filter_map(|x| x.parse().ok()).collect()
    };
    let mut rv = parse(&head(running));
    let mut cv = parse(&head(candidate));
    if rv.is_empty() || cv.is_empty() {
        return VersionRelation::Unknown;
    }
    // Normalize lengths so "7.1" and "7.1.0" compare as equal.
    while rv.len() < 3 { rv.push(0); }
    while cv.len() < 3 { cv.push(0); }
    match cv.cmp(&rv) {
        std::cmp::Ordering::Greater => VersionRelation::Newer,
        std::cmp::Ordering::Equal   => VersionRelation::Same,
        std::cmp::Ordering::Less    => VersionRelation::Older,
    }
}
