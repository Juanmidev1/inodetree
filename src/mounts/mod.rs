//! Discovery of real local filesystems.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{list_mounts, parse_mountinfo};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::list_mounts;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn list_mounts() -> std::io::Result<Vec<MountInfo>> {
    Ok(Vec::new())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountInfo {
    pub device: String,
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}
