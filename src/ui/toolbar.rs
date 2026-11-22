//! Top bar: drive selector, path input, scan control and size mode.

use eframe::egui::{self, Key, Ui};

use crate::format::{format_count, format_size};
use crate::mounts::MountInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarEvent {
    StartScan,
    Stop,
}

/// Widget id of the search box, so `Ctrl+F` can focus it from outside the toolbar.
pub const FILTER_FIELD_ID: &str = "filter_input";

pub struct ToolbarState<'a> {
    pub path_input: &'a mut String,
    pub filter_input: &'a mut String,
    pub match_count: Option<usize>,
    pub mounts: &'a [MountInfo],
    pub use_logical: &'a mut bool,
    pub is_scanning: bool,
}

pub fn show(ui: &mut Ui, state: &mut ToolbarState<'_>) -> Option<ToolbarEvent> {
    let mut event = None;
    ui.horizontal(|ui| {
        if drive_picker(ui, state.mounts, state.path_input) {
            event = Some(ToolbarEvent::StartScan);
        }
        let path_field = ui.add(
            egui::TextEdit::singleline(state.path_input)
                .hint_text("Path to scan")
                .desired_width(280.0),
        );
        let submitted = path_field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if state.is_scanning {
            if ui.button("Stop").clicked() {
                event = Some(ToolbarEvent::Stop);
            }
        } else if ui.button("Scan").clicked() || submitted {
            event = Some(ToolbarEvent::StartScan);
        }
        ui.separator();
        filter_field(ui, state.filter_input, state.match_count);
        ui.separator();
        ui.selectable_value(state.use_logical, false, "Allocated size");
        ui.selectable_value(state.use_logical, true, "Logical size");
    });
    event
}

fn filter_field(ui: &mut Ui, filter_input: &mut String, match_count: Option<usize>) {
    let field = ui.add(
        egui::TextEdit::singleline(filter_input)
            .id(egui::Id::new(FILTER_FIELD_ID))
            .hint_text("Filter: name or *.ext (Ctrl+F)")
            .desired_width(190.0),
    );
    let escape_pressed = ui.input(|input| input.key_pressed(Key::Escape));
    if (field.has_focus() || field.lost_focus()) && escape_pressed {
        filter_input.clear();
        field.surrender_focus();
    }
    if let Some(count) = match_count {
        ui.label(format!("{} matches", format_count(count as u64)));
    }
}

/// Returns `true` when the user picked a mount, whose path is written to `path_input`.
fn drive_picker(ui: &mut Ui, mounts: &[MountInfo], path_input: &mut String) -> bool {
    let mut picked = false;
    egui::ComboBox::from_id_salt("drive_picker")
        .selected_text("Select drive")
        .width(360.0)
        .show_ui(ui, |ui| {
            for mount in mounts {
                if ui.selectable_label(false, mount_label(mount)).clicked() {
                    path_input.clone_from(&mount.mount_point);
                    picked = true;
                }
            }
        });
    picked
}

fn mount_label(mount: &MountInfo) -> String {
    let used = mount.total_bytes.saturating_sub(mount.free_bytes);
    format!(
        "{} - {} - {} / {}",
        mount.mount_point,
        mount.device,
        format_size(used),
        format_size(mount.total_bytes)
    )
}
