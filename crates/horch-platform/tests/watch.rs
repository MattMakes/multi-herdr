//! The concern watch: file watching as a speed-up (PL-2, RR-4).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_platform::watch::Watcher;

/// Poll `changed` until it gives `dir`, for at most 2 s.
fn reported_within_2s(watcher: &mut Watcher, dir: &Path) -> bool {
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        if watcher.changed().iter().any(|p| p == dir) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn new_dir(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn a_new_file_in_a_watched_folder_is_reported() {
    let root = tempfile::tempdir().unwrap();
    let dir = new_dir(root.path(), "outbox");
    let mut watcher = Watcher::new().unwrap();
    watcher.add(&dir).unwrap();

    std::fs::write(dir.join("result.json"), b"{}").unwrap();

    assert!(reported_within_2s(&mut watcher, &dir));
}

#[test]
fn an_unwatched_folder_is_not_reported() {
    let root = tempfile::tempdir().unwrap();
    let watched = new_dir(root.path(), "watched");
    let other = new_dir(root.path(), "other");
    let mut watcher = Watcher::new().unwrap();
    watcher.add(&watched).unwrap();

    std::fs::write(other.join("result.json"), b"{}").unwrap();
    std::fs::write(watched.join("result.json"), b"{}").unwrap();

    let mut seen = Vec::new();
    let end = Instant::now() + Duration::from_millis(500);
    while Instant::now() < end {
        seen.extend(watcher.changed());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(seen.contains(&watched), "{seen:?}");
    assert!(!seen.contains(&other), "{seen:?}");
}

#[test]
fn changed_never_blocks() {
    let root = tempfile::tempdir().unwrap();
    let dir = new_dir(root.path(), "outbox");
    let mut watcher = Watcher::new().unwrap();
    watcher.add(&dir).unwrap();
    // Drain the events of the setup, if the OS gives any.
    let _ = watcher.changed();

    let start = Instant::now();
    let changed = watcher.changed();
    assert!(start.elapsed() < Duration::from_millis(50), "{changed:?}");

    let empty = Watcher::new().unwrap().changed();
    assert!(empty.is_empty(), "{empty:?}");
}

#[test]
fn a_removed_folder_is_not_an_error() {
    let root = tempfile::tempdir().unwrap();
    let gone = new_dir(root.path(), "gone");
    let kept = new_dir(root.path(), "kept");
    let mut watcher = Watcher::new().unwrap();
    watcher.add(&gone).unwrap();
    watcher.add(&kept).unwrap();

    std::fs::remove_dir(&gone).unwrap();
    let _ = watcher.changed();
    watcher.remove(&gone);
    // A folder that was never added is not an error either.
    watcher.remove(&root.path().join("never"));

    std::fs::write(kept.join("result.json"), b"{}").unwrap();
    assert!(reported_within_2s(&mut watcher, &kept));
}

#[test]
fn a_missing_folder_cannot_be_added() {
    let root = tempfile::tempdir().unwrap();
    let mut watcher = Watcher::new().unwrap();
    assert!(watcher.add(&root.path().join("missing")).is_err());
}
