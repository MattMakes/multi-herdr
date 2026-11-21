//! SR-7 for the process starts of the `horch` CLI commands (fleetd-p7
//! ticket 09): a child that a command starts never has a forbidden name,
//! also when the shell of the operator has it in the environment of `horch`.
//!
//! The test runs the development `horch` with temporary folders only. Its
//! child is a small script that writes its environment to the file of
//! `ENV_OUT`.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_platform::start::FORBIDDEN_ENV;

/// The file that the child of the start writes its environment to.
const ENV_OUT: &str = "HORCH_TEST_START_ENV";

/// A `git` that writes its environment to the file of `ENV_OUT` when it
/// runs `git lfs version`, and prints a version.
fn env_printing_git(dir: &Path) -> PathBuf {
    let path = dir.join("git");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = lfs ]; then env > \"${ENV_OUT}\"; fi\n\
             printf '%s\\n' 'git-lfs/3.0.0'\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A herdr CLI that answers every call with success and an empty list.
fn fake_herdr(dir: &Path) -> PathBuf {
    let path = dir.join("herdr");
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[],\"panes\":[]}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn assert_no_forbidden_name(seen: &str) {
    assert!(
        seen.lines().any(|l| l.starts_with(&format!("{ENV_OUT}="))),
        "the rest of the environment is missing:\n{seen}"
    );
    for key in FORBIDDEN_ENV {
        assert!(
            !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
            "{key} reached the child"
        );
    }
}

/// `crates/horch/src/cmd`: `horch doctor` runs `git lfs version` for an
/// offered teammate that requires Git LFS (an Unreal project).
#[test]
fn doctor_git_lfs_child_has_no_forbidden_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Game.uproject"), "{}\n").unwrap();
    let out = root.join("env.txt");
    let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    let mut doctor = Command::new(env!("CARGO_BIN_EXE_horch"));
    doctor
        .arg("doctor")
        .current_dir(&project)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_DATA_DIR", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
        .env("HORCH_PROJECT_DIR", &project)
        .env("HORCH_TEAMMATES_DIR", teammates)
        .env("HORCH_GIT_BIN", env_printing_git(&root))
        .env("HORCH_HERDR_BIN", fake_herdr(&root))
        .env(ENV_OUT, &out);
    for key in FORBIDDEN_ENV {
        doctor.env(key, "from-the-parent");
    }
    let run = doctor.output().unwrap();
    let seen = std::fs::read_to_string(&out).unwrap_or_else(|e| {
        panic!(
            "horch doctor did not run git lfs version ({e}); stderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        )
    });
    assert_no_forbidden_name(&seen);
}
