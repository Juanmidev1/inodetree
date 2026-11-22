use std::fs;
use std::time::{Duration, Instant};

use inodetree::scan::ScanConfig;
use inodetree::scan::job::ScanJob;

fn wait_for_result(job: &ScanJob) -> std::io::Result<inodetree::scan::ScanResult> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(result) = job.try_take() {
            return result;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("scan job did not finish in time");
}

#[test]
fn job_delivers_the_arena_and_runs_the_callback() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"hello").unwrap();
    let (notify, notified) = std::sync::mpsc::channel();

    let job = ScanJob::start(ScanConfig::new(dir.path()), move || {
        notify.send(()).unwrap();
    })
    .unwrap();
    let result = wait_for_result(&job).unwrap();

    assert_eq!(result.arena.nodes[0].file_count, 1);
    assert!(notified.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn cancelled_job_reports_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let config = ScanConfig::new(dir.path());
    config
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let job = ScanJob::start(config, || {}).unwrap();
    assert!(wait_for_result(&job).unwrap().was_cancelled);
}

#[test]
fn invalid_root_arrives_as_an_error() {
    let job = ScanJob::start(ScanConfig::new("/nonexistent/inodetree-root"), || {}).unwrap();
    assert!(wait_for_result(&job).is_err());
}
