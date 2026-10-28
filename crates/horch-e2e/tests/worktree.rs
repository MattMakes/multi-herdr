//! ARC-31 end-to-end tests for worker worktree landing and the done guard.

#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

use horch_core::execution::legacy::{Record, STATUS_DONE};
use horch_core::execution::records::Ledger;
use horch_e2e::harness::Harness;
use serde_json::Value;

const WORKSPACE: &str = "worktree-e2e";
const RECORD: &str = "record-worktree-e2e";
const ROLE: &str = "sonnet-1";

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn git(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut command = h.git_cmd(args);
    command.current_dir(dir);
    let output = command.output().expect("running git");
    assert!(output.status.success(), "{}", stderr(&output));
    stdout(&output).trim().to_string()
}

fn worker_world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    assert!(h.git_bin().is_some(), "git is required for this test");
    h.set("HORCH_ROLE", ROLE);
    h.set("HORCH_RECORD_ID", RECORD);
    h
}

fn new_worktree(h: &Harness, name: &str) -> std::path::PathBuf {
    let output = h.run(&["worktree", "new", name]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("path: "))
        .expect("worktree path in stdout");
    std::path::PathBuf::from(path)
}

fn commit_work(h: &Harness, path: &Path) -> String {
    std::fs::write(path.join("worker.txt"), "worker work\n").unwrap();
    git(h, path, &["add", "worker.txt"]);
    git(
        h,
        path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "worker"],
    );
    git(h, path, &["rev-parse", "HEAD"])
}

fn create_target_pane(h: &Harness) -> String {
    let mut command = Command::new(h.bin.join("herdr"));
    command.args(["workspace", "create", "--label", "worktree-e2e"]);
    h.seal(&mut command);
    let output = command.output().expect("running fake herdr");
    assert!(output.status.success(), "{}", stderr(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    response["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn prepare_done(h: &mut Harness) -> String {
    let pane = create_target_pane(h);
    let mailbox = h.state.join(format!("herdr-orchestration-{WORKSPACE}"));
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), &pane).unwrap();
    h.set("HORCH_WORKSPACE_ID", WORKSPACE);
    h.set("HERDR_PANE_ID", pane.clone());
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: RECORD.into(),
            role: ROLE.into(),
            task: "worktree e2e".into(),
            ..Record::default()
        })
        .unwrap();
    pane
}

#[test]
fn arc_31_e2e_new_land_list() {
    let h = worker_world("arc31-new-land-list");
    let worktree = new_worktree(&h, "unit");
    let commit = commit_work(&h, &worktree);

    let mut land = h.horch(&["worktree", "land", "unit"]);
    land.current_dir(&worktree);
    let landed = land.output().expect("landing from the worker worktree");
    assert!(landed.status.success(), "{}", stderr(&landed));
    assert!(stdout(&landed).contains("LANDED wt/sonnet-1-unit -> main"));
    assert_eq!(git(&h, &h.project, &["rev-parse", "main"]), commit);
    assert!(!worktree.exists());

    let listed = h.run(&["worktree", "list"]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(stdout(&listed).contains("unit sonnet-1 landed wt/sonnet-1-unit main"));
    assert!(stdout(&listed).contains("unregistered: none"));
}

#[test]
fn arc_31_e2e_done_refuses_then_unlanded_reports() {
    let mut h = worker_world("arc31-done-guard");
    prepare_done(&mut h);
    let worktree = new_worktree(&h, "unit");
    let commit = commit_work(&h, &worktree);
    let short = &commit[..7];

    let refused = h.run(&["done", "s"]);
    assert!(!refused.status.success());
    let expected = format!(
        "horch: horch done refused because this record has an open worktree claim:\n\
         wt/sonnet-1-unit at {short} (1 commits ahead of main), worktree {}\n\
         Run: horch worktree land\n\
         If it cannot land: horch done --unlanded \"<reason>\" \"<summary>\"\n",
        worktree.display()
    );
    assert_eq!(stderr(&refused), expected);

    let completed = h.run(&["done", "--unlanded", "r", "s"]);
    assert!(completed.status.success(), "{}", stderr(&completed));
    let record = Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .get(RECORD)
        .unwrap();
    assert_eq!(record.status, STATUS_DONE);
    let calls = serde_json::to_string(&h.calls_of("herdr")).unwrap();
    assert!(calls.contains("UNLANDED:"), "{calls}");
    assert!(calls.contains("wt/sonnet-1-unit"), "{calls}");
}
