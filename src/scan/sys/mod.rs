//! Platform-specific filesystem metadata and fast directory reading.

use std::ffi::OsString;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{canonical_root, is_rotational, read_directory, stat_root};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{canonical_root, is_rotational, read_directory, stat_root};

/// Subdirectories to descend into: batch index and the raw name.
pub type Subdirs = Vec<(usize, OsString)>;

#[derive(Debug, Clone, Copy)]
pub struct RootMeta {
    pub dev: u64,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub mtime_secs: i64,
}
