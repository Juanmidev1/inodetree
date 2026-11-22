//! Parallel `getdents64` + `statx` directory walker.

use std::cell::RefCell;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use rayon::Scope;

use super::sys::{self, Metadata};
use super::{HardlinkSet, ScanConfig, ScanProgress, ScanResult};
use crate::arena::{Arena, ChildBatch, NewNode, NodeId, NodeKind, ROOT};

const DIRENT_BUFFER_BYTES: usize = 64 * 1024;

thread_local! {
    static DIRENT_BUFFER: RefCell<Vec<u8>> = RefCell::new(vec![0; DIRENT_BUFFER_BYTES]);
}

struct Context<'a> {
    arena: Mutex<Arena>,
    hardlinks: HardlinkSet,
    root_dev: u64,
    config: &'a ScanConfig,
}

/// Subdirectories to descend into: batch index and the raw (possibly non-UTF-8) name.
type Subdirs = Vec<(usize, OsString)>;

pub fn scan(config: &ScanConfig) -> io::Result<ScanResult> {
    let started = Instant::now();
    let root = std::fs::canonicalize(&config.root)?;
    let root_meta = sys::statx_path(&root)?;
    if root_meta.file_type() != libc::S_IFDIR {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scan root is not a directory",
        ));
    }
    let root_arena = Arena::with_root(
        &root.to_string_lossy(),
        root_meta.size,
        root_meta.blocks * 512,
        root_meta.mtime_secs,
    );
    let context = Context {
        arena: Mutex::new(root_arena),
        hardlinks: HardlinkSet::default(),
        root_dev: root_meta.dev,
        config,
    };
    rayon::scope(|scope| scan_dir(scope, &context, &root, ROOT));
    // A poisoned lock still guards a structurally valid arena, since batches are inserted atomically
    let mut arena = context
        .arena
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    arena.aggregate();
    Ok(ScanResult {
        arena,
        elapsed: started.elapsed(),
        was_cancelled: config.cancel.load(Relaxed),
    })
}

fn scan_dir<'scope>(
    scope: &Scope<'scope>,
    context: &'scope Context<'scope>,
    path: &Path,
    id: NodeId,
) {
    if context.config.cancel.load(Relaxed) {
        return;
    }
    let (batch, subdirs) = match read_directory(context, path) {
        Ok(result) => result,
        Err(error) => {
            if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) {
                context
                    .config
                    .progress
                    .unreadable_dirs
                    .fetch_add(1, Relaxed);
            }
            return;
        }
    };
    record_progress(&context.config.progress, &batch);
    let first_child = {
        let mut arena = context.arena.lock().unwrap_or_else(PoisonError::into_inner);
        arena.append_children(id, &batch)
    };
    for (index, name) in subdirs {
        let child_path = path.join(name);
        let child_id = first_child + index as NodeId;
        scope.spawn(move |scope| scan_dir(scope, context, &child_path, child_id));
    }
}

fn read_directory(context: &Context<'_>, path: &Path) -> io::Result<(ChildBatch, Subdirs)> {
    let dir = sys::open_dir(path)?;
    let mut batch = ChildBatch::default();
    let mut subdirs = Subdirs::new();
    DIRENT_BUFFER.with(|buffer| {
        sys::read_entries(dir.as_fd(), &mut buffer.borrow_mut(), |name| {
            // Entries that vanish or fail to stat mid-scan are skipped rather than aborting
            if let Ok(meta) = sys::statx_at(dir.as_raw_fd(), name, false) {
                add_entry(context, &mut batch, &mut subdirs, name.to_bytes(), &meta);
            }
        })
    })?;
    Ok((batch, subdirs))
}

fn add_entry(
    context: &Context<'_>,
    batch: &mut ChildBatch,
    subdirs: &mut Subdirs,
    raw_name: &[u8],
    meta: &Metadata,
) {
    let kind = match meta.file_type() {
        libc::S_IFDIR => NodeKind::Directory,
        libc::S_IFREG => NodeKind::File,
        libc::S_IFLNK => NodeKind::Symlink,
        _ => NodeKind::Other,
    };
    let is_duplicate = kind != NodeKind::Directory
        && meta.nlink > 1
        && !context.hardlinks.is_first_sighting(meta.dev, meta.ino);
    let (logical_size, allocated_size) = if is_duplicate {
        (0, 0)
    } else {
        (meta.size, meta.blocks * 512)
    };
    let name_start = batch.names.len();
    batch.names.push_str(&String::from_utf8_lossy(raw_name));
    // Directories on another device (mount points) stay as empty leaves instead of being entered
    if kind == NodeKind::Directory && meta.dev == context.root_dev {
        subdirs.push((batch.entries.len(), OsString::from_vec(raw_name.to_vec())));
    }
    batch.entries.push(NewNode {
        name_start,
        name_end: batch.names.len(),
        kind,
        logical_size,
        allocated_size,
        mtime_secs: meta.mtime_secs,
        is_hardlink_duplicate: is_duplicate,
    });
}

fn record_progress(progress: &ScanProgress, batch: &ChildBatch) {
    let dirs = batch
        .entries
        .iter()
        .filter(|entry| entry.kind == NodeKind::Directory)
        .count();
    let allocated: u64 = batch.entries.iter().map(|entry| entry.allocated_size).sum();
    progress.dirs.fetch_add(dirs as u64, Relaxed);
    progress
        .files
        .fetch_add((batch.entries.len() - dirs) as u64, Relaxed);
    progress.allocated_bytes.fetch_add(allocated, Relaxed);
}
