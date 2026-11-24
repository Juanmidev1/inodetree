//! Windows-specific volume mount-point guard and reparse-safe permanent deletion.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, GetVolumePathNameW,
};

/// Refuse drive roots and directory volume mount points whose volume root differs from their
/// parent directory's volume root. Regular symlinks and same-volume junctions stay deletable
/// (only the link/junction itself is removed).
pub(super) fn is_mount_point(path: &Path) -> io::Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    if (metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY) == 0 {
        return Ok(false);
    }
    if metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let Some(parent) = path.parent() else {
        return Ok(true);
    };
    Ok(!same_volume_root(path, parent)?)
}

fn same_volume_root(a: &Path, b: &Path) -> io::Result<bool> {
    let vol_a = volume_path_name(a)?;
    let vol_b = volume_path_name(b)?;
    Ok(vol_a.eq_ignore_ascii_case(&vol_b))
}

fn volume_path_name(path: &Path) -> io::Result<String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let cap = wide.len().max(MAX_PATH as usize) + 16;
    let mut out = vec![0_u16; cap];
    // SAFETY: `wide` is NUL-terminated and `out` has capacity `cap` UTF-16 code units.
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), out.as_mut_ptr(), out.len() as u32) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let len = out.iter().position(|&c| c == 0).unwrap_or(out.len());
    Ok(String::from_utf16_lossy(&out[..len]))
}

pub(super) fn remove_permanently(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let attrs = meta.file_attributes();
    if (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        if (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0 {
            // Directory symlinks and NTFS junctions are removed with `remove_dir` without touching target contents.
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        }
    } else if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
