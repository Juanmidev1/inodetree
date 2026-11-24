//! Unix `getdents64` + `statx` directory reader and metadata helpers.

use std::cell::RefCell;
use std::ffi::{CStr, CString, OsString};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use super::{RootMeta, Subdirs};
use crate::arena::{ChildBatch, NewNode, NodeKind};
use crate::scan::HardlinkSet;

const STATX_MASK: u32 = libc::STATX_TYPE
    | libc::STATX_MODE
    | libc::STATX_NLINK
    | libc::STATX_SIZE
    | libc::STATX_BLOCKS
    | libc::STATX_MTIME
    | libc::STATX_INO;

// Layout of `linux_dirent64`: ino (8) + off (8) + reclen (2) + type (1) + NUL-terminated name
const DIRENT_INO_OFFSET: usize = 0;
const DIRENT_RECLEN_OFFSET: usize = 16;
const DIRENT_NAME_OFFSET: usize = 19;
const DIRENT_BUFFER_BYTES: usize = 64 * 1024;

thread_local! {
    static DIRENT_BUFFER: RefCell<Vec<u8>> = RefCell::new(vec![0; DIRENT_BUFFER_BYTES]);
}

#[derive(Debug, Clone, Copy)]
struct Metadata {
    dev: u64,
    ino: u64,
    nlink: u32,
    mode: u32,
    size: u64,
    blocks: u64,
    mtime_secs: i64,
}

impl Metadata {
    fn file_type(self) -> u32 {
        self.mode & libc::S_IFMT
    }
}

pub fn canonical_root(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

pub fn stat_root(path: &Path) -> io::Result<RootMeta> {
    let root_meta = statx_path(path)?;
    if root_meta.file_type() != libc::S_IFDIR {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scan root is not a directory",
        ));
    }
    Ok(RootMeta {
        dev: root_meta.dev,
        logical_size: root_meta.size,
        allocated_size: root_meta.blocks * 512,
        mtime_secs: root_meta.mtime_secs,
    })
}

/// Whether the block device behind `dev` reports itself as rotational (a spinning HDD).
///
/// Devices without a sysfs entry, such as the anonymous ones of btrfs or ZFS, count as
/// non-rotational.
#[must_use]
pub fn is_rotational(_root: &Path, dev: u64) -> bool {
    let device_dir = format!("/sys/dev/block/{}:{}", libc::major(dev), libc::minor(dev));
    // Partitions have no queue of their own, so their parent disk is consulted instead
    ["queue/rotational", "../queue/rotational"]
        .iter()
        .find_map(|relative| std::fs::read(format!("{device_dir}/{relative}")).ok())
        .is_some_and(|content| content.first() == Some(&b'1'))
}

pub fn read_directory(
    path: &Path,
    root_dev: u64,
    hardlinks: &HardlinkSet,
) -> io::Result<(ChildBatch, Subdirs)> {
    let dir = open_dir(path)?;
    let mut batch = ChildBatch::default();
    let mut subdirs = Subdirs::new();
    for (_, name) in collect_entries_by_inode(&dir)? {
        // Entries that vanish or fail to stat mid-scan are skipped rather than aborting
        if let Ok(meta) = statx_at(dir.as_raw_fd(), &name, false) {
            add_entry(
                root_dev,
                hardlinks,
                &mut batch,
                &mut subdirs,
                name.to_bytes(),
                &meta,
            );
        }
    }
    Ok((batch, subdirs))
}

/// Entries sorted by inode number, so the following `statx` calls read the inode table
/// (or NTFS MFT) mostly sequentially instead of seeking at random on spinning disks.
fn collect_entries_by_inode(dir: &OwnedFd) -> io::Result<Vec<(u64, CString)>> {
    let mut entries = Vec::new();
    DIRENT_BUFFER.with(|buffer| {
        read_entries(dir.as_fd(), &mut buffer.borrow_mut(), |ino, name| {
            entries.push((ino, name.to_owned()));
        })
    })?;
    entries.sort_unstable_by_key(|(ino, _)| *ino);
    Ok(entries)
}

fn add_entry(
    root_dev: u64,
    hardlinks: &HardlinkSet,
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
        && !hardlinks.is_first_sighting(meta.dev, meta.ino);
    let (logical_size, allocated_size) = if is_duplicate {
        (0, 0)
    } else {
        (meta.size, meta.blocks * 512)
    };
    let name_start = batch.names.len();
    batch.names.push_str(&String::from_utf8_lossy(raw_name));
    // Directories on another device (mount points) stay as empty leaves instead of being entered
    if kind == NodeKind::Directory && meta.dev == root_dev {
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

fn open_dir(path: &Path) -> io::Result<OwnedFd> {
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
fn statx_at(dirfd: i32, name: &CStr, follow_symlinks: bool) -> io::Result<Metadata> {
    let nofollow = if follow_symlinks {
        0
    } else {
        libc::AT_SYMLINK_NOFOLLOW
    };
    let flags = nofollow | libc::AT_STATX_DONT_SYNC;
    let mut buffer = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: `name` is NUL-terminated and `buffer` is a valid out-pointer for one `statx`
    let result =
        unsafe { libc::statx(dirfd, name.as_ptr(), flags, STATX_MASK, buffer.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `statx` returned 0, so it fully initialized the struct
    let raw = unsafe { buffer.assume_init() };
    Ok(Metadata {
        dev: libc::makedev(raw.stx_dev_major, raw.stx_dev_minor),
        ino: raw.stx_ino,
        nlink: raw.stx_nlink,
        mode: u32::from(raw.stx_mode),
        size: raw.stx_size,
        blocks: raw.stx_blocks,
        mtime_secs: raw.stx_mtime.tv_sec,
    })
}

fn statx_path(path: &Path) -> io::Result<Metadata> {
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    statx_at(libc::AT_FDCWD, &c_path, true)
}

/// Calls `visit` with the inode number and name of every entry except `.` and `..`,
/// reading with `getdents64`.
fn read_entries(
    dir: BorrowedFd<'_>,
    buffer: &mut [u8],
    mut visit: impl FnMut(u64, &CStr),
) -> io::Result<()> {
    loop {
        // SAFETY: the kernel writes at most `buffer.len()` bytes into `buffer`
        let filled = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                dir.as_raw_fd(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        if filled < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if filled == 0 {
            return Ok(());
        }
        visit_records(&buffer[..filled as usize], &mut visit);
    }
}

fn visit_records(records: &[u8], visit: &mut impl FnMut(u64, &CStr)) {
    let mut position = 0;
    while position + DIRENT_NAME_OFFSET <= records.len() {
        let reclen_bytes = [
            records[position + DIRENT_RECLEN_OFFSET],
            records[position + DIRENT_RECLEN_OFFSET + 1],
        ];
        let record_len = usize::from(u16::from_ne_bytes(reclen_bytes));
        let end = (position + record_len).min(records.len());
        if record_len == 0 {
            return;
        }
        if let Ok(name) = CStr::from_bytes_until_nul(&records[position + DIRENT_NAME_OFFSET..end]) {
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                visit(read_ino(&records[position..end]), name);
            }
        }
        position += record_len;
    }
}

fn read_ino(record: &[u8]) -> u64 {
    let mut ino_bytes = [0; 8];
    ino_bytes.copy_from_slice(&record[DIRENT_INO_OFFSET..DIRENT_INO_OFFSET + 8]);
    u64::from_ne_bytes(ino_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(ino: u64, name: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; DIRENT_NAME_OFFSET];
        bytes[..8].copy_from_slice(&ino.to_ne_bytes());
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.resize(bytes.len().next_multiple_of(8), 0);
        let record_len = u16::try_from(bytes.len()).unwrap();
        bytes[DIRENT_RECLEN_OFFSET..DIRENT_RECLEN_OFFSET + 2]
            .copy_from_slice(&record_len.to_ne_bytes());
        bytes
    }

    #[test]
    fn visits_inode_and_name_skipping_dot_entries() {
        let records = [record(2, b"."), record(1, b".."), record(42, b"file.txt")].concat();
        let mut visited = Vec::new();
        visit_records(&records, &mut |ino, name: &CStr| {
            visited.push((ino, name.to_bytes().to_vec()));
        });
        assert_eq!(visited, [(42, b"file.txt".to_vec())]);
    }
}
