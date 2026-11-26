//! Platform backends for the scanner's filesystem access. Each backend exposes `stat_at`,
//! `is_rotational`, `read_entries` and the `S_IF*` constants. The helpers that only rely on
//! POSIX calls live here and are shared by every backend.

use std::ffi::CString;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

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

#[derive(Debug, Clone, Copy)]
pub struct Metadata {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u32,
    pub mode: u32,
    pub size: u64,
    pub blocks: u64,
    pub mtime_secs: i64,
}

impl Metadata {
    #[must_use]
    pub fn file_type(&self) -> u32 {
        self.mode & S_IFMT
    }
}

pub fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `c_path` is a valid NUL-terminated string for the duration of the call
    let fd = unsafe { libc::open(c_path.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `open` just returned this descriptor and nothing else owns it
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn stat_path(path: &Path) -> io::Result<Metadata> {
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    stat_at(libc::AT_FDCWD, &c_path, true)
}
