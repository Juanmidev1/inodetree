use std::fs;
use std::os::unix::fs::symlink;

use inodetree::actions::{DeleteMode, delete_path, target_path};
use inodetree::arena::ROOT;
use inodetree::scan::{ScanConfig, vfs};

#[test]
fn permanent_delete_removes_files_and_directories() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    let folder = dir.path().join("folder");
    fs::write(&file, b"x").unwrap();
    fs::create_dir_all(folder.join("nested")).unwrap();
    fs::write(folder.join("nested/inner"), b"x").unwrap();

    delete_path(&file, DeleteMode::Permanent).unwrap();
    delete_path(&folder, DeleteMode::Permanent).unwrap();

    assert!(!file.exists() && !folder.exists());
}

#[test]
fn deleting_a_directory_symlink_keeps_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"x").unwrap();
    let link = dir.path().join("link");
    symlink(&target, &link).unwrap();

    delete_path(&link, DeleteMode::Permanent).unwrap();

    assert!(fs::symlink_metadata(&link).is_err());
    assert!(target.join("keep.txt").exists());
}

#[test]
fn deleting_a_missing_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(delete_path(&dir.path().join("missing"), DeleteMode::Permanent).is_err());
}

#[test]
fn target_path_refuses_the_scan_root() {
    let dir = tempfile::tempdir().unwrap();
    let arena = vfs::scan(&ScanConfig::new(dir.path())).unwrap().arena;
    assert!(target_path(&arena, ROOT).is_err());
}

#[test]
fn target_path_rebuilds_existing_paths_and_rejects_vanished_ones() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("gone.txt"), b"x").unwrap();
    let arena = vfs::scan(&ScanConfig::new(dir.path())).unwrap().arena;
    let child = arena.children(ROOT).next().unwrap();

    let path = target_path(&arena, child).unwrap();
    assert!(path.ends_with("gone.txt"));
    fs::remove_file(&path).unwrap();
    assert!(target_path(&arena, child).is_err());
}

#[test]
fn target_path_refuses_mount_points() {
    let shm = std::path::Path::new("/dev/shm");
    let is_mount = shm.is_dir() && {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(shm).unwrap().dev() != fs::metadata("/dev").unwrap().dev()
    };
    if !is_mount {
        return; // nothing mounted below /dev on this machine
    }
    let arena = vfs::scan(&ScanConfig::new("/dev")).unwrap().arena;
    let shm_id = (0..arena.nodes.len() as u32)
        .find(|&id| arena.name(id) == "shm")
        .unwrap();

    let error = target_path(&arena, shm_id).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}
