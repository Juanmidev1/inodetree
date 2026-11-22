//! Filesystem scanning: shared configuration, progress counters and result types.

pub mod sys;
pub mod vfs;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::arena::Arena;

const HARDLINK_SHARDS: usize = 64;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub root: PathBuf,
    /// Set to `true` from any thread to stop the scan within milliseconds
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<ScanProgress>,
}

impl ScanConfig {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cancel: Arc::default(),
            progress: Arc::default(),
        }
    }
}

/// Live counters that a UI can poll while the scan runs.
#[derive(Debug, Default)]
pub struct ScanProgress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub allocated_bytes: AtomicU64,
    pub unreadable_dirs: AtomicU64,
}

#[derive(Debug)]
pub struct ScanResult {
    pub arena: Arena,
    pub elapsed: Duration,
    pub was_cancelled: bool,
}

/// Sharded set of `(dev, ino)` pairs, so threads rarely contend on the same lock.
#[derive(Debug)]
pub struct HardlinkSet {
    shards: Vec<Mutex<HashSet<(u64, u64)>>>,
}

impl Default for HardlinkSet {
    fn default() -> Self {
        Self {
            shards: (0..HARDLINK_SHARDS).map(|_| Mutex::default()).collect(),
        }
    }
}

impl HardlinkSet {
    /// Returns `true` only the first time an inode is seen.
    pub fn is_first_sighting(&self, dev: u64, ino: u64) -> bool {
        let shard = &self.shards[(ino as usize) % HARDLINK_SHARDS];
        // A poisoned shard still holds a valid set: the panic happened in another thread's insert
        let mut seen = shard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.insert((dev, ino))
    }
}
