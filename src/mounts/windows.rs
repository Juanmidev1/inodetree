//! Discovery of mounted Windows drives via Win32 volume APIs.

use std::io;

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use super::MountInfo;

/// Lists mounted Windows drives with their volume label, filesystem, and capacity.
pub fn list_mounts() -> io::Result<Vec<MountInfo>> {
    // SAFETY: `GetLogicalDrives` takes no pointers and returns a 26-bit drive mask.
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut mounts = Vec::new();
    for bit in 0..26_u8 {
        if (mask & (1_u32 << bit)) == 0 {
            continue;
        }
        let letter = char::from(b'A' + bit);
        let mount_point = format!("{letter}:\\");
        let root_wide: Vec<u16> = mount_point.encode_utf16().chain(Some(0)).collect();

        // SAFETY: `root_wide` is NUL-terminated UTF-16.
        let drive_type = unsafe { GetDriveTypeW(root_wide.as_ptr()) };
        let is_supported = matches!(
            drive_type,
            DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_REMOTE | DRIVE_CDROM | DRIVE_RAMDISK
        );
        if !is_supported {
            continue;
        }

        let space = windows_drive_space(&root_wide);
        // Empty card readers and optical drives fail capacity queries; skip them so the picker stays clean.
        if matches!(drive_type, DRIVE_REMOVABLE | DRIVE_CDROM) && space.is_none() {
            continue;
        }
        let (total_bytes, free_bytes) = space.unwrap_or((0, 0));
        let (label, filesystem) = windows_volume_info(&root_wide);
        let fallback = match drive_type {
            DRIVE_REMOVABLE => "Removable Disk",
            DRIVE_REMOTE => "Network Drive",
            DRIVE_CDROM => "Optical Drive",
            DRIVE_RAMDISK => "RAM Disk",
            _ => "Local Disk",
        };
        let base_label = if label.is_empty() { fallback } else { &label };
        let device = if filesystem.is_empty() {
            base_label.to_owned()
        } else {
            format!("{base_label} ({filesystem})")
        };

        mounts.push(MountInfo {
            device,
            mount_point,
            filesystem,
            total_bytes,
            free_bytes,
        });
    }
    Ok(mounts)
}

fn windows_drive_space(root_wide: &[u16]) -> Option<(u64, u64)> {
    let mut free_to_caller = 0_u64;
    let mut total_bytes = 0_u64;
    // SAFETY: `root_wide` is NUL-terminated and both output pointers are valid writable `u64`s.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            root_wide.as_ptr(),
            &raw mut free_to_caller,
            &raw mut total_bytes,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some((total_bytes, free_to_caller))
}

fn windows_volume_info(root_wide: &[u16]) -> (String, String) {
    let mut label_buf = [0_u16; 261];
    let mut fs_buf = [0_u16; 261];
    // SAFETY: `root_wide` is NUL-terminated and both buffers match their passed lengths.
    let ok = unsafe {
        GetVolumeInformationW(
            root_wide.as_ptr(),
            label_buf.as_mut_ptr(),
            label_buf.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_buf.as_mut_ptr(),
            fs_buf.len() as u32,
        )
    };
    if ok == 0 {
        return (String::new(), String::new());
    }
    (utf16_z_to_string(&label_buf), utf16_z_to_string(&fs_buf))
}

fn utf16_z_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_lists_at_least_one_drive() {
        let mounts = list_mounts().unwrap();
        assert_ne!(mounts.len(), 0);
        assert!(
            mounts
                .iter()
                .all(|mount| mount.mount_point.ends_with(":\\") && mount.mount_point.len() == 3)
        );
    }
}
