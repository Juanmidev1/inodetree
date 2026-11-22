//! File extension summary table with a color legend.

use eframe::egui::{self, Color32, Layout, Sense, Ui, Vec2};
use egui_extras::{Column, TableBuilder};

use super::tree_cells::right_aligned;
use crate::extensions::ExtensionTable;
use crate::format::format_size;
use crate::stats::ExtensionStats;

const ROW_HEIGHT: f32 = 20.0;

pub struct ExtView<'a> {
    pub stats: &'a [ExtensionStats],
    pub extensions: &'a ExtensionTable,
    pub colors: &'a [Color32],
    pub scan_total_bytes: u64,
    pub selected: Option<u16>,
}

/// Returns the extension the user clicked this frame, if any.
pub fn show(ui: &mut Ui, view: &ExtView<'_>) -> Option<u16> {
    let mut clicked = None;
    // Selectable labels swallow the click, so pressing on an extension name would do nothing
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .striped(true)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .column(Column::remainder().at_least(60.0).clip(true))
        .column(Column::exact(40.0))
        .column(Column::initial(80.0))
        .column(Column::initial(70.0))
        .column(Column::initial(70.0))
        .header(ROW_HEIGHT, |mut header| {
            for title in ["Extension", "Color", "Size", "% of Scan", "Files"] {
                header.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, view.stats.len(), |mut row| {
                let entry = view.stats[row.index()];
                row.set_selected(view.selected == Some(entry.ext_id));
                paint_cells(&mut row, view, entry);
                if row.response().clicked() {
                    clicked = Some(entry.ext_id);
                }
            });
        });
    clicked
}

fn paint_cells(row: &mut egui_extras::TableRow<'_, '_>, view: &ExtView<'_>, entry: ExtensionStats) {
    let name = view.extensions.name(entry.ext_id);
    let color = view
        .colors
        .get(usize::from(entry.ext_id))
        .copied()
        .unwrap_or(Color32::GRAY);
    let percent = if view.scan_total_bytes == 0 {
        0.0
    } else {
        entry.allocated_size as f64 * 100.0 / view.scan_total_bytes as f64
    };
    row.col(|ui| {
        ui.label(if name.is_empty() { "(none)" } else { name });
    });
    row.col(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(24.0, 10.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
    });
    row.col(|ui| right_aligned(ui, &format_size(entry.allocated_size)));
    row.col(|ui| right_aligned(ui, &format!("{percent:.1} %")));
    row.col(|ui| right_aligned(ui, &entry.file_count.to_string()));
}
