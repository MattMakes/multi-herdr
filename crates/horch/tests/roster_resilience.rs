//! F7: one teammate file this binary cannot parse never takes the roster
//! down. `horch teammates` lists the rest and warns once; `--check` fails.
//!
//! Hermetic: the environment is cleared and the roster is the built-ins plus
//! a temp `HORCH_TEAMMATES_DIR`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A temp home and roster directory holding one file that is ahead of the
/// binary: `requires` names a value it does not know.
fn world() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["home", "state", "data", "teammates"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let opus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/opus.md");
    let text = std::fs::read_to_string(opus)
        .unwrap()
        .replace("name: opus", "name: newcomer")
        .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1);
    std::fs::write(root.join("teammates/newcomer.md"), text).unwrap();
    (tmp, root)
}

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
fn f7_teammates_lists_the_rest_and_warns_once() {
    let (_tmp, root) = world();
    let out = horch(&root, &["teammates"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("opus"), "{stdout}");
    assert!(!stdout.contains("newcomer"), "{stdout}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("warning:"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    let file = root.join("teammates/newcomer.md");
    assert!(
        warnings[0].contains(&file.display().to_string()),
        "{stderr}"
    );
    assert!(
        warnings[0].contains("unknown variant `no-such-tool`"),
        "{stderr}"
    );
}

#[test]
fn f7_teammates_check_still_fails_on_the_file() {
    let (_tmp, root) = world();
    let out = horch(&root, &["teammates", "--check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("teammate 'newcomer' did not load"),
        "{stderr}"
    );
    assert!(
        stderr.contains("unknown variant `no-such-tool`"),
        "{stderr}"
    );
    // Reported once, as a problem, not also as a load warning.
    assert_eq!(stderr.matches("did not load").count(), 1, "{stderr}");
}
