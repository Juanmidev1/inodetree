//! User actions on scanned entries. Disk operations never run on the UI thread.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use crate::arena::{Arena, NodeId, ROOT};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

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
    if platform::is_mount_point(&path)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete a mount point: its contents were not scanned",
        ));
    }
    Ok(path)
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
        DeleteMode::Permanent => {
            platform::remove_permanently(path).map_err(|error| error.to_string())
        }
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
