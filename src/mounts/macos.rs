//! macOS mount discovery with `getmntinfo`.

use std::ffi::CStr;
use std::io;

use super::MountInfo;

const REAL_FILESYSTEMS: &[&str] = &[
    "apfs", "hfs", "exfat", "msdos", "ntfs", "smbfs", "nfs", "ufs",
];

/// Lists mounted local filesystems with their capacity.
pub fn list_mounts() -> io::Result<Vec<MountInfo>> {
    let mut entries: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: `getmntinfo` points `entries` at an internal buffer that stays valid until the next call
    let count = unsafe { libc::getmntinfo(&raw mut entries, libc::MNT_NOWAIT) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `getmntinfo` returned `count` initialized records
    let records = unsafe { std::slice::from_raw_parts(entries, count as usize) };
    Ok(records.iter().filter_map(to_mount).collect())
}

fn to_mount(record: &libc::statfs) -> Option<MountInfo> {
    let filesystem = c_chars_to_string(&record.f_fstypename);
    let mount_point = c_chars_to_string(&record.f_mntonname);
    if !is_listed(&filesystem, &mount_point) {
        return None;
    }
    let block_size = u64::from(record.f_bsize);
    Some(MountInfo {
        device: c_chars_to_string(&record.f_mntfromname),
        mount_point,
        filesystem,
        total_bytes: record.f_blocks.saturating_mul(block_size),
        free_bytes: record.f_bavail.saturating_mul(block_size),
    })
}

/// System-internal APFS volumes (VM, Preboot, Update, ...) are not useful scan roots, except Data.
fn is_listed(filesystem: &str, mount_point: &str) -> bool {
    REAL_FILESYSTEMS.contains(&filesystem)
        && (!mount_point.starts_with("/System/Volumes/") || mount_point == "/System/Volumes/Data")
}

fn c_chars_to_string(raw: &[libc::c_char]) -> String {
    // SAFETY: the kernel NUL-terminates these fixed-size fields
    unsafe { CStr::from_ptr(raw.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_data_volume_but_hides_system_internals() {
        assert!(is_listed("apfs", "/System/Volumes/Data"));
        assert!(is_listed("apfs", "/"));
        assert!(is_listed("exfat", "/Volumes/Backup"));
        assert!(!is_listed("apfs", "/System/Volumes/VM"));
        assert!(!is_listed("devfs", "/dev"));
    }

    #[test]
    fn root_filesystem_is_listed_with_capacity() {
        let mounts = list_mounts().unwrap();
        let root = mounts
            .iter()
            .find(|mount| mount.mount_point == "/")
            .unwrap();
        assert!(root.total_bytes > 0);
    }
}
