//! Virtualized directory tree table: only visible rows are laid out each frame.

use eframe::egui::{self, Align, Color32, Label, Layout, Rect, Sense, Ui, Vec2};
use egui_extras::{Column, TableBuilder, TableRow};

use super::tree_model::{Row, TreeModel};
use crate::arena::{Arena, NO_NODE, Node, NodeId, NodeKind};
use crate::format::{format_mtime, format_size};

const ROW_HEIGHT: f32 = 20.0;
const INDENT_PER_LEVEL: f32 = 16.0;
const ARROW_WIDTH: f32 = 14.0;
const FOLDER_COLOR: Color32 = Color32::from_rgb(120, 144, 190);
const BAR_COLOR: Color32 = Color32::from_rgba_premultiplied(40, 70, 112, 63);
const HIGHLIGHT_COLOR: Color32 = Color32::from_rgba_premultiplied(120, 90, 0, 120);

pub struct TreeView<'a> {
    pub arena: &'a Arena,
    pub colors: &'a [Color32],
    pub use_logical: bool,
    pub highlighted_ext: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    OpenInFileManager(NodeId),
    CopyPath(NodeId),
    MoveToTrash(NodeId),
    DeletePermanently(NodeId),
}

/// Interactions collected while painting, applied afterwards because painting borrows the model.
#[derive(Default)]
struct Pending {
    toggle: Option<NodeId>,
    select: Option<NodeId>,
    action: Option<RowAction>,
}

pub fn show(ui: &mut Ui, model: &mut TreeModel, view: &TreeView<'_>) -> Option<RowAction> {
    let mut pending = Pending::default();
    let (rows, expanded, selected) = (&model.rows, &model.expanded, model.selected);
    TableBuilder::new(ui)
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
        .column(Column::remainder().at_least(110.0))
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
struct RowState {
    is_expanded: bool,
    is_selected: bool,
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
    response.context_menu(|ui| pending.action = row_menu(ui, id).or(pending.action));
}

fn row_menu(ui: &mut Ui, id: NodeId) -> Option<RowAction> {
    let is_root = id == crate::arena::ROOT;
    let mut chosen = None;
    if ui.button("Open in File Manager").clicked() {
        chosen = Some(RowAction::OpenInFileManager(id));
    }
    if ui.button("Copy Full Path").clicked() {
        chosen = Some(RowAction::CopyPath(id));
    }
    if ui
        .add_enabled(!is_root, egui::Button::new("Move to Trash"))
        .clicked()
    {
        chosen = Some(RowAction::MoveToTrash(id));
    }
    if ui
        .add_enabled(!is_root, egui::Button::new("Delete Permanently"))
        .clicked()
    {
        chosen = Some(RowAction::DeletePermanently(id));
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

fn name_cell(ui: &mut Ui, view: &TreeView<'_>, entry: Row, state: RowState, pending: &mut Pending) {
    let node = &view.arena.nodes[entry.id as usize];
    highlight_if_matching(ui, view, node);
    ui.add_space(entry.depth as f32 * INDENT_PER_LEVEL);
    if node.kind == NodeKind::Directory && node.first_child != NO_NODE {
        // Painted instead of a text glyph: the default egui font has no down-pointing triangle
        let (_, response) = ui.allocate_exact_size(Vec2::splat(ARROW_WIDTH), Sense::click());
        let openness = if state.is_expanded { 1.0 } else { 0.0 };
        egui::collapsing_header::paint_default_icon(ui, openness, &response);
        if response.clicked() {
            pending.toggle = Some(entry.id);
        }
    } else {
        ui.add_space(ARROW_WIDTH);
    }
    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter()
        .circle_filled(dot.center(), 4.0, node_color(view, node));
    ui.add(Label::new(view.arena.name(entry.id)).selectable(false));
}

fn node_color(view: &TreeView<'_>, node: &Node) -> Color32 {
    if node.kind == NodeKind::Directory {
        return FOLDER_COLOR;
    }
    view.colors
        .get(usize::from(node.ext_id))
        .copied()
        .unwrap_or(Color32::GRAY)
}

fn highlight_if_matching(ui: &Ui, view: &TreeView<'_>, node: &Node) {
    let is_match = node.kind == NodeKind::File && view.highlighted_ext == Some(node.ext_id);
    if is_match {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, HIGHLIGHT_COLOR);
    }
}

fn size_cell(ui: &mut Ui, view: &TreeView<'_>, node: &Node) {
    let size = node.size(view.use_logical);
    let cell = ui.max_rect();
    let bar_width = cell.width() * parent_fraction(view, node, size);
    let bar = Rect::from_min_size(cell.min, Vec2::new(bar_width, cell.height()));
    ui.painter().rect_filled(bar, 2.0, BAR_COLOR);
    right_aligned(ui, &format_size(size));
}

fn parent_fraction(view: &TreeView<'_>, node: &Node, size: u64) -> f32 {
    let parent_size = if node.parent == NO_NODE {
        size
    } else {
        view.arena.nodes[node.parent as usize].size(view.use_logical)
    };
    if parent_size == 0 {
        0.0
    } else {
        (size as f64 / parent_size as f64) as f32
    }
}

fn percent_of_parent(view: &TreeView<'_>, node: &Node) -> String {
    let fraction = parent_fraction(view, node, node.size(view.use_logical));
    format!("{:.1} %", fraction * 100.0)
}

fn right_aligned(ui: &mut Ui, text: &str) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(text);
    });
}
