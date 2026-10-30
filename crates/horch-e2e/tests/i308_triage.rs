//! Worktree triage at the CLI seam (#308): the real `horch` binary, a git
//! repository and a ledger file in a temp dir.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_e2e::harness::Harness;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

/// A worktree `.worktrees/<name>` that no ledger row names, with 1 commit.
fn folder(h: &Harness, name: &str) -> PathBuf {
    let path = h.project.join(".worktrees").join(name);
    git_in(
        h,
        &h.project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            name,
            &path.to_string_lossy(),
        ],
    );
    std::fs::write(path.join(format!("{name}.txt")), "work\n").unwrap();
    git_in(h, &path, &["add", "-A"]);
    git_in(
        h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", name],
    );
    path
}

fn reply(h: &Harness, rows: &[(&str, &str, &str, &str)]) -> PathBuf {
    let mut body = String::from(
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n",
    );
    for (id, name, decision, direction) in rows {
        body.push_str(&format!("| {id} | {name} | | {decision} | {direction} |\n"));
    }
    let p = h.root.join("reply.md");
    std::fs::write(&p, body).unwrap();
    p
}

fn triage_json(h: &Harness) -> Value {
    let o = h.run(&["worktree", "triage", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn triage_lists_a_parked_folder_and_prints_the_reply_template() {
    let h = world("i308-list");
    folder(&h, "lh-target-3");
    let v = triage_json(&h);
    assert_eq!(v["rows"][0]["id"], "W1");
    assert_eq!(v["rows"][0]["name"], "lh-target-3");
    assert_eq!(v["rows"][0]["ahead"], 1);
    let shown = ok(&h, &["worktree", "triage"]);
    assert!(
        shown.starts_with("W1 lh-target-3 [folder] branch lh-target-3 base main\n"),
        "{shown}"
    );
    assert!(shown.contains("| W1 | lh-target-3 |"), "{shown}");
}

#[test]
fn a_bad_reply_names_its_lines_and_runs_nothing() {
    let h = world("i308-bad");
    let path = folder(&h, "feat");
    let r = reply(
        &h,
        &[("W1", "feat", "DELETE", ""), ("W2", "ghost", "KEEP", "")],
    );
    let o = h.run(&["worktree", "triage", "apply", &r.to_string_lossy()]);
    assert!(!o.status.success(), "{}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("no decision ran"), "{err}");
    assert!(
        err.contains("line 4: unknown ID 'W2'; this triage has W1 to W1"),
        "{err}"
    );
    assert!(path.exists());
}

#[test]
fn finish_queues_a_task_that_inherits_the_worktree_and_wakes_the_orchestrator() {
    let h = world("i308-finish");
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let path = folder(&h, "T7-cache");
    let r = reply(
        &h,
        &[("W1", "T7-cache", "FINISH", "Fix the flaky test, then land")],
    );
    let said = ok(&h, &["worktree", "triage", "apply", &r.to_string_lossy()]);
    assert!(
        said.starts_with(
            "W1 T7-cache FINISH: T-1 queued for opus with the brief ai_docs/triage/T7-cache.md"
        ),
        "{said}"
    );
    let status = ok(&h, &["ledger", "show", "T-1", "--json"]);
    let task: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(task["task"]["status"], "queued", "{task:#}");
    assert!(path.exists());
    let events: Value = serde_json::from_str(&ok(&h, &["ledger", "events", "--json"])).unwrap();
    let woke = events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "woke")
        .filter_map(|e| e["text"].as_str())
        .find(|t| t.contains("worktree triage queued T-1 (FINISH)"));
    assert!(woke.is_some(), "{events:#}");
    // The task's worktree has a worker coming: it is no longer parked.
    assert_eq!(triage_json(&h)["rows"], serde_json::json!([]));
}

#[test]
fn archive_restore_and_gc_through_the_cli() {
    let h = world("i308-archive");
    let path = folder(&h, "feat");
    std::fs::write(path.join("untracked.txt"), "keep\n").unwrap();
    let r = reply(&h, &[("W1", "feat", "archive", "")]);
    let said = ok(&h, &["worktree", "triage", "apply", &r.to_string_lossy()]);
    assert!(
        said.starts_with("W1 feat ARCHIVE: archived at refs/horch/archive/feat"),
        "{said}"
    );
    assert!(!path.exists());
    let said = ok(&h, &["worktree", "restore", "feat"]);
    assert!(said.starts_with("restored "), "{said}");
    assert_eq!(
        std::fs::read_to_string(path.join("untracked.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        ok(&h, &["ledger", "gc"]).trim(),
        "No trash ref is older than 30 days."
    );
}

/// A run `alpha` with task T-1 claimed by a worker that made the worktree
/// `T1-f` and committed in it. Returns the worker id and the path.
fn claimed_worktree(h: &Harness) -> (String, PathBuf) {
    ok(h, &["ledger", "run", "new", "alpha"]);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let t = dir.join("01-f.md");
    std::fs::write(&t, "# 01: Feature\n").unwrap();
    ok(
        h,
        &[
            "ledger",
            "add",
            "--file",
            &t.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = ok(h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let as_worker = |args: &[&str]| {
        let mut cmd = h.horch(args);
        cmd.env("HORCH_WORKER_ID", &w);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
        out(&o)
    };
    as_worker(&["ledger", "claim"]);
    let said = as_worker(&["ledger", "worktree", "new", "f"]);
    let path = PathBuf::from(said.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(path.join("f.txt"), "work\n").unwrap();
    git_in(h, &path, &["add", "f.txt"]);
    git_in(h, &path, &["commit", "--quiet", "--no-gpg-sign", "-m", "f"]);
    (w, path)
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn run_close_refuses_while_a_parked_worktree_has_no_decision() {
    let h = world("i308-close");
    let (_, path) = claimed_worktree(&h);
    ok(&h, &["ledger", "cancel", "T-1", "not needed now"]);
    let status = ok(&h, &["ledger", "status"]);
    assert!(status.contains("Parked worktrees: 1 ("), "{status}");
    let o = h.run(&["ledger", "run", "close"]);
    assert!(!o.status.success(), "{}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains(&path.to_string_lossy().into_owned()), "{err}");
    assert!(err.contains("horch worktree triage"), "{err}");
    let r = reply(&h, &[("W1", &name_of(&path), "KEEP", "")]);
    ok(&h, &["worktree", "triage", "apply", &r.to_string_lossy()]);
    let said = ok(&h, &["ledger", "run", "close"]);
    assert!(said.contains("Run alpha closed as done."), "{said}");
}

#[test]
fn run_close_abandon_cancels_then_refuses_until_the_worktree_is_decided() {
    let h = world("i308-abandon");
    let (_, path) = claimed_worktree(&h);
    let o = h.run(&["ledger", "run", "close", "--abandon", "stop here"]);
    assert!(!o.status.success(), "{}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("horch worktree triage"), "{err}");
    // The cancel stands; the run stays active until the decision.
    let t: Value = serde_json::from_str(&ok(&h, &["ledger", "show", "T-1", "--json"])).unwrap();
    assert_eq!(t["task"]["status"], "cancelled", "{t:#}");
    let rows = triage_json(&h);
    assert_eq!(rows["rows"][0]["status"], "abandoned", "{rows:#}");
    let r = reply(&h, &[("W1", &name_of(&path), "ARCHIVE", "")]);
    ok(&h, &["worktree", "triage", "apply", &r.to_string_lossy()]);
    assert!(!path.exists());
    let said = ok(&h, &["ledger", "run", "close", "--abandon", "stop here"]);
    assert!(said.contains("Run alpha closed as abandoned"), "{said}");
}
