//! Flattened, expandable view of the arena. Pure data, so it can be tested without a display.

use std::collections::HashSet;

use crate::arena::{Arena, NodeId, NodeKind, ROOT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: NodeId,
    pub depth: u32,
}

#[derive(Debug, Default)]
pub struct TreeModel {
    pub expanded: HashSet<NodeId>,
    pub rows: Vec<Row>,
    pub selected: Option<NodeId>,
}

impl TreeModel {
    /// Starts from a fresh scan: only the root is expanded.
    pub fn reset(&mut self, arena: &Arena) {
        self.expanded.clear();
        self.expanded.insert(ROOT);
        self.selected = None;
        self.rebuild(arena);
    }

    /// Rebuilds the visible rows depth-first. Call after expansion or deletion changes.
    pub fn rebuild(&mut self, arena: &Arena) {
        self.rows.clear();
        let mut stack = vec![Row { id: ROOT, depth: 0 }];
        while let Some(row) = stack.pop() {
            self.rows.push(row);
            if self.expanded.contains(&row.id) {
                let children: Vec<NodeId> = arena
                    .children(row.id)
                    .filter(|&child| !arena.nodes[child as usize].is_deleted)
                    .collect();
                // Pushed in reverse so the largest child is popped first
                stack.extend(children.into_iter().rev().map(|id| Row {
                    id,
                    depth: row.depth + 1,
                }));
            }
        }
        if self
            .selected
            .is_some_and(|id| arena.nodes[id as usize].is_deleted)
        {
            self.selected = None;
        }
    }

    pub fn toggle(&mut self, arena: &Arena, id: NodeId) {
        if arena.nodes[id as usize].kind != NodeKind::Directory {
            return;
        }
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        self.rebuild(arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{ChildBatch, NewNode};

    fn add(arena: &mut Arena, parent: NodeId, entries: &[(&str, NodeKind, u64)]) -> NodeId {
        let mut batch = ChildBatch::default();
        for &(name, kind, size) in entries {
            let name_start = batch.names.len();
            batch.names.push_str(name);
            batch.entries.push(NewNode {
                name_start,
                name_end: batch.names.len(),
                kind,
                logical_size: size,
                allocated_size: size,
                mtime_secs: 0,
                is_hardlink_duplicate: false,
            });
        }
        arena.append_children(parent, &batch)
    }

    /// root / { docs/ { a, b }, big }
    fn sample() -> (Arena, NodeId) {
        let mut arena = Arena::with_root("/r", 0, 0, 0);
        let first = add(
            &mut arena,
            ROOT,
            &[
                ("docs", NodeKind::Directory, 0),
                ("big", NodeKind::File, 900),
            ],
        );
        add(
            &mut arena,
            first,
            &[("a", NodeKind::File, 5), ("b", NodeKind::File, 7)],
        );
        arena.aggregate();
        (arena, first)
    }

    fn names(model: &TreeModel, arena: &Arena) -> Vec<String> {
        model
            .rows
            .iter()
            .map(|row| arena.name(row.id).to_owned())
            .collect()
    }

    #[test]
    fn reset_shows_only_the_roots_children() {
        let (arena, _) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);
        assert_eq!(names(&model, &arena), ["/r", "big", "docs"]);
    }

    #[test]
    fn toggle_expands_and_collapses_in_order() {
        let (arena, docs) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);
        model.toggle(&arena, docs);
        assert_eq!(names(&model, &arena), ["/r", "big", "docs", "b", "a"]);
        assert_eq!(model.rows[3].depth, 2);
        model.toggle(&arena, docs);
        assert_eq!(model.rows.len(), 3);
    }

    #[test]
    fn toggling_a_file_does_nothing() {
        let (arena, _) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);
        model.toggle(&arena, 2);
        assert_eq!(model.rows.len(), 3);
    }

    #[test]
    fn deleted_nodes_disappear_and_clear_the_selection() {
        let (mut arena, _) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);
        model.selected = Some(2);
        arena.mark_deleted(2);
        model.rebuild(&arena);
        assert_eq!(names(&model, &arena), ["/r", "docs"]);
        assert_eq!(model.selected, None);
    }
}
