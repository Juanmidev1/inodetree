//! User actions on scanned entries. Disk operations never run on the UI thread.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use crate::arena::{Arena, NodeId, ROOT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Debug)]
pub struct ActionResult {
    pub id: NodeId,
    pub outcome: Result<(), String>,
}

/// Rebuilds the on-disk path of `id` and refuses targets that must never be deleted.
pub fn target_path(arena: &Arena, id: NodeId) -> io::Result<PathBuf> {
    if id == ROOT {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete the scan root",
        ));
    }
    // Lossy names from non-UTF-8 entries rebuild a path that does not exist, so nothing is touched
    let path = PathBuf::from(arena.path_of(id));
    if is_mount_point(&path)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete a mount point: its contents were not scanned",
        ));
    }
    Ok(path)
}

/// The scanner shows other filesystems as empty folders, so deleting one would wipe unseen data.
fn is_mount_point(path: &Path) -> io::Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    let Some(parent) = path.parent().filter(|_| metadata.is_dir()) else {
        return Ok(false);
    };
    Ok(std::fs::symlink_metadata(parent)?.dev() != metadata.dev())
}

pub fn open_in_file_manager(path: &Path) -> io::Result<()> {
    let is_directory = std::fs::symlink_metadata(path)?.is_dir();
    let target = if is_directory {
        Some(path)
    } else {
        path.parent()
    };
    let target = target.ok_or_else(|| io::Error::other("path has no parent directory"))?;
    open::that_detached(target)
}

pub fn delete_path(path: &Path, mode: DeleteMode) -> Result<(), String> {
    match mode {
        DeleteMode::Trash => trash::delete(path).map_err(|error| error.to_string()),
        DeleteMode::Permanent => remove_permanently(path).map_err(|error| error.to_string()),
    }
}

fn remove_permanently(path: &Path) -> io::Result<()> {
    // `symlink_metadata` keeps a symlink to a directory from being treated as the directory
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Deletes on a worker thread and reports through `results`; `on_done` wakes the UI.
pub fn spawn_delete(
    id: NodeId,
    path: PathBuf,
    mode: DeleteMode,
    results: Sender<ActionResult>,
    on_done: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    thread::Builder::new()
        .name("inodetree-delete".into())
        .spawn(move || {
            let outcome = delete_path(&path, mode);
            // A closed channel means the app is shutting down, so the result is no longer needed
            let _ = results.send(ActionResult { id, outcome });
            on_done();
        })?;
    Ok(())
}
