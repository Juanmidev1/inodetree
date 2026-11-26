//! macOS counterpart of `linux.rs`: `fstatat` and `readdir` instead of `statx` and `getdents64`.

use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

use super::Metadata;

pub const S_IFDIR: u32 = libc::S_IFDIR as u32;
pub const S_IFREG: u32 = libc::S_IFREG as u32;
pub const S_IFLNK: u32 = libc::S_IFLNK as u32;
pub const S_IFMT: u32 = libc::S_IFMT as u32;

/// `follow_symlinks` is only meant for the scan root. Entries are never followed.
pub fn stat_at(dirfd: i32, name: &CStr, follow_symlinks: bool) -> io::Result<Metadata> {
    let flags = if follow_symlinks {
        0
    } else {
        libc::AT_SYMLINK_NOFOLLOW
    };
    let mut buffer = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `name` is NUL-terminated and `buffer` is a valid out-pointer for one `stat`
    let result = unsafe { libc::fstatat(dirfd, name.as_ptr(), buffer.as_mut_ptr(), flags) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstatat` returned 0, so it fully initialized the struct
    let raw = unsafe { buffer.assume_init() };
    Ok(Metadata {
        dev: u64::from(raw.st_dev as u32),
        ino: raw.st_ino,
        nlink: u32::from(raw.st_nlink),
        mode: u32::from(raw.st_mode),
        size: raw.st_size as u64,
        blocks: raw.st_blocks as u64,
        mtime_secs: raw.st_mtime,
    })
}

/// Rotational detection is not implemented on macOS yet, so every device uses the full thread pool.
#[must_use]
pub fn is_rotational(_dev: u64) -> bool {
    false
}

/// Calls `visit` with the inode number and name of every entry except `.` and `..`.
/// `buffer` is unused: `readdir` buffers internally.
pub fn read_entries(
    dir: BorrowedFd<'_>,
    _buffer: &mut [u8],
    mut visit: impl FnMut(u64, &CStr),
) -> io::Result<()> {
    // SAFETY: `closedir` closes the descriptor it was given, so it gets its own duplicate
    let duplicate = unsafe { libc::dup(dir.as_raw_fd()) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `duplicate` is a valid directory descriptor owned by this function
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: `fdopendir` failed, so the duplicate is still ours to close
        unsafe { libc::close(duplicate) };
        return Err(error);
    }
    loop {
        // SAFETY: `__error` points at this thread's errno; `readdir` only sets it on failure
        unsafe { *libc::__error() = 0 };
        // SAFETY: `stream` is a valid open directory stream
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            // NULL means either the end of the stream (errno stays 0) or an I/O error
            let error = io::Error::last_os_error();
            // SAFETY: `stream` is open and not used afterwards
            unsafe { libc::closedir(stream) };
            return match error.raw_os_error() {
                Some(0) => Ok(()),
                _ => Err(error),
            };
        }
        // SAFETY: `readdir` returned a valid entry whose name is NUL-terminated
        let (ino, name) = unsafe { ((*entry).d_ino, CStr::from_ptr((*entry).d_name.as_ptr())) };
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            visit(ino, name);
        }
    }
}
