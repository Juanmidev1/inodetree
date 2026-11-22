//! Pieces used while building an arena: the name pool and per-directory insertion batches.

use super::NodeKind;

/// Append-only UTF-8 names. Offsets are `u32`, so the pool is capped at 4 GiB.
#[derive(Debug, Default)]
pub struct StringPool {
    bytes: Vec<u8>,
}

impl StringPool {
    pub(super) fn push(&mut self, name: &str) -> (u32, u16) {
        let offset = self.bytes.len() as u32;
        let len = name.len().min(usize::from(u16::MAX));
        self.bytes.extend_from_slice(&name.as_bytes()[..len]);
        (offset, len as u16)
    }

    pub(super) fn get(&self, offset: u32, len: u16) -> &str {
        let start = offset as usize;
        let slice = self
            .bytes
            .get(start..start + usize::from(len))
            .unwrap_or_default();
        std::str::from_utf8(slice).unwrap_or_default()
    }
}

/// Entry waiting to be inserted. `name_start..name_end` indexes `ChildBatch::names`.
#[derive(Debug)]
pub struct NewNode {
    pub name_start: usize,
    pub name_end: usize,
    pub kind: NodeKind,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub mtime_secs: i64,
    pub is_hardlink_duplicate: bool,
}

/// All children of one directory, collected without touching the shared arena.
#[derive(Debug, Default)]
pub struct ChildBatch {
    pub names: String,
    pub entries: Vec<NewNode>,
}
