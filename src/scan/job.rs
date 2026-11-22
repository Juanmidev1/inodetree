//! Runs a scan on its own thread so a UI can poll it without blocking.

use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use super::{ScanConfig, ScanResult, vfs};

pub struct ScanJob {
    pub config: ScanConfig,
    receiver: Receiver<io::Result<ScanResult>>,
}

impl ScanJob {
    /// `on_done` runs on the scan thread after the result is queued, to wake a UI event loop.
    pub fn start(config: ScanConfig, on_done: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let thread_config = config.clone();
        thread::Builder::new()
            .name("inodetree-scan".into())
            .spawn(move || {
                // The receiver is gone when the job was dropped, so a failed send is expected
                let _ = sender.send(vfs::scan(&thread_config));
                on_done();
            })?;
        Ok(Self { config, receiver })
    }

    /// Returns the result once, as soon as the scan has finished.
    #[must_use]
    pub fn try_take(&self) -> Option<io::Result<ScanResult>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("scan thread ended unexpectedly")))
            }
        }
    }

    pub fn cancel(&self) {
        self.config
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
