//! Parallel directory walker backed by platform-specific directory readers.

use std::io;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use rayon::{Scope, ThreadPoolBuilder};

use super::sys;
use super::{HardlinkSet, ScanConfig, ScanProgress, ScanResult};
use crate::arena::{Arena, ChildBatch, NodeId, NodeKind, ROOT};

// More threads than this make a spinning disk's head jump between directories (thrashing),
// which is slower than a few mostly sequential readers
const HDD_SCAN_THREADS: usize = 3;

struct Context<'a> {
    arena: Mutex<Arena>,
    hardlinks: HardlinkSet,
    root_dev: u64,
    config: &'a ScanConfig,
}

pub fn scan(config: &ScanConfig) -> io::Result<ScanResult> {
    let started = Instant::now();
    let root = sys::canonical_root(&config.root)?;
    let root_meta = sys::stat_root(&root)?;
    let root_arena = Arena::with_root(
        &root.to_string_lossy(),
        root_meta.logical_size,
        root_meta.allocated_size,
        root_meta.mtime_secs,
    );
    let context = Context {
        arena: Mutex::new(root_arena),
        hardlinks: HardlinkSet::default(),
        root_dev: root_meta.dev,
        config,
    };
    walk(&context, &root);
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

fn walk(context: &Context<'_>, root: &Path) {
    // If the small pool cannot be built the scan still runs, just on the global pool
    let hdd_pool = sys::is_rotational(root, context.root_dev)
        .then(|| {
            ThreadPoolBuilder::new()
                .num_threads(HDD_SCAN_THREADS)
                .build()
                .ok()
        })
        .flatten();
    match &hdd_pool {
        Some(pool) => pool.scope(|scope| scan_dir(scope, context, root, ROOT)),
        None => rayon::scope(|scope| scan_dir(scope, context, root, ROOT)),
    }
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
    let (batch, subdirs) = match sys::read_directory(path, context.root_dev, &context.hardlinks) {
        Ok(result) => result,
        Err(error) => {
            if error.kind() == io::ErrorKind::PermissionDenied {
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
