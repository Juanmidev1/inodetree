//! Whole-arena mutations: bottom-up aggregation, sibling ordering and deletion.

use super::{Arena, NO_NODE, NodeId, NodeKind};

impl Arena {
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
        self.sort_all_children(false);
    }

    /// Orders every sibling chain by descending size in the chosen mode.
    pub fn sort_all_children(&mut self, use_logical: bool) {
        let mut scratch: Vec<NodeId> = Vec::new();
        for id in 0..self.nodes.len() as NodeId {
            scratch.clear();
            scratch.extend(self.children(id));
            if scratch.len() < 2 {
                continue;
            }
            scratch.sort_by_key(|&child| {
                std::cmp::Reverse(self.nodes[child as usize].size(use_logical))
            });
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
