//! Flat node arena: one `Vec<Node>` plus one contiguous name pool.

use crate::extensions::ExtensionTable;

pub type NodeId = u32;
pub const NO_NODE: NodeId = u32::MAX;
pub const ROOT: NodeId = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    Directory = 0,
    File = 1,
    Symlink = 2,
    Other = 3,
}

/// 56 bytes with alignment (the SPEC comment says 48, but its own field list does not fit).
#[derive(Debug, Clone)]
pub struct Node {
    pub parent: NodeId,
    pub first_child: NodeId,
    pub next_sibling: NodeId,
    pub name_offset: u32,
    pub name_len: u16,
    pub ext_id: u16,
    pub kind: NodeKind,
    pub is_deleted: bool,
    /// Counted once per inode: later hard links keep a node but contribute no size.
    pub is_hardlink_duplicate: bool,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub file_count: u32,
    pub dir_count: u32,
    pub mtime_secs: i64,
}

/// Append-only UTF-8 names. Offsets are `u32`, so the pool is capped at 4 GiB.
#[derive(Debug, Default)]
pub struct StringPool {
    bytes: Vec<u8>,
}

impl StringPool {
    fn push(&mut self, name: &str) -> (u32, u16) {
        let offset = self.bytes.len() as u32;
        let len = name.len().min(usize::from(u16::MAX));
        self.bytes.extend_from_slice(&name.as_bytes()[..len]);
        (offset, len as u16)
    }

    fn get(&self, offset: u32, len: u16) -> &str {
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

#[derive(Debug, Default)]
pub struct Arena {
    pub nodes: Vec<Node>,
    pub pool: StringPool,
    pub extensions: ExtensionTable,
}

impl Arena {
    /// Creates an arena whose node 0 is the scan root, named after its full path.
    #[must_use]
    pub fn with_root(path: &str, logical_size: u64, allocated_size: u64, mtime_secs: i64) -> Self {
        let mut arena = Self::default();
        let (name_offset, name_len) = arena.pool.push(path);
        arena.nodes.push(Node {
            parent: NO_NODE,
            first_child: NO_NODE,
            next_sibling: NO_NODE,
            name_offset,
            name_len,
            ext_id: 0,
            kind: NodeKind::Directory,
            is_deleted: false,
            is_hardlink_duplicate: false,
            logical_size,
            allocated_size,
            file_count: 0,
            dir_count: 0,
            mtime_secs,
        });
        arena
    }

    /// Inserts `batch` as the children of `parent` and returns the first new id.
    /// Ids are contiguous, so the child at batch index `i` has id `first + i`.
    pub fn append_children(&mut self, parent: NodeId, batch: &ChildBatch) -> NodeId {
        let first = self.nodes.len() as NodeId;
        let count = batch.entries.len();
        if count == 0 {
            return NO_NODE;
        }
        self.nodes.reserve(count);
        for (index, entry) in batch.entries.iter().enumerate() {
            let name = &batch.names[entry.name_start..entry.name_end];
            let (name_offset, name_len) = self.pool.push(name);
            let counts_as_file = matches!(entry.kind, NodeKind::File | NodeKind::Symlink);
            let ext_id = if counts_as_file {
                self.extensions.intern(name)
            } else {
                0
            };
            self.nodes.push(Node {
                parent,
                first_child: NO_NODE,
                next_sibling: if index + 1 < count {
                    first + index as NodeId + 1
                } else {
                    NO_NODE
                },
                name_offset,
                name_len,
                ext_id,
                kind: entry.kind,
                is_deleted: false,
                is_hardlink_duplicate: entry.is_hardlink_duplicate,
                logical_size: entry.logical_size,
                allocated_size: entry.allocated_size,
                file_count: u32::from(counts_as_file),
                dir_count: 0,
                mtime_secs: entry.mtime_secs,
            });
        }
        self.nodes[parent as usize].first_child = first;
        first
    }

    #[must_use]
    pub fn name(&self, id: NodeId) -> &str {
        let node = &self.nodes[id as usize];
        self.pool.get(node.name_offset, node.name_len)
    }

    #[must_use]
    pub fn path_of(&self, id: NodeId) -> String {
        let mut components = Vec::new();
        let mut current = id;
        while current != NO_NODE {
            components.push(self.name(current));
            current = self.nodes[current as usize].parent;
        }
        let mut path = String::new();
        for component in components.into_iter().rev() {
            if !path.is_empty() && !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(component);
        }
        path
    }

    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut next = self.nodes[id as usize].first_child;
        std::iter::from_fn(move || {
            if next == NO_NODE {
                return None;
            }
            let current = next;
            next = self.nodes[current as usize].next_sibling;
            Some(current)
        })
    }

    /// Rolls sizes and counters up to the root, then sorts every sibling chain by size.
    pub fn aggregate(&mut self) {
        // Children always have larger ids than their parent, so one reverse pass is enough
        for id in (1..self.nodes.len()).rev() {
            let child = self.nodes[id].clone();
            let parent = &mut self.nodes[child.parent as usize];
            parent.logical_size += child.logical_size;
            parent.allocated_size += child.allocated_size;
            parent.file_count += child.file_count;
            parent.dir_count += child.dir_count + u32::from(child.kind == NodeKind::Directory);
            parent.mtime_secs = parent.mtime_secs.max(child.mtime_secs);
        }
        self.sort_all_children();
    }

    fn sort_all_children(&mut self) {
        let mut scratch: Vec<NodeId> = Vec::new();
        for id in 0..self.nodes.len() as NodeId {
            scratch.clear();
            scratch.extend(self.children(id));
            if scratch.len() < 2 {
                continue;
            }
            scratch
                .sort_by_key(|&child| std::cmp::Reverse(self.nodes[child as usize].allocated_size));
            self.nodes[id as usize].first_child = scratch[0];
            for pair in scratch.windows(2) {
                self.nodes[pair[0] as usize].next_sibling = pair[1];
            }
            if let Some(&last) = scratch.last() {
                self.nodes[last as usize].next_sibling = NO_NODE;
            }
        }
    }

    /// Tombstones the subtree and subtracts its totals from every ancestor in O(depth).
    pub fn mark_deleted(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id as usize) else {
            return;
        };
        if node.is_deleted {
            return;
        }
        let removed_dirs = node.dir_count + u32::from(node.kind == NodeKind::Directory);
        let (logical, allocated, files) = (node.logical_size, node.allocated_size, node.file_count);
        let mut ancestor = node.parent;
        self.tombstone_subtree(id);
        while ancestor != NO_NODE {
            let node = &mut self.nodes[ancestor as usize];
            node.logical_size = node.logical_size.saturating_sub(logical);
            node.allocated_size = node.allocated_size.saturating_sub(allocated);
            node.file_count = node.file_count.saturating_sub(files);
            node.dir_count = node.dir_count.saturating_sub(removed_dirs);
            ancestor = node.parent;
        }
    }

    fn tombstone_subtree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            self.nodes[current as usize].is_deleted = true;
            stack.extend(self.children(current));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_stays_within_the_size_budget() {
        assert!(std::mem::size_of::<Node>() <= 56);
    }
}
