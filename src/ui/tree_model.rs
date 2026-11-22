//! Flattened, expandable view of the arena. Pure data, so it can be tested without a display.

use std::collections::HashSet;

use crate::arena::{Arena, NO_NODE, NodeId, NodeKind, ROOT};
use crate::filter::FilterMask;

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
    /// Row to bring into view on the next frame, then cleared by the table
    pub scroll_to: Option<NodeId>,
    /// Active search result: rows are limited to matches and the folders leading to them
    pub filter: Option<FilterMask>,
    // Expansion the user had before searching, restored when the search is cleared
    saved_expanded: Option<HashSet<NodeId>>,
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
                    .filter(|&child| {
                        !arena.nodes[child as usize].is_deleted
                            && self
                                .filter
                                .as_ref()
                                .is_none_or(|mask| mask.is_visible(child))
                    })
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

    /// Applies or clears a search result. While a search is active every folder that leads to a
    /// match is open, so results are visible without clicking through the tree.
    pub fn set_filter(&mut self, arena: &Arena, mask: Option<FilterMask>) {
        match (&self.filter, &mask) {
            (None, Some(_)) => self.saved_expanded = Some(std::mem::take(&mut self.expanded)),
            (Some(_), None) => {
                if let Some(saved) = self.saved_expanded.take() {
                    self.expanded = saved;
                }
            }
            _ => {}
        }
        if let Some(mask) = &mask {
            self.expanded = (0..arena.nodes.len() as NodeId)
                .filter(|&id| mask.has_match_below(id))
                .collect();
            self.expanded.insert(ROOT);
        }
        self.filter = mask;
        self.rebuild(arena);
    }

    /// Selects `id`, expands its ancestors and asks the table to scroll to it.
    pub fn select_and_reveal(&mut self, arena: &Arena, id: NodeId) {
        let mut ancestor = arena.nodes[id as usize].parent;
        while ancestor != NO_NODE {
            self.expanded.insert(ancestor);
            ancestor = arena.nodes[ancestor as usize].parent;
        }
        self.selected = Some(id);
        self.scroll_to = Some(id);
        self.rebuild(arena);
    }

    #[must_use]
    pub fn row_index(&self, id: NodeId) -> Option<usize> {
        self.rows.iter().position(|row| row.id == id)
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

    #[test]
    fn select_and_reveal_expands_ancestors_and_targets_the_row() {
        let (arena, docs) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);
        model.select_and_reveal(&arena, docs + 3);
        assert!(model.expanded.contains(&docs));
        assert_eq!(model.selected, Some(docs + 3));
        assert_eq!(model.scroll_to, Some(docs + 3));
        assert_eq!(model.row_index(docs + 3), Some(3));
        assert_eq!(model.row_index(999), None);
    }

    #[test]
    fn a_filter_limits_rows_opens_matches_and_restores_expansion_when_cleared() {
        use crate::filter::NameFilter;
        let (arena, docs) = sample();
        let mut model = TreeModel::default();
        model.reset(&arena);

        let mask = FilterMask::build(&arena, &NameFilter::parse("a").unwrap());
        model.set_filter(&arena, Some(mask));
        assert_eq!(names(&model, &arena), ["/r", "docs", "a"]);
        assert!(model.expanded.contains(&docs));

        model.set_filter(&arena, None);
        assert_eq!(names(&model, &arena), ["/r", "big", "docs"]);
        assert!(!model.expanded.contains(&docs));
    }
}
