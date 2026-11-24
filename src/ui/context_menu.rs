//! Right-click menu shared by the tree table and the treemap.

use eframe::egui::{self, Ui};

use crate::arena::{NodeId, ROOT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    OpenInFileManager(NodeId),
    CopyPath(NodeId),
    MoveToTrash(NodeId),
    DeletePermanently(NodeId),
}

pub fn show(ui: &mut Ui, id: NodeId) -> Option<RowAction> {
    let is_root = id == ROOT;
    let mut chosen = None;
    let open_label = if cfg!(windows) {
        "Open in Explorer"
    } else {
        "Open in File Manager"
    };
    let trash_label = if cfg!(windows) {
        "Move to Recycle Bin"
    } else {
        "Move to Trash"
    };
    if ui.button(open_label).clicked() {
        chosen = Some(RowAction::OpenInFileManager(id));
    }
    if ui.button("Copy Full Path").clicked() {
        chosen = Some(RowAction::CopyPath(id));
    }
    if ui
        .add_enabled(!is_root, egui::Button::new(trash_label))
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
