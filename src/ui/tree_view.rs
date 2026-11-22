//! Virtualized directory tree table: only visible rows are laid out each frame.

use eframe::egui::{Align, Color32, Layout, Sense, Ui};
use egui_extras::{Column, TableBuilder, TableRow};

use super::context_menu::{self, RowAction};
use super::tree_cells::{name_cell, percent_of_parent, right_aligned, size_cell};
use super::tree_model::{Row, TreeModel};
use crate::arena::{Arena, NodeId, NodeKind};
use crate::format::format_mtime;

const ROW_HEIGHT: f32 = 20.0;

pub struct TreeView<'a> {
    pub arena: &'a Arena,
    pub colors: &'a [Color32],
    pub use_logical: bool,
    pub highlighted_ext: Option<u16>,
}

/// Interactions collected while painting, applied afterwards because painting borrows the model.
#[derive(Default)]
pub(super) struct Pending {
    pub(super) toggle: Option<NodeId>,
    pub(super) select: Option<NodeId>,
    pub(super) action: Option<RowAction>,
}

pub fn show(ui: &mut Ui, model: &mut TreeModel, view: &TreeView<'_>) -> Option<RowAction> {
    let mut pending = Pending::default();
    let scroll_row = model.scroll_to.take().and_then(|id| model.row_index(id));
    let (rows, expanded, selected) = (&model.rows, &model.expanded, model.selected);
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        // Cells default to a vertical layout, which pushes indented names out of the row
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::initial(210.0).at_least(120.0).clip(true))
        .column(Column::initial(130.0).at_least(90.0))
        .column(Column::initial(80.0))
        .column(Column::initial(70.0))
        .column(Column::initial(70.0))
        .column(Column::initial(70.0))
        .column(Column::remainder().at_least(110.0));
    if let Some(row) = scroll_row {
        table = table.scroll_to_row(row, Some(Align::Center));
    }
    table
        .header(ROW_HEIGHT, |mut header| {
            for title in [
                "Name",
                "Size",
                "% of Parent",
                "Items",
                "Files",
                "Folders",
                "Last Modified",
            ] {
                header.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, rows.len(), |mut table_row| {
                let entry = rows[table_row.index()];
                let state = RowState {
                    is_expanded: expanded.contains(&entry.id),
                    is_selected: selected == Some(entry.id),
                };
                paint_row(&mut table_row, view, entry, state, &mut pending);
            });
        });
    if let Some(id) = pending.select {
        model.selected = Some(id);
    }
    if let Some(id) = pending.toggle {
        model.toggle(view.arena, id);
    }
    pending.action
}

#[derive(Clone, Copy)]
pub(super) struct RowState {
    pub(super) is_expanded: bool,
    pub(super) is_selected: bool,
}

fn paint_row(
    row: &mut TableRow<'_, '_>,
    view: &TreeView<'_>,
    entry: Row,
    state: RowState,
    pending: &mut Pending,
) {
    let node = &view.arena.nodes[entry.id as usize];
    let is_directory = node.kind == NodeKind::Directory;
    row.set_selected(state.is_selected);
    row.col(|ui| name_cell(ui, view, entry, state, pending));
    row.col(|ui| size_cell(ui, view, node));
    row.col(|ui| right_aligned(ui, &percent_of_parent(view, node)));
    for count in [
        node.file_count + node.dir_count,
        node.file_count,
        node.dir_count,
    ] {
        let text = if is_directory {
            count.to_string()
        } else {
            String::new()
        };
        row.col(|ui| right_aligned(ui, &text));
    }
    row.col(|ui| {
        ui.label(format_mtime(node.mtime_secs));
    });
    handle_interaction(row, entry.id, is_directory, pending);
}

fn handle_interaction(
    row: &TableRow<'_, '_>,
    id: NodeId,
    is_directory: bool,
    pending: &mut Pending,
) {
    let response = row.response();
    if response.clicked() || response.secondary_clicked() {
        pending.select = Some(id);
    }
    if response.double_clicked() && is_directory {
        pending.toggle = Some(id);
    }
    response.context_menu(|ui| pending.action = context_menu::show(ui, id).or(pending.action));
}
