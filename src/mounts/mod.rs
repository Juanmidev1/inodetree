//! Discovery of real local filesystems, with one backend per platform.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::{list_mounts, parse_mountinfo};
#[cfg(target_os = "macos")]
pub use macos::list_mounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountInfo {
    pub device: String,
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}
