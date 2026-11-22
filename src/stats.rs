//! Aggregated views over a finished scan: extension breakdown and top-N lists.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::arena::{Arena, NodeId, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionStats {
    pub ext_id: u16,
    pub allocated_size: u64,
    pub file_count: u64,
}

/// Size and file count per extension, largest first. Deleted nodes are skipped.
#[must_use]
pub fn extension_breakdown(arena: &Arena) -> Vec<ExtensionStats> {
    let mut by_extension = vec![(0_u64, 0_u64); arena.extensions.len()];
    for node in &arena.nodes {
        if node.is_deleted || node.kind != NodeKind::File {
            continue;
        }
        let entry = &mut by_extension[usize::from(node.ext_id)];
        entry.0 += node.allocated_size;
        entry.1 += 1;
    }
    let mut stats: Vec<_> = by_extension
        .into_iter()
        .enumerate()
        .filter(|(_, (_, file_count))| *file_count > 0)
        .map(|(index, (allocated_size, file_count))| ExtensionStats {
            ext_id: index as u16,
            allocated_size,
            file_count,
        })
        .collect();
    stats.sort_by_key(|entry| Reverse(entry.allocated_size));
    stats
}

#[must_use]
pub fn top_files(arena: &Arena, limit: usize) -> Vec<NodeId> {
    top_by_size(arena, limit, |kind| kind == NodeKind::File)
}

/// Excludes the root so the list shows what is inside the scan.
#[must_use]
pub fn top_dirs(arena: &Arena, limit: usize) -> Vec<NodeId> {
    // One extra slot because the root, always the largest directory, is dropped afterwards
    let mut ranked = top_by_size(arena, limit + 1, |kind| kind == NodeKind::Directory);
    ranked.retain(|&id| id != crate::arena::ROOT);
    ranked.truncate(limit);
    ranked
}

fn top_by_size(arena: &Arena, limit: usize, keep: impl Fn(NodeKind) -> bool) -> Vec<NodeId> {
    let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(limit + 1);
    for (index, node) in arena.nodes.iter().enumerate() {
        if node.is_deleted || !keep(node.kind) {
            continue;
        }
        heap.push(Reverse((node.allocated_size, index as NodeId)));
        if heap.len() > limit {
            heap.pop();
        }
    }
    let mut ranked: Vec<_> = heap.into_iter().map(|Reverse(entry)| entry).collect();
    ranked.sort_by_key(|&(size, _)| Reverse(size));
    ranked.into_iter().map(|(_, id)| id).collect()
}
