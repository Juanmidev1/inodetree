//! macOS counterpart of `sys.rs`: `fstatat` and `readdir` instead of `statx` and `getdents64`.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub const S_IFDIR: u32 = libc::S_IFDIR as u32;
pub const S_IFREG: u32 = libc::S_IFREG as u32;
pub const S_IFLNK: u32 = libc::S_IFLNK as u32;
const S_IFMT: u32 = libc::S_IFMT as u32;

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

/// `follow_symlinks` is only meant for the scan root. Entries are never followed.
pub fn statx_at(dirfd: i32, name: &CStr, follow_symlinks: bool) -> io::Result<Metadata> {
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

pub fn statx_path(path: &Path) -> io::Result<Metadata> {
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    statx_at(libc::AT_FDCWD, &c_path, true)
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
