//! Everything derived from one finished scan, kept together so it is replaced atomically.

use std::time::Duration;

use eframe::egui::Color32;

use super::palette;
use super::tree_model::TreeModel;
use super::treemap::TreemapState;
use crate::arena::{Arena, Node, ROOT};
use crate::filter::{FilterMask, NameFilter};
use crate::scan::ScanResult;
use crate::stats::{self, ExtensionStats};

pub struct Session {
    pub arena: Arena,
    pub tree: TreeModel,
    pub ext_stats: Vec<ExtensionStats>,
    pub colors: Vec<Color32>,
    pub selected_ext: Option<u16>,
    pub elapsed: Duration,
    pub unreadable_dirs: u64,
    pub treemap: TreemapState,
    /// Bumped on every change to the arena after the scan, so cached layouts know to rebuild
    pub generation: u64,
    /// Text of the search box the tree and treemap currently reflect
    pub applied_filter: String,
}

impl Session {
    pub fn new(result: ScanResult, unreadable_dirs: u64) -> Self {
        let mut tree = TreeModel::default();
        tree.reset(&result.arena);
        Self {
            ext_stats: stats::extension_breakdown(&result.arena),
            colors: palette::color_table(&result.arena.extensions),
            arena: result.arena,
            tree,
            selected_ext: None,
            elapsed: result.elapsed,
            unreadable_dirs,
            treemap: TreemapState::default(),
            generation: 0,
            applied_filter: String::new(),
        }
    }

    pub fn root(&self) -> &Node {
        &self.arena.nodes[ROOT as usize]
    }

    pub fn refresh_after_delete(&mut self) {
        // The mask was built before the deletion, so an active search is evaluated again
        let search = std::mem::take(&mut self.applied_filter);
        if search.is_empty() {
            self.tree.rebuild(&self.arena);
        } else {
            self.apply_filter(&search);
        }
        self.ext_stats = stats::extension_breakdown(&self.arena);
        self.generation += 1;
    }

    /// Reorders siblings to match the displayed size mode and rebuilds the visible rows.
    pub fn apply_size_mode(&mut self, use_logical: bool) {
        self.arena.sort_all_children(use_logical);
        self.tree.rebuild(&self.arena);
        self.generation += 1;
    }

    /// Re-evaluates the search box text against the whole arena (a single linear pass).
    pub fn apply_filter(&mut self, text: &str) {
        text.clone_into(&mut self.applied_filter);
        let mask = NameFilter::parse(text).map(|filter| FilterMask::build(&self.arena, &filter));
        self.tree.set_filter(&self.arena, mask);
        self.generation += 1;
    }

    pub fn match_count(&self) -> Option<usize> {
        self.tree.filter.as_ref().map(|mask| mask.match_count)
    }

    /// Clicking the selected extension again clears the highlight.
    pub fn toggle_extension(&mut self, ext_id: u16) {
        self.selected_ext = if self.selected_ext == Some(ext_id) {
            None
        } else {
            Some(ext_id)
        };
    }
}
