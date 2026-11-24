//! Platform backends for the scanner's filesystem access. Each backend exposes the same
//! interface: `Metadata`, `open_dir`, `statx_at`, `statx_path`, `is_rotational`,
//! `read_entries` and the `S_IF*` constants.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("inodetree supports only Linux and macOS");
