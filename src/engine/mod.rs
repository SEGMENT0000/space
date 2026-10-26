//! Read-only scan engine. Filesystem enumeration, classification and drives.

mod classify;
mod scan;

pub use classify::{classify_entry, classify_file};
pub use scan::{ScanHandle, scan_blocking, spawn_scan};

use crate::model::DriveInfo;

/// List mounted volumes. Refreshed on every call; no caching at this layer.
pub fn list_drives() -> Vec<DriveInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut drives = Vec::with_capacity(disks.list().len());

    for disk in disks.list() {
        let total = disk.total_space();
        if total == 0 {
            continue;
        }
        let raw_name = disk.name().to_string_lossy().trim().to_string();
        let mount = disk.mount_point().to_string_lossy().to_string();
        let name = if raw_name.is_empty() {
            mount.trim_end_matches(['\\', '/']).to_string()
        } else {
            raw_name
        };
        let kind = match disk.kind() {
            sysinfo::DiskKind::SSD => "ssd",
            sysinfo::DiskKind::HDD => "hdd",
            _ if disk.is_removable() => "removable",
            _ => "unknown",
        };
        drives.push(DriveInfo {
            name,
            mount,
            fs: disk.file_system().to_string_lossy().to_string(),
            total,
            free: disk.available_space(),
            kind: kind.to_string(),
            is_removable: disk.is_removable(),
        });
    }

    drives.sort_by(|a, b| {
        a.is_removable
            .cmp(&b.is_removable)
            .then_with(|| a.mount.cmp(&b.mount))
    });
    drives
}
