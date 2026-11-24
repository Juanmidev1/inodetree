//! Unix-specific mount-point guard and permanent deletion.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// The scanner shows other filesystems as empty folders, so deleting one would wipe unseen data.
pub(super) fn is_mount_point(path: &Path) -> io::Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    let Some(parent) = path.parent().filter(|_| metadata.is_dir()) else {
        return Ok(false);
    };
    Ok(std::fs::symlink_metadata(parent)?.dev() != metadata.dev())
}

pub(super) fn remove_permanently(path: &Path) -> io::Result<()> {
    // `symlink_metadata` keeps a symlink to a directory from being treated as the directory
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
