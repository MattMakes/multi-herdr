//! Every git layer of horch-core runs git through the 1 runner, so every git
//! child gets the same environment and config (#101): no prompt, the C
//! locale, no system config, no hooks, and none of the caller's repository
//! variables.
//!
//! The parent test runs each layer in a child test process whose own
//! environment carries a `GIT_DIR`, numbered config variables, a prompt and a
//! locale. A fake git records what it receives.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::git::Git;
use horch_core::vcs::git::{GitCli, GitClient};
use horch_core::vcs::landing::Landing;

/// The directory of the fake git, which a child test reads.
const FAKE_DIR: &str = "W3_VCS_FSX_FAKE_DIR";

/// A fake git that writes its arguments and environment to `call-<pid>`
/// next to itself. It is run once here: macOS scans a new executable at its
/// first exec.
fn fake_git(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let git = dir.join("git");
    std::fs::write(
        &git,
        "#!/bin/sh\nif [ \"$1\" = warm ]; then\n  exit 0\nfi\n\
         { printf 'ARGS %s\\n' \"$*\"; env; } > \"$(dirname \"$0\")/call-$$\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Command::new(&git).arg("warm").status().unwrap().success());
    git
}

/// Run the ignored test `child` with a caller environment that git must not
/// see, and return the 1 call the fake git recorded.
fn record_one_call(child: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    fake_git(tmp.path());
    let decoy = tmp.path().join("decoy.git");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", child, "--test-threads=1"])
        .env(FAKE_DIR, tmp.path())
        .env("GIT_DIR", &decoy)
        .env("GIT_WORK_TREE", tmp.path())
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.editor")
        .env("GIT_CONFIG_VALUE_0", "w3-decoy-editor")
        .env("GIT_TERMINAL_PROMPT", "1")
        .env("LC_ALL", "fr_FR.UTF-8")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{child}: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls: Vec<PathBuf> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("call-")
        })
        .collect();
    assert_eq!(calls.len(), 1, "{child}: 1 git call");
    std::fs::read_to_string(&calls[0]).unwrap()
}

fn assert_runner_env(layer: &str, call: &str) {
    let lines: Vec<&str> = call.lines().collect();
    for set in ["GIT_TERMINAL_PROMPT=0", "LC_ALL=C", "GIT_CONFIG_NOSYSTEM=1"] {
        assert!(lines.contains(&set), "{layer}: no {set} in\n{call}");
    }
    for removed in [
        "GIT_DIR=",
        "GIT_WORK_TREE=",
        "GIT_CONFIG_COUNT=",
        "GIT_CONFIG_KEY_0=",
        "GIT_CONFIG_VALUE_0=",
    ] {
        assert!(
            !lines.iter().any(|l| l.starts_with(removed)),
            "{layer}: {removed} reached git in\n{call}"
        );
    }
    assert!(
        lines[0].contains("-c core.hooksPath=/dev/null"),
        "{layer}: hooks are not off: {}",
        lines[0]
    );
}

/// The directory a child test works in, and the fake git in it.
fn child_setup() -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(std::env::var_os(FAKE_DIR).expect("run by the parent test"));
    let git = dir.join("git");
    (dir, git)
}

#[test]
fn task_ledger_git_gets_the_runner_env() {
    let call = record_one_call("task_ledger_git_child");
    assert!(call.starts_with("ARGS ") && call.lines().next().unwrap().ends_with(" status"));
    assert_runner_env("taskledger::git::Git", &call);
}

#[test]
#[ignore = "run by task_ledger_git_gets_the_runner_env"]
fn task_ledger_git_child() {
    let (dir, git) = child_setup();
    let out = Git::new(git).try_run(&dir, &["status"]).unwrap();
    assert!(out.ok);
}

#[test]
fn dataset_git_cli_gets_the_runner_env() {
    let call = record_one_call("dataset_git_cli_child");
    assert_runner_env("vcs::git::GitCli", &call);
}

#[test]
#[ignore = "run by dataset_git_cli_gets_the_runner_env"]
fn dataset_git_cli_child() {
    let (dir, git) = child_setup();
    // The fake prints nothing: the branch is empty, not an error.
    let _ = GitCli::new(git).current_branch(&dir).unwrap();
}

#[test]
fn landing_git_gets_the_runner_env() {
    let call = record_one_call("landing_git_child");
    assert_runner_env("vcs::landing::Landing", &call);
}

#[test]
#[ignore = "run by landing_git_gets_the_runner_env"]
fn landing_git_child() {
    let (dir, git) = child_setup();
    // The fake lists no worktree, so the call fails after 1 git run.
    let error = Landing::new(git, dir.join("state"))
        .claims(&dir)
        .unwrap_err();
    assert!(error.to_string().contains("no main worktree"), "{error}");
}
