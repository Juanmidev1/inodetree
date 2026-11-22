//! Top bar: drive selector, path input, scan control and size mode.

use eframe::egui::{self, Key, Ui};

use crate::format::format_size;
use crate::mounts::MountInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarEvent {
    StartScan,
    Stop,
}

pub struct ToolbarState<'a> {
    pub path_input: &'a mut String,
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
        ui.selectable_value(state.use_logical, false, "Allocated size");
        ui.selectable_value(state.use_logical, true, "Logical size");
    });
    event
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
