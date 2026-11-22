//! Background work of `App`: mounts listing, scans and deletions, and their result polling.

use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use super::app::App;
use super::session::Session;
use crate::actions::{self, DeleteMode};
use crate::arena::NodeId;
use crate::mounts::{self, MountInfo};
use crate::scan::ScanConfig;
use crate::scan::job::ScanJob;

const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

impl App {
    pub(super) fn load_mounts(&mut self, sender: Sender<Vec<MountInfo>>) {
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

    pub(super) fn start_scan(&mut self) {
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

    pub(super) fn poll_mounts(&mut self) {
        if let Ok(mounts) = self.mounts_receiver.try_recv() {
            self.mounts = mounts;
        }
    }

    pub(super) fn poll_scan(&mut self) {
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

    pub(super) fn poll_actions(&mut self) {
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

    pub(super) fn open_in_file_manager(&mut self, id: NodeId) {
        let Some(session) = &self.session else { return };
        let path = session.arena.path_of(id);
        if let Err(error) = actions::open_in_file_manager(Path::new(&path)) {
            self.error = Some(format!("Cannot open {path}: {error}"));
        }
    }

    pub(super) fn start_delete(&mut self, id: NodeId, mode: DeleteMode) {
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
}
