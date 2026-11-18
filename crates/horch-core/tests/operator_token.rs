//! The operator token (ticket 10 of fleetd-p5): SR-8. Each test uses a
//! temporary fleet root and never reads the token of the live fleet.
#![cfg(unix)]

use std::fs::{self, Permissions};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::token_file::{ensure, read};

fn token_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("operator.token")
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn first_start_makes_a_token_with_mode_0600() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");

    let token = ensure(&root).unwrap();

    let path = token_path(&root);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&root.join("run")), 0o700);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.len(), 64, "{text}");
    assert!(text.bytes().all(|b| b.is_ascii_hexdigit()), "{text}");
    assert!(token.matches(&text));
    assert!(read(&root).unwrap().matches(&text));
}

#[test]
fn second_start_keeps_the_token() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");
    ensure(&root).unwrap();
    let first = fs::read_to_string(token_path(&root)).unwrap();

    let token = ensure(&root).unwrap();

    assert_eq!(fs::read_to_string(token_path(&root)).unwrap(), first);
    assert!(token.matches(&first));
}

#[test]
fn token_file_with_mode_0644_is_refused_with_its_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");
    ensure(&root).unwrap();
    let path = token_path(&root);
    let before = fs::read_to_string(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();

    let reason = format!("{:#}", read(&root).unwrap_err());
    assert!(reason.contains("mode 0644"), "{reason}");
    assert!(reason.contains("operator.token"), "{reason}");

    let reason = format!("{:#}", ensure(&root).unwrap_err());
    assert!(reason.contains("mode 0644"), "{reason}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        before,
        "no second token"
    );
    assert_eq!(mode(&path), 0o644);
}

#[test]
fn token_file_that_is_a_link_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");
    ensure(&root).unwrap();
    let path = token_path(&root);
    let elsewhere = tmp.path().join("planted.token");
    fs::write(&elsewhere, "a".repeat(64)).unwrap();
    fs::set_permissions(&elsewhere, Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&elsewhere, &path).unwrap();

    let reason = format!("{:#}", read(&root).unwrap_err());
    assert!(reason.contains("link"), "{reason}");

    let reason = format!("{:#}", ensure(&root).unwrap_err());
    assert!(reason.contains("link"), "{reason}");
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "a".repeat(64));
}

#[test]
fn wrong_token_does_not_match() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");
    let token = ensure(&root).unwrap();
    let text = fs::read_to_string(token_path(&root)).unwrap();
    let last = text.chars().last().unwrap();
    let other = if last == '0' { '1' } else { '0' };
    let wrong = format!("{}{other}", &text[..63]);

    assert!(!token.matches(&wrong));
    assert!(!token.matches(&"0".repeat(64)));
}

#[test]
fn token_of_other_length_does_not_match() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fleet");
    let token = ensure(&root).unwrap();
    let text = fs::read_to_string(token_path(&root)).unwrap();

    assert!(!token.matches(""));
    assert!(!token.matches(&text[..63]));
    assert!(!token.matches(&format!("{text}0")));
    assert!(!token.matches(&format!("{text}\n")));
}
