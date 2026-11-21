//! SR-7 for the process starts of the task ledger and `vcs/` (fleetd-p7
//! ticket 07): a child that they start never has a forbidden name, also when
//! the parent has it in its environment.
//!
//! Each test runs this test binary again with each forbidden name in its
//! environment, as a shell of the operator can have it. The ignored test
//! that it runs makes 1 start of 1 folder, and the child of that start
//! writes its environment to the file of `ENV_OUT`.

#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::taskledger::finish::{pr, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::triage::apply;
use horch_core::taskledger::Store;
use horch_core::vcs::landing::Landing;
use test_jobs::TestJobs;

/// The file that the child of the start writes its environment to.
const ENV_OUT: &str = "HORCH_TEST_START_ENV";

/// Run the ignored test `child` in a new process of this binary, with each
/// forbidden name set, and return the environment that its started child
/// wrote.
fn env_of_child(child: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("env.txt");
    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent
        .args(["--exact", child, "--ignored", "--test-threads=1"])
        .env(ENV_OUT, &out);
    for key in FORBIDDEN_ENV {
        parent.env(key, "from-the-parent");
    }
    let run = parent.output().unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let seen = std::fs::read_to_string(&out).unwrap();
    assert!(
        seen.lines().any(|l| l.starts_with(&format!("{ENV_OUT}="))),
        "the rest of the environment is missing:\n{seen}"
    );
    seen
}

fn assert_no_forbidden_name(seen: &str) {
    for key in FORBIDDEN_ENV {
        assert!(
            !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
            "{key} reached the child"
        );
    }
}

/// The file of `ENV_OUT`, in the process that the parent test started.
fn env_out() -> PathBuf {
    PathBuf::from(std::env::var_os(ENV_OUT).expect("run by its parent test"))
}

/// An executable `name` in `dir` that writes its environment to the file of
/// `ENV_OUT`, then prints `stdout`.
fn env_printer(dir: &Path, name: &str, stdout: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\nenv > \"${ENV_OUT}\"\nprintf '%s\\n' '{stdout}'\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A git config file with an author, for the git calls of a test.
fn git_config(dir: &Path) -> PathBuf {
    let path = dir.join("gitconfig");
    std::fs::write(
        &path,
        "[user]\n\tname = t\n\temail = t@example.com\n[commit]\n\tgpgsign = false\n",
    )
    .unwrap();
    path
}

fn git(dir: &Path, config: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
}

/// A repository with 1 commit on `main` and the branch `fleet/r1`.
fn repo(top: &Path) -> (PathBuf, PathBuf) {
    let config = git_config(top);
    let root = top.join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &config, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    git(&root, &config, &["add", "a.txt"]);
    git(&root, &config, &["commit", "-qm", "a"]);
    git(&root, &config, &["branch", "fleet/r1"]);
    (root, config)
}

/// The run r1 of `root` in a new ledger in `top`.
fn run(top: &Path, root: &Path) -> (Store, Run) {
    let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                root,
                &NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some("fleet/r1".into()),
                    remote: Some("origin".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    (store, run)
}

#[test]
fn finish_pr_check_starts_gh_without_a_forbidden_name() {
    assert_no_forbidden_name(&env_of_child(
        "finish_pr_check_in_a_parent_with_a_forbidden_name",
    ));
}

#[test]
#[ignore = "run by finish_pr_check_starts_gh_without_a_forbidden_name"]
fn finish_pr_check_in_a_parent_with_a_forbidden_name() {
    let _ = env_out();
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let (root, _) = repo(&top);
    let (_store, mut run) = run(&top, &root);
    run.pr_number = Some(1);
    let gh = env_printer(&top, "gh", r#"{"state":"OPEN"}"#);
    let checked = pr::pr_check(&gh, &Git::new("git"), &run).unwrap();
    assert!(
        matches!(checked, runs::PrCheck::Unmerged { number: 1, .. }),
        "{checked:?}"
    );
}

#[test]
fn triage_pr_remote_starts_gh_without_a_forbidden_name() {
    assert_no_forbidden_name(&env_of_child(
        "triage_pr_remote_in_a_parent_with_a_forbidden_name",
    ));
}

#[test]
#[ignore = "run by triage_pr_remote_starts_gh_without_a_forbidden_name"]
fn triage_pr_remote_in_a_parent_with_a_forbidden_name() {
    let _ = env_out();
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let (root, _) = repo(&top);
    let gh = env_printer(&top, "gh", "gh version 2");
    assert_eq!(apply::pr_remote(&Git::new("git"), &root, &gh), None);
}

#[test]
fn run_gate_starts_the_check_without_a_forbidden_name() {
    assert_no_forbidden_name(&env_of_child("run_gate_in_a_parent_with_a_forbidden_name"));
}

#[test]
#[ignore = "run by run_gate_starts_the_check_without_a_forbidden_name"]
fn run_gate_in_a_parent_with_a_forbidden_name() {
    let _ = env_out();
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let (root, _) = repo(&top);
    let (mut store, run) = run(&top, &root);
    let tools = Tools {
        git: Git::new("git"),
        gh: top.join("gh"),
        node: "/bin/sh".into(),
        complexity_cli: None,
        code_sweep_dir: None,
        evidence_root: top.join("evidence"),
        shell: "/bin/sh".into(),
        reviewers_down: Vec::new(),
        codex: CodexSandbox::default(),
        jobs: Some(std::sync::Arc::new(TestJobs)),
    };
    let gate = runs::run_gate(
        &mut store,
        &tools,
        &run,
        &format!("env > \"${ENV_OUT}\""),
        &top.join("logs"),
    )
    .unwrap();
    assert_eq!(gate.exit_code, Some(0), "{gate:?}");
}

#[test]
fn vcs_land_starts_the_check_without_a_forbidden_name() {
    assert_no_forbidden_name(&env_of_child("vcs_land_in_a_parent_with_a_forbidden_name"));
}

#[test]
#[ignore = "run by vcs_land_starts_the_check_without_a_forbidden_name"]
fn vcs_land_in_a_parent_with_a_forbidden_name() {
    let _ = env_out();
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let (root, config) = repo(&top);
    let landing = Landing::new("git".into(), top.join("state"))
        .with_env("GIT_CONFIG_GLOBAL", config.to_str().unwrap())
        .with_env("GIT_CONFIG_NOSYSTEM", "1");
    let claim = landing.create(&root, "worker", None, "one").unwrap();
    std::fs::write(claim.path.join("b.txt"), "b\n").unwrap();
    git(&claim.path, &config, &["add", "b.txt"]);
    git(&claim.path, &config, &["commit", "-qm", "b"]);
    landing
        .land(
            &claim.path,
            "worker",
            Some("one"),
            Some(&format!("env > \"${ENV_OUT}\"")),
        )
        .unwrap();
}
