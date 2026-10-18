//! Disk listing for the drives screen.
//!
//! Thin wrapper around `sysinfo::Disks`, refreshed on every call.

use sysinfo::{DiskKind, Disks};

use crate::model::DriveInfo;

/// Maps every `Disk` reported by sysinfo to a [`DriveInfo`].
///
/// Mounts without any reported total are skipped. The result is sorted with
/// non-removable disks first, then by mount point.
pub fn list_drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut drives = Vec::with_capacity(disks.list().len());

    for disk in disks.list() {
        let total = disk.total_space();
        if total == 0 {
            continue;
        }

        let name = disk.name().to_string_lossy();
        let name = if name.is_empty() { "Local Disk" } else { name.as_ref() };
        let kind = match disk.kind() {
            DiskKind::SSD => "ssd",
            DiskKind::HDD => "hdd",
            _ => "unknown",
        };

        drives.push(DriveInfo {
            name: name.to_string(),
            mount: disk.mount_point().to_string_lossy().to_string(),
            fs: disk.file_system().to_string_lossy().to_string(),
            total,
            free: disk.available_space(),
            kind: kind.to_string(),
            is_removable: disk.is_removable(),
        });
    }

    // Non-removable disks first, then ordered by mount point.
    drives.sort_by(|a, b| {
        let a_removable = !a.is_removable;
        let b_removable = !b.is_removable;
        b_removable.cmp(&a_removable).then_with(|| a.mount.cmp(&b.mount))
    });

    drives
}
