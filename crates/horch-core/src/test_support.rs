//! Helpers shared by the unit tests of several modules.

use std::path::Path;

/// Make a FIFO at `path`, with its parent directory.
pub(crate) fn mkfifo(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success());
}
