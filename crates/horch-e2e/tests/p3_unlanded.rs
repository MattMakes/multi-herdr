//! Work that does not land, with the tree states of Phase 3 (fleetd-p3
//! ticket 14): the real `horch` binary, its own git repository, ledger and
//! state folders in a temp dir. `done --unlanded` parks the task with its
//! tree (`created`, no owner) and its dependents stay blocked; `requeue`
//! resumes it in the same tree; `ledger unfinished` lists each tree with
//! work that never landed until 1 decision (RR-14, RR-15, RR-18; rows L5,
//! L8 and L9 of `docs/specs/fleetd/preservation-04-behavior.md`, row V-14
//! of `preservation-05-contracts-and-values.md`). No fleetd runs.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::store::Store;
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
    h.set("HORCH_LEDGER_DB", ledger(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn ledger(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, w: &str, args: &[&str]) -> String {
    let o = h.horch(args).env("HORCH_WORKER_ID", w).output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    serde_json::from_str(&ok(h, args)).unwrap()
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

fn commit_file(h: &Harness, dir: &Path, file: &str) {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git_in(h, dir, &["add", file]);
    git_in(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
}

fn tree_path(said: &str) -> PathBuf {
    PathBuf::from(
        said.lines()
            .find_map(|l| l.strip_prefix("path: "))
            .unwrap_or_else(|| panic!("no path line: {said}")),
    )
}

/// Queue the task `name` for `persona`, with the ticket `tickets/<name>.md`.
fn queue(h: &Harness, name: &str, persona: &str, extra: &[&str]) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join(format!("{name}.md"));
    std::fs::write(&ticket, format!("# {name}\n\n**Check:** none\n")).unwrap();
    let ticket = ticket.to_string_lossy().into_owned();
    let mut args = vec!["ledger", "add", "--file", &ticket, "--primary", persona];
    args.extend_from_slice(extra);
    ok(h, &args);
}

fn worker_new(h: &Harness, persona: &str) -> String {
    ok(h, &["ledger", "worker", "new", "--persona", persona])
        .trim()
        .to_string()
}

/// A new worker of `persona` claims the next task and makes the tree
/// `name` with 1 commit. Gives the worker, its claim output and the path.
fn claim_and_work(h: &Harness, persona: &str, name: &str) -> (String, String, PathBuf) {
    let w = worker_new(h, persona);
    let claim = ok_as(h, &w, &["ledger", "claim"]);
    let path = tree_path(&ok_as(h, &w, &["ledger", "worktree", "new", name]));
    commit_file(h, &path, &format!("{name}.txt"));
    (w, claim, path)
}

/// The values of the first column of `sql`.
fn query(h: &Harness, sql: &str) -> Vec<String> {
    let store = Store::open(&ledger(h)).unwrap();
    let mut stmt = store.conn().prepare(sql).unwrap();
    let rows = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .unwrap()
        .map(|v| v.unwrap().unwrap_or_default())
        .collect();
    rows
}

/// `path` as the ledger writes it: with each link of the project folder
/// resolved (`/tmp` is `/private/tmp` on macOS).
fn stored(h: &Harness, path: &Path) -> String {
    let project = std::fs::canonicalize(&h.project).unwrap();
    match path.strip_prefix(&h.project) {
        Ok(rel) => project.join(rel).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// 1 column of the 1 row of the folder `path` that is not `removed`.
fn column(h: &Harness, path: &Path, name: &str) -> String {
    let rows = query(
        h,
        &format!(
            "SELECT {name} FROM worktrees WHERE path = '{}' AND state <> 'removed'",
            stored(h, path)
        ),
    );
    assert_eq!(rows.len(), 1, "the rows of {}: {rows:?}", path.display());
    rows[0].clone()
}

/// The state moves of the row of `path` that the door recorded,
/// `<from>><to>` each, oldest first.
fn moves(h: &Harness, path: &Path) -> Vec<String> {
    let id = column(h, path, "worktree_id");
    query(
        h,
        &format!(
            "SELECT from_state || '>' || to_state FROM events
             WHERE entity = 'worktree' AND entity_id = '{id}' AND to_state IS NOT NULL
             ORDER BY rowid"
        ),
    )
}

/// The cause text of the last state move of the row of `path`.
fn last_cause(h: &Harness, path: &Path) -> String {
    let id = column(h, path, "worktree_id");
    query(
        h,
        &format!(
            "SELECT text FROM events
             WHERE entity = 'worktree' AND entity_id = '{id}' AND to_state IS NOT NULL
             ORDER BY rowid DESC LIMIT 1"
        ),
    )
    .remove(0)
}

/// The attempt of `worker` at task `seq`.
fn attempt_of(h: &Harness, seq: i64, worker: &str) -> String {
    let rows = query(
        h,
        &format!(
            "SELECT a.id FROM attempts a JOIN tasks t ON t.task_id = a.task_id
             WHERE t.seq = {seq} AND a.worker_id = '{worker}'"
        ),
    );
    assert_eq!(rows.len(), 1, "the attempts of {worker}: {rows:?}");
    rows[0].clone()
}

/// The task `T-<seq>` as `ledger show --json` gives it.
fn task(h: &Harness, seq: i64) -> Value {
    json_of(h, &["ledger", "show", &format!("T-{seq}"), "--json"])["task"].clone()
}

/// The open findings of `ledger unfinished --json`.
fn unfinished(h: &Harness) -> Vec<Value> {
    json_of(h, &["ledger", "unfinished", "--json"])
        .as_array()
        .unwrap()
        .clone()
}

/// The 1 open finding of the tree `path`.
fn finding_of(h: &Harness, path: &Path) -> Value {
    let id = column(h, path, "worktree_id");
    let list = unfinished(h);
    let of: Vec<&Value> = list.iter().filter(|f| f["worktree_id"] == id).collect();
    assert_eq!(of.len(), 1, "the findings of {}: {list:?}", path.display());
    of[0].clone()
}

/// A run `alpha` with the land step: T-1 for `opus-a`, and T-2 for
/// `opus-b` that waits for T-1. A worker of `opus-a` works on T-1 in the
/// tree `feat`, with 1 commit and 1 untracked file, and parks it with
/// `done --unlanded`. Gives the worker and the path of the tree.
fn parked(h: &Harness) -> (String, PathBuf) {
    ok(h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
    queue(h, "01-a", "opus-a", &[]);
    queue(h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    let (w, _, path) = claim_and_work(h, "opus-a", "feat");
    std::fs::write(path.join("notes.txt"), "untracked\n").unwrap();
    ok_as(
        h,
        &w,
        &["ledger", "done", "--unlanded", "a red check", "parked"],
    );
    (w, path)
}

// ─── RR-14: park with the tree, requeue in the same tree ────────────────

#[test]
fn pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents() {
    let h = world("p3-unl-park");
    let (_, path) = parked(&h);

    // The tree and each file stay.
    assert!(path.join("feat.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(path.join("notes.txt")).unwrap(),
        "untracked\n"
    );
    // The tree is `created` with no owner, in the same transaction as the
    // park, through the door.
    assert_eq!(column(&h, &path, "status"), "unlanded");
    assert_eq!(column(&h, &path, "state"), "created");
    assert_eq!(column(&h, &path, "owner_attempt"), "");
    assert_eq!(moves(&h, &path).last().unwrap(), "active>created");
    let cause = last_cause(&h, &path);
    assert!(cause.contains("unlanded: a red check"), "{cause}");

    // The task is parked, and its dependent waits.
    let t1 = task(&h, 1);
    assert_eq!(t1["held_reason"], "unlanded", "{t1}");
    let t2 = task(&h, 2);
    assert_eq!(t2["state"], "pending", "{t2}");
    let w = worker_new(&h, "opus-b");
    let c: Value = serde_json::from_str(&ok_as(&h, &w, &["ledger", "claim", "--json"])).unwrap();
    assert_ne!(c["outcome"], "claimed", "T-2 waits for T-1: {c}");
}

#[test]
fn rr14_requeue_of_a_parked_task_runs_in_the_same_tree() {
    let h = world("p3-unl-requeue");
    let (_, path) = parked(&h);
    ok(&h, &["ledger", "requeue", "T-1", "the check is fixed"]);
    // The requeue gives the tree back: `open`, still `created`.
    assert_eq!(column(&h, &path, "status"), "open");
    assert_eq!(column(&h, &path, "state"), "created");

    let w = worker_new(&h, "opus-a");
    let claim = ok_as(&h, &w, &["ledger", "claim"]);
    assert!(claim.contains("Claimed T-1"), "{claim}");
    assert!(
        claim.contains(&path.display().to_string()),
        "the claim names the tree {}: {claim}",
        path.display()
    );
    assert_eq!(column(&h, &path, "state"), "active");
    assert_eq!(
        column(&h, &path, "owner_attempt"),
        attempt_of(&h, 1, &w),
        "the new attempt holds the tree"
    );
    assert_eq!(
        moves(&h, &path)[moves(&h, &path).len() - 2..],
        ["active>created".to_string(), "created>active".to_string()]
    );
    // `worktree new` gives the same tree, with its work.
    let again = tree_path(&ok_as(&h, &w, &["ledger", "worktree", "new", "feat"]));
    assert_eq!(stored(&h, &again), stored(&h, &path));
    assert!(again.join("feat.txt").is_file());
    assert!(again.join("notes.txt").is_file());
    // The dependent still waits until T-1 is done.
    assert_eq!(task(&h, 2)["state"], "pending");
}

// ─── RR-15: unlanded work stays on record until 1 decision ──────────────

#[test]
fn pres_04_l9_unlanded_work_is_listed_and_decided_once() {
    let h = world("p3-unl-record");
    let (_, dismissed) = parked(&h);
    queue(&h, "03-c", "opus-c", &[]);
    let (w3, _, reopened) = claim_and_work(&h, "opus-c", "other");
    ok_as(
        &h,
        &w3,
        &["ledger", "done", "--unlanded", "a conflict", "parked"],
    );
    // A parked task is held for the operator. Its cancel makes its tree
    // unlanded work with no task.
    ok(&h, &["ledger", "cancel", "T-1", "dropped"]);
    ok(&h, &["ledger", "cancel", "T-3", "dropped"]);
    let first = finding_of(&h, &dismissed);
    let second = finding_of(&h, &reopened);
    // Listed again on each read, until a decision.
    assert_eq!(finding_of(&h, &dismissed), first);

    let id = first["finding_id"].to_string();
    ok(&h, &["ledger", "dismiss", &id, "not needed"]);
    for _ in 0..2 {
        let list = unfinished(&h);
        assert!(
            !list.iter().any(|f| f["finding_id"] == first["finding_id"]),
            "a dismissed finding is not listed again: {list:?}"
        );
    }
    let o = h.run(&["ledger", "dismiss", &id, "again"]);
    assert!(
        !o.status.success() && text(&o).contains("is not open"),
        "a finding is decided 1 time: {}",
        text(&o)
    );
    assert!(
        dismissed.join("feat.txt").is_file(),
        "dismiss keeps the tree"
    );

    // A reopen gives the tree to the new task, and its claim takes it.
    let ticket = h.project.join("tickets/04-again.md");
    std::fs::write(&ticket, "# 04: Again\n\n**Check:** none\n").unwrap();
    let sid = second["finding_id"].to_string();
    ok(
        &h,
        &[
            "ledger",
            "reopen",
            &sid,
            "--file",
            &ticket.to_string_lossy(),
            "--primary",
            "opus-d",
        ],
    );
    assert!(unfinished(&h).is_empty(), "{:?}", unfinished(&h));
    let o = h.run(&[
        "ledger",
        "reopen",
        &sid,
        "--file",
        &ticket.to_string_lossy(),
        "--primary",
        "opus-d",
    ]);
    assert!(!o.status.success(), "{}", text(&o));
    let w = worker_new(&h, "opus-d");
    let claim = ok_as(&h, &w, &["ledger", "claim"]);
    assert!(
        claim.contains("Reopened from unfinished past work"),
        "{claim}"
    );
    assert!(claim.contains(&reopened.display().to_string()), "{claim}");
    assert_eq!(column(&h, &reopened, "state"), "active");
    assert_eq!(
        column(&h, &reopened, "owner_attempt"),
        attempt_of(&h, 4, &w)
    );
}

#[test]
fn rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref() {
    let h = world("p3-unl-orphans");
    ok(&h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
    queue(&h, "01-a", "opus", &[]);
    queue(&h, "02-b", "opus", &[]);

    // T-1: its worker is reaped, and the reconcile orphans its tree.
    let (w1, _, orphan) = claim_and_work(&h, "opus", "orphan");
    // T-2: its tree is dropped, but git refuses to remove a locked tree:
    // the drop leaves it `rescued`, on its verified rescue ref.
    let (w2, _, rescued) = claim_and_work(&h, "opus", "rescued");
    git_in(
        &h,
        &h.project,
        &["worktree", "lock", &rescued.to_string_lossy()],
    );
    let o = h
        .horch(&[
            "ledger",
            "worktree",
            "drop",
            "T2-rescued",
            "--reason",
            "stop",
            "--keep-branch",
        ])
        .env("HORCH_WORKER_ID", &w2)
        .output()
        .unwrap();
    assert!(!o.status.success(), "{}", text(&o));
    assert_eq!(column(&h, &rescued, "state"), "rescued", "{}", text(&o));
    let rescue_ref = column(&h, &rescued, "rescue_ref");
    assert!(rescue_ref.starts_with("refs/horch/rescue/"), "{rescue_ref}");
    ok(&h, &["ledger", "reap", &w1, "it hung"]);
    ok(&h, &["ledger", "watch", "--once", "--force"]);
    assert_eq!(column(&h, &orphan, "state"), "orphaned");
    assert_eq!(column(&h, &rescued, "state"), "rescued");

    let o = finding_of(&h, &orphan);
    assert_eq!(o["task_id"], task(&h, 1)["task_id"], "{o}");
    let detail = o["detail"].as_str().unwrap();
    assert!(detail.contains("orphaned"), "{detail}");
    assert!(detail.contains(&orphan.display().to_string()), "{detail}");
    let r = finding_of(&h, &rescued);
    assert_eq!(r["task_id"], task(&h, 2)["task_id"], "{r}");
    let detail = r["detail"].as_str().unwrap();
    assert!(detail.contains("rescued"), "{detail}");
    assert!(detail.contains(&rescue_ref), "{detail}");
    // The text list names the rescue ref too.
    let said = ok(&h, &["ledger", "unfinished"]);
    assert!(said.contains(&rescue_ref), "{said}");

    // 1 decision each: a dismissed tree is not listed again.
    ok(
        &h,
        &["ledger", "dismiss", &o["finding_id"].to_string(), "x"],
    );
    ok(&h, &["ledger", "watch", "--once", "--force"]);
    let list = unfinished(&h);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["finding_id"], r["finding_id"]);
}

/// T-1 of a run with the land step: its first worker makes the tree
/// `feat` with 1 commit and is reaped, and the reconcile orphans the tree.
/// `before_rescue` runs on the orphaned folder. The next worker rescues
/// and takes the tree, commits 1 more file, lands it and is done. Gives
/// the path of the tree, and the finding of the orphan before the land.
fn orphan_then_land(h: &Harness, before_rescue: impl Fn(&Path)) -> (PathBuf, Value) {
    ok(h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
    queue(h, "01-a", "opus", &[]);
    let (w1, _, path) = claim_and_work(h, "opus", "feat");
    ok(h, &["ledger", "reap", &w1, "it hung"]);
    ok(h, &["ledger", "watch", "--once", "--force"]);
    assert_eq!(column(h, &path, "state"), "orphaned");
    let listed = finding_of(h, &path);
    before_rescue(&path);

    let w2 = worker_new(h, "opus");
    ok_as(h, &w2, &["ledger", "claim"]);
    let again = tree_path(&ok_as(h, &w2, &["ledger", "worktree", "new", "feat"]));
    assert_eq!(stored(h, &again), stored(h, &path));
    let rescue_ref = column(h, &path, "rescue_ref");
    assert!(rescue_ref.starts_with("refs/horch/rescue/"), "{rescue_ref}");
    let _ = std::fs::remove_file(path.join("scratch.txt"));
    commit_file(h, &path, "more.txt");
    let o = h
        .horch(&["ledger", "finish", "--json"])
        .env("HORCH_WORKER_ID", &w2)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    ok_as(h, &w2, &["ledger", "done", "landed"]);
    assert_eq!(task(h, 1)["state"], "done");
    (path, listed)
}

#[test]
fn rr15_landed_work_of_a_done_task_is_not_listed() {
    let h = world("p3-unl-landed");
    let (path, listed) = orphan_then_land(&h, |_| {});
    // The rescue saved nothing beyond the HEAD, and that HEAD landed.
    let list = unfinished(&h);
    assert!(
        !list.iter().any(|f| f["finding_id"] == listed["finding_id"]),
        "{list:?}"
    );
    // The finding is not decided: it is only not listed.
    let id = column(&h, &path, "worktree_id");
    assert_eq!(
        query(
            &h,
            &format!("SELECT status FROM import_findings WHERE worktree_id = '{id}'")
        ),
        ["open"]
    );
}

#[test]
fn rr15_rescued_work_that_did_not_land_stays_listed_after_the_task_is_done() {
    let h = world("p3-unl-not-landed");
    let (path, listed) = orphan_then_land(&h, |path| {
        std::fs::write(path.join("scratch.txt"), "only on the rescue ref\n").unwrap();
    });
    // The rescue ref holds scratch.txt, and scratch.txt never landed.
    let f = finding_of(&h, &path);
    assert_eq!(f["finding_id"], listed["finding_id"]);
    let rescue_ref = column(&h, &path, "rescue_ref");
    assert!(f["detail"].as_str().unwrap().contains(&rescue_ref), "{f}");
    ok(
        &h,
        &[
            "ledger",
            "dismiss",
            &f["finding_id"].to_string(),
            "not needed",
        ],
    );
    assert!(unfinished(&h).is_empty(), "{:?}", unfinished(&h));
}

// ─── RR-18: the attempt count of a requeue ──────────────────────────────

#[test]
fn rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0() {
    let h = world("p3-unl-attempts");
    ok(
        &h,
        &[
            "ledger",
            "run",
            "new",
            "alpha",
            "--steps",
            "land",
            "--max-attempts",
            "2",
        ],
    );
    queue(&h, "01-a", "opus", &[]);
    // The first limit asks for the 1 replan (`failed`), the second holds
    // the task for the operator (`escalated`).
    attempts(&h, 1..=2, "failed");
    ok(&h, &["ledger", "requeue", "T-1", "the replan"]);
    attempts(&h, 3..=4, "escalated");
    let t = task(&h, 1);
    assert_eq!(t["held_reason"], "2 attempts failed", "{t}");
    let w = worker_new(&h, "opus");
    let c: Value = serde_json::from_str(&ok_as(&h, &w, &["ledger", "claim", "--json"])).unwrap();
    assert_ne!(
        c["outcome"], "claimed",
        "an escalated task does not run: {c}"
    );

    // Your requeue sets the count of the limit to 0: the task runs again,
    // with each of its attempts.
    ok(&h, &["ledger", "requeue", "T-1", "the cause is fixed"]);
    let t = task(&h, 1);
    assert_eq!(t["attempt_floor"], 4, "{t}");
    assert!(t["held_reason"].is_null(), "{t}");
    attempts(&h, 5..=6, "escalated");
}

/// Attempts `ns` of T-1, each claimed by a new worker and reaped. Before
/// the last, the task is queued again; after it, its state is `end`.
fn attempts(h: &Harness, ns: std::ops::RangeInclusive<i64>, end: &str) {
    let last = *ns.end();
    for n in ns {
        let w = worker_new(h, "opus");
        let claim = ok_as(h, &w, &["ledger", "claim"]);
        assert!(
            claim.contains(&format!("Claimed T-1 (attempt {n})")),
            "{claim}"
        );
        ok(h, &["ledger", "reap", &w, "it hung"]);
        let t = task(h, 1);
        let want = if n == last { end } else { "ready" };
        assert_eq!(t["state"], want, "after attempt {n}: {t}");
        assert_eq!(t["attempts"], n, "{t}");
    }
}

/// A world with a bare remote and the code-sweep reviewer prompts, as the
/// code-sweep step needs them.
fn sweep_world(name: &str) -> Harness {
    let mut h = world(name);
    let cli = h.write_warmed(
        "fake-complexity",
        b"#!/bin/sh\necho '{\"files\":[],\"violations\":[]}'\n",
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let sweep = h.root.join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for r in [
        "bug-hunter",
        "clarity-checker",
        "coverage-analyst",
        "vulnerability-scanner",
        "structure-inspector",
        "efficiency-auditor",
        "contract-guardian",
        "waste-detector",
        "reliability-auditor",
        "dependency-examiner",
        "power-of-ten-reviewer",
        "interface-auditor",
    ] {
        std::fs::write(
            sweep.join("reviewers").join(format!("{r}.md")),
            format!("You are {r}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    h
}

fn finish_json(h: &Harness, worker: &str) -> Value {
    let o = h
        .horch(&["ledger", "finish", "--json"])
        .env("HORCH_WORKER_ID", worker)
        .output()
        .unwrap();
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

/// The review tasks, `(T-<seq>, status)` each, in seq order.
fn reviews(h: &Harness) -> Vec<(String, String)> {
    // `status` exits 1 while the run is not complete.
    let st: Value = serde_json::from_slice(&h.run(&["ledger", "status", "--json"]).stdout).unwrap();
    st["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| &t["task"])
        .filter(|t| t["kind"] == "review")
        .map(|t| {
            (
                format!("T-{}", t["seq"]),
                t["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn rr18_review_requeue_keeps_the_attempt_count() {
    let h = sweep_world("p3-unl-review");
    ok(
        &h,
        &["ledger", "run", "new", "alpha", "--steps", "code-sweep"],
    );
    queue(&h, "01-a", "opus", &[]);
    let (w, _, _) = claim_and_work(&h, "opus", "feat");
    let r = finish_json(&h, &w);
    let n = r["queued_reviews"].as_array().unwrap().len();
    assert!(n > 0, "{r}");
    let before = task(&h, 1);
    let (first, _) = reviews(&h)[0].clone();
    ok(
        &h,
        &["ledger", "cancel", &first, "the reviewer cannot start"],
    );

    // The code-sweep queues the cancelled review again, on its own counter.
    let r = finish_json(&h, &w);
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("1 cancelled reviews are queued again"),
        "{r}"
    );
    let all = reviews(&h);
    assert_eq!(all.len(), n + 1, "{all:?}");
    let after = task(&h, 1);
    for key in ["attempts", "attempt_floor", "status", "current_worker_id"] {
        assert_eq!(after[key], before[key], "{key}: {before} -> {after}");
    }
    // The cancelled review keeps its 1 attempt; the new review has none yet.
    let review =
        |seq: &str| -> Value { json_of(&h, &["ledger", "show", seq, "--json"])["task"].clone() };
    assert_eq!(review(&first)["status"], "cancelled");
    let (new, status) = all.last().unwrap().clone();
    assert_eq!(status, "queued");
    assert_eq!(review(&new)["attempts"], 0);
}
