//! Clickable path from the scan root to the zoomed directory (`root > home > user`).

use eframe::egui::Ui;

use crate::arena::{Arena, NodeId};

/// Returns the ancestor the user clicked, if any.
pub fn show(ui: &mut Ui, arena: &Arena, zoom: NodeId) -> Option<NodeId> {
    let chain = arena.ancestry(zoom);
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for (index, &id) in chain.iter().enumerate() {
            if index > 0 {
                ui.label(">");
            }
            if index + 1 == chain.len() {
                ui.strong(arena.name(id));
            } else if ui.link(arena.name(id)).clicked() {
                clicked = Some(id);
            }
        }
    });
    clicked
}
