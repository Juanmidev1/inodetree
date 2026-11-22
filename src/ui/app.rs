//! `eframe::App` state machine: background scan, deletions and panel layout.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, Event, Key, Modifiers};

use super::ext_view::{self, ExtView};
use super::session::Session;
use super::toolbar::{self, ToolbarEvent, ToolbarState};
use super::tree_view::{self, RowAction, TreeView};
use crate::actions::{self, ActionResult, DeleteMode};
use crate::arena::NodeId;
use crate::format::{format_count, format_size};
use crate::mounts::{self, MountInfo};
use crate::scan::ScanConfig;
use crate::scan::job::ScanJob;

const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(100);
const WARNING_COLOR: Color32 = Color32::from_rgb(230, 170, 40);

pub struct App {
    context: egui::Context,
    path_input: String,
    mounts: Vec<MountInfo>,
    mounts_receiver: Receiver<Vec<MountInfo>>,
    job: Option<ScanJob>,
    session: Option<Session>,
    use_logical: bool,
    error: Option<String>,
    confirming_delete: Option<NodeId>,
    // Ids of deletions in flight, so a rescan in between cannot apply them to a new arena
    pending_deletes: HashSet<NodeId>,
    action_sender: Sender<ActionResult>,
    action_receiver: Receiver<ActionResult>,
}

impl App {
    pub fn new(context: &egui::Context, initial_path: Option<&Path>) -> Self {
        let (mounts_sender, mounts_receiver) = mpsc::channel();
        let (action_sender, action_receiver) = mpsc::channel();
        let mut app = Self {
            context: context.clone(),
            path_input: initial_path
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            mounts: Vec::new(),
            mounts_receiver,
            job: None,
            session: None,
            use_logical: false,
            error: None,
            confirming_delete: None,
            pending_deletes: HashSet::new(),
            action_sender,
            action_receiver,
        };
        app.load_mounts(mounts_sender);
        if initial_path.is_some() {
            app.start_scan();
        }
        app
    }

    fn load_mounts(&mut self, sender: Sender<Vec<MountInfo>>) {
        let repaint = self.context.clone();
        let spawned = thread::Builder::new()
            .name("inodetree-mounts".into())
            .spawn(move || {
                // An unreadable mountinfo only leaves the drive picker empty
                let _ = sender.send(mounts::list_mounts().unwrap_or_default());
                repaint.request_repaint();
            });
        if let Err(error) = spawned {
            self.error = Some(format!("Cannot list drives: {error}"));
        }
    }

    fn start_scan(&mut self) {
        let path = self.path_input.trim().to_owned();
        if path.is_empty() {
            self.error = Some("Enter a path or pick a drive first".into());
            return;
        }
        if let Some(previous) = self.job.take() {
            previous.cancel();
        }
        let repaint = self.context.clone();
        match ScanJob::start(ScanConfig::new(path), move || repaint.request_repaint()) {
            Ok(job) => {
                self.job = Some(job);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Cannot start scan: {error}")),
        }
    }

    fn poll_mounts(&mut self) {
        if let Ok(mounts) = self.mounts_receiver.try_recv() {
            self.mounts = mounts;
        }
    }

    fn poll_scan(&mut self) {
        let Some(job) = &self.job else { return };
        let Some(result) = job.try_take() else {
            self.context.request_repaint_after(PROGRESS_POLL_INTERVAL);
            return;
        };
        let unreadable_dirs = job.config.progress.unreadable_dirs.load(Relaxed);
        self.job = None;
        match result {
            Ok(result) if !result.was_cancelled => {
                let mut session = Session::new(result, unreadable_dirs);
                if self.use_logical {
                    session.apply_size_mode(true);
                }
                self.session = Some(session);
                self.pending_deletes.clear();
                self.confirming_delete = None;
            }
            // A cancelled scan keeps the previous tree on screen
            Ok(_) => {}
            Err(error) => self.error = Some(format!("Scan failed: {error}")),
        }
    }

    fn poll_actions(&mut self) {
        while let Ok(result) = self.action_receiver.try_recv() {
            let was_pending = self.pending_deletes.remove(&result.id);
            match (result.outcome, &mut self.session) {
                (Ok(()), Some(session)) if was_pending => {
                    session.arena.mark_deleted(result.id);
                    session.refresh_after_delete();
                }
                (Err(message), _) => self.error = Some(format!("Delete failed: {message}")),
                _ => {}
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let selected = self
            .session
            .as_ref()
            .and_then(|session| session.tree.selected);
        let (permanent, trash, copy, rescan) = ctx.input_mut(|input| {
            (
                input.consume_key(Modifiers::SHIFT, Key::Delete),
                input.consume_key(Modifiers::NONE, Key::Delete),
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::Copy)),
                input.consume_key(Modifiers::NONE, Key::F5),
            )
        });
        match (selected, permanent, trash, copy) {
            (Some(id), true, _, _) => self.apply_action(RowAction::DeletePermanently(id)),
            (Some(id), _, true, _) => self.apply_action(RowAction::MoveToTrash(id)),
            (Some(id), _, _, true) => self.apply_action(RowAction::CopyPath(id)),
            _ => {}
        }
        if rescan {
            self.start_scan();
        }
    }

    fn apply_action(&mut self, action: RowAction) {
        match action {
            RowAction::OpenInFileManager(id) => self.open_in_file_manager(id),
            RowAction::CopyPath(id) => {
                if let Some(session) = &self.session {
                    self.context.copy_text(session.arena.path_of(id));
                }
            }
            RowAction::MoveToTrash(id) => self.start_delete(id, DeleteMode::Trash),
            RowAction::DeletePermanently(id) => self.confirming_delete = Some(id),
        }
    }

    fn open_in_file_manager(&mut self, id: NodeId) {
        let Some(session) = &self.session else { return };
        let path = session.arena.path_of(id);
        if let Err(error) = actions::open_in_file_manager(Path::new(&path)) {
            self.error = Some(format!("Cannot open {path}: {error}"));
        }
    }

    fn start_delete(&mut self, id: NodeId, mode: DeleteMode) {
        let Some(session) = &self.session else { return };
        let path = match actions::target_path(&session.arena, id) {
            Ok(path) => path,
            Err(error) => {
                self.error = Some(format!("Cannot delete: {error}"));
                return;
            }
        };
        if !self.pending_deletes.insert(id) {
            return;
        }
        let repaint = self.context.clone();
        let spawned =
            actions::spawn_delete(id, path, mode, self.action_sender.clone(), move || {
                repaint.request_repaint();
            });
        if let Err(error) = spawned {
            self.pending_deletes.remove(&id);
            self.error = Some(format!("Cannot start delete: {error}"));
        }
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        let was_logical = self.use_logical;
        let mut state = ToolbarState {
            path_input: &mut self.path_input,
            mounts: &self.mounts,
            use_logical: &mut self.use_logical,
            is_scanning: self.job.is_some(),
        };
        let event = toolbar::show(ui, &mut state);
        if self.use_logical != was_logical {
            if let Some(session) = &mut self.session {
                session.apply_size_mode(self.use_logical);
            }
        }
        match event {
            Some(ToolbarEvent::StartScan) => self.start_scan(),
            Some(ToolbarEvent::Stop) => {
                if let Some(job) = &self.job {
                    job.cancel();
                }
            }
            None => {}
        }
    }

    fn show_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(job) = &self.job {
                let progress = &job.config.progress;
                ui.spinner();
                ui.label(format!(
                    "Scanning... {} files, {} folders, {}",
                    format_count(progress.files.load(Relaxed)),
                    format_count(progress.dirs.load(Relaxed)),
                    format_size(progress.allocated_bytes.load(Relaxed)),
                ));
            } else if let Some(session) = &self.session {
                let root = session.root();
                ui.label(format!(
                    "Scanned {} files in {:.2?} | {} allocated",
                    format_count(u64::from(root.file_count)),
                    session.elapsed,
                    format_size(root.allocated_size),
                ));
                if session.unreadable_dirs > 0 {
                    let text = format!(
                        "⚠ {} folders unreadable",
                        format_count(session.unreadable_dirs)
                    );
                    ui.colored_label(WARNING_COLOR, text);
                }
            }
            if let Some(message) = self.error.clone() {
                ui.colored_label(Color32::LIGHT_RED, message);
                if ui.small_button("Dismiss").clicked() {
                    self.error = None;
                }
            }
        });
    }

    fn show_extensions(&mut self, ui: &mut egui::Ui) {
        let Some(session) = &mut self.session else {
            return;
        };
        let view = ExtView {
            stats: &session.ext_stats,
            extensions: &session.arena.extensions,
            colors: &session.colors,
            scan_total_bytes: session.root().allocated_size,
            selected: session.selected_ext,
        };
        if let Some(ext_id) = ext_view::show(ui, &view) {
            session.toggle_extension(ext_id);
        }
    }

    fn show_tree(&mut self, ui: &mut egui::Ui) -> Option<RowAction> {
        let Some(session) = &mut self.session else {
            ui.centered_and_justified(|ui| {
                ui.label("Pick a drive or enter a path, then press Scan")
            });
            return None;
        };
        let view = TreeView {
            arena: &session.arena,
            colors: &session.colors,
            use_logical: self.use_logical,
            highlighted_ext: session.selected_ext,
        };
        tree_view::show(ui, &mut session.tree, &view)
    }

    fn show_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirming_delete else {
            return;
        };
        let Some(session) = &self.session else {
            self.confirming_delete = None;
            return;
        };
        let path = session.arena.path_of(id);
        let mut confirmed = None;
        let modal = egui::Modal::new(egui::Id::new("confirm_delete")).show(ctx, |ui| {
            ui.heading("Delete permanently?");
            ui.label(&path);
            ui.label("This cannot be undone.");
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    confirmed = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    confirmed = Some(false);
                }
            });
        });
        match confirmed.or(modal.should_close().then_some(false)) {
            Some(true) => {
                self.confirming_delete = None;
                self.start_delete(id, DeleteMode::Permanent);
            }
            Some(false) => self.confirming_delete = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_mounts();
        self.poll_scan();
        self.poll_actions();
        self.handle_shortcuts(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| self.show_toolbar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.show_status(ui));
        if self.session.is_some() {
            egui::SidePanel::right("extensions")
                .resizable(true)
                .default_width(ctx.screen_rect().width() * 0.32)
                .show(ctx, |ui| self.show_extensions(ui));
        }
        let action = egui::CentralPanel::default()
            .show(ctx, |ui| self.show_tree(ui))
            .inner;
        if let Some(action) = action {
            self.apply_action(action);
        }
        self.show_delete_confirmation(ctx);
    }
}
