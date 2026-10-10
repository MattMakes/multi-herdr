//! `horch teammates --check` prints the context-policy warnings and still
//! exits 0 (u3 review finding 2b).
//!
//! Hermetic: the environment is cleared and the roster is the built-ins plus
//! a temp `HORCH_TEAMMATES_DIR`.

use std::path::Path;
use std::process::{Command, Output};

fn horch(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_horch"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_TEAMMATES_DIR", root.join("teammates"))
        .output()
        .unwrap()
}

#[test]
fn teammates_check_warns_about_a_copied_messages_base_and_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["home", "state", "data", "teammates/_base"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let shipped =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/_base/context-messages.md");
    let text = std::fs::read_to_string(shipped)
        .unwrap()
        .replace("[horch] BLOCKED: Compaction", "Compaction");
    let copy = root.join("teammates/_base/context-messages.md");
    std::fs::write(&copy, text).unwrap();

    let out = horch(&root, &["teammates", "--check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let warning = format!(
        "warning: _base/context-messages.md: {} replaces the built-in protocol messages",
        copy.display()
    );
    assert_eq!(stderr.matches(&warning).count(), 1, "{stderr}");
}
