#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::finish::{self, WorktreeRow};
use horch_core::taskledger::tasks::{self, Task};
use horch_core::taskledger::Store;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn harness(label: &str) -> Harness {
    let mut h = Harness::new(label).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    let teammates = h.root.join("teammates");
    std::fs::create_dir_all(&teammates).unwrap();
    for (name, extra) in [("boxed", "sandbox: {}\n"), ("open", "")] {
        let body = format!(
            "---\nname: {name}\nbrief_description: Test-only {name}.\nhidden: true\n\
             base: fleet-worker\nagent: claude\nmodel: opus\n{extra}---\nTest-only teammate.\n"
        );
        std::fs::write(teammates.join(format!("{name}.md")), body).unwrap();
    }
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", label])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let o = h.run(&["ledger", "run", "new", "boxed-run"]);
    assert!(o.status.success(), "{}", text(&o));
    h
}

fn queue(h: &Harness, persona: &str) {
    let file = h.project.join(format!("{persona}-ticket.md"));
    std::fs::write(&file, format!("# {persona}\n**Check:** `true`\n")).unwrap();
    let file = file.to_string_lossy().into_owned();
    let o = h.run(&["ledger", "add", "--file", &file, "--primary", persona]);
    assert!(o.status.success(), "{}", text(&o));
}

fn spawn(h: &Harness, persona: &str) -> Output {
    h.run(&[
        "spawn",
        persona,
        "--exact",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ])
}

fn brief(h: &Harness, role: &str) -> Value {
    let path = h
        .state
        .join(format!("herdr-orchestration-w1/{role}.brief.json"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&raw).expect("the brief is JSON")
}

fn board(h: &Harness) -> (Task, Option<WorktreeRow>) {
    let o = h.run(&["ledger", "status", "--json"]);
    let status: Value = serde_json::from_slice(&o.stdout).unwrap();
    let run = status["run"]["run_id"].as_str().expect("a run id");
    let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    let task = tasks::of_run(store.conn(), run).unwrap().remove(0);
    let row = finish::worktree_of(store.conn(), &task.task_id).unwrap();
    (task, row)
}

fn place(h: &Harness, persona: &str) -> PathBuf {
    h.project.join(".worktrees").join(format!("T1-{persona}"))
}

fn local_branches(project: &Path) -> String {
    let out = std::process::Command::new("git")
        .current_dir(project)
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_worktree_exists_before_a_sandboxed_claude_launches() {
    let mut h = harness("boxed-before");
    queue(&h, "boxed");
    h.set("HORCH_FAULT", "abort-after-brief");

    let o = spawn(&h, "boxed");
    assert!(
        !o.status.success(),
        "the fault stops the launch: {}",
        text(&o)
    );

    let workdir = brief(&h, "boxed-1")["workdir"]
        .as_str()
        .unwrap_or_else(|| panic!("the brief has no workdir: {}", text(&o)))
        .to_string();
    assert!(workdir.ends_with(".worktrees/T1-boxed"), "{workdir}");
    assert!(Path::new(&workdir).join(".git").is_file(), "{workdir}");
    let (task, row) = board(&h);
    let row = row.expect("the ledger holds the worktree");
    assert_eq!(row.path, workdir);
    assert_eq!(row.worker_id, None, "the claim adopts it");
    assert_eq!(task.status, "queued");
}

#[test]
fn a_failed_launch_of_a_sandboxed_claude_leaves_no_worktree_or_branch() {
    let mut h = harness("boxed-failed");
    queue(&h, "boxed");
    let before = local_branches(&h.project);
    h.set("HORCH_FAULT", "fail-pane-split");

    let o = spawn(&h, "boxed");
    assert!(!o.status.success(), "{}", text(&o));

    assert!(!place(&h, "boxed").exists(), "{}", text(&o));
    assert_eq!(local_branches(&h.project), before);
    let (task, row) = board(&h);
    assert_eq!(row, None);
    assert_eq!(task.status, "queued");
    assert_eq!(task.current_worker_id, None);
}

#[test]
fn an_unsandboxed_claude_keeps_the_claim_path() {
    let h = harness("open-claim");
    queue(&h, "open");

    let o = spawn(&h, "open");
    assert!(o.status.success(), "{}", text(&o));

    assert!(brief(&h, "open-1").get("workdir").is_none());
    assert!(!place(&h, "open").exists());
    assert_eq!(board(&h).1, None);
}
