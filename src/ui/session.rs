//! Everything derived from one finished scan, kept together so it is replaced atomically.

use std::time::Duration;

use eframe::egui::Color32;

use super::palette;
use super::tree_model::TreeModel;
use crate::arena::{Arena, Node, ROOT};
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
        }
    }

    pub fn root(&self) -> &Node {
        &self.arena.nodes[ROOT as usize]
    }

    pub fn refresh_after_delete(&mut self) {
        self.tree.rebuild(&self.arena);
        self.ext_stats = stats::extension_breakdown(&self.arena);
    }

    /// Reorders siblings to match the displayed size mode and rebuilds the visible rows.
    pub fn apply_size_mode(&mut self, use_logical: bool) {
        self.arena.sort_all_children(use_logical);
        self.tree.rebuild(&self.arena);
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
