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

use horch_core::fleetd::task_execution::application::dispatch::Bound;
use horch_core::taskledger::finish;
use horch_core::taskledger::finish::codesweep::REVIEWERS;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::store::Store;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_e2e::bind::{bind, try_bind};
use horch_e2e::harness::{wait_for_or, Harness};
use horch_e2e::ledger_seed::seed_run;
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
    h.serve_fleetd();
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

/// 1 forced pass of fleetd on the run `alpha`, while the test fleetd is
/// down, with the fleet clock 30 s later.
fn pass(h: &Harness) {
    h.advance_fleet_clock(30);
    pass_flat(h);
}

/// 1 forced pass of fleetd with no change of the fleet clock.
fn pass_flat(h: &Harness) {
    let o = h.without_fleetd(|| {
        h.run(&[
            "ledger", "watch", "--once", "--force", "--json", "--run", "alpha",
        ])
    });
    assert!(o.status.success(), "horch ledger watch: {}", text(&o));
}

/// Passes until `f` gives a value.
fn until<T>(what: &str, h: &Harness, mut f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(
        what,
        || {
            pass(h);
            f()
        },
        || dump(h),
    )
}

/// Passes with no change of the fleet clock until `f` gives a value.
fn flat_until<T>(what: &str, h: &Harness, mut f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(
        what,
        || {
            pass_flat(h);
            f()
        },
        || dump(h),
    )
}

/// The state of the idea of the task T-1.
fn set_idea_state(h: &Harness, state: &str) {
    h.without_fleetd(|| {
        Store::open(&ledger(h))
            .unwrap()
            .conn()
            .execute(
                "UPDATE ideas SET state = ?1
                 WHERE id = (SELECT idea_id FROM tasks WHERE seq = 1)",
                [state],
            )
            .unwrap();
    });
}

/// The rows of the ledger, for the text of a timeout.
fn dump(h: &Harness) -> String {
    let tasks = query(
        h,
        "SELECT 'T-' || seq || ' ' || status || ' ' || coalesce(held_reason, '') FROM tasks",
    );
    let trees = query(
        h,
        "SELECT state || ' ' || coalesce(status, '') || ' w=' || substr(coalesce(worker_id, ''), 1, 6) || ' ' || path FROM worktrees",
    );
    let attempts = query(
        h,
        "SELECT id || ' T-' || coalesce((SELECT seq FROM tasks t WHERE t.task_id = attempts.task_id), '') || ' w=' || substr(worker_id, 1, 6) || ' ' || state || ' ' || coalesce(outcome, '') FROM attempts",
    );
    format!("{tasks:#?}\n{trees:#?}\n{attempts:#?}")
}

/// A run `alpha` with the finish steps `steps`.
fn seed_alpha(h: &Harness, steps: &str, max_attempts: Option<i64>) -> Run {
    let run = seed_run(h, "alpha", None, "w1:p1");
    let words: Vec<String> = steps.split(',').map(|s| format!("{s:?}")).collect();
    let conn = Store::open(&ledger(h)).unwrap();
    conn.conn()
        .execute(
            "UPDATE runs SET finish_steps = ?1 WHERE run_id = ?2",
            (format!("[{}]", words.join(",")), &run.run_id),
        )
        .unwrap();
    if let Some(n) = max_attempts {
        conn.conn()
            .execute(
                "UPDATE runs SET max_attempts = ?1 WHERE run_id = ?2",
                (n, &run.run_id),
            )
            .unwrap();
    }
    run
}

/// Queue the task `name` for `persona` in `run`, with the ticket
/// `tickets/<name>.md`, blocked by the tasks `blocked_by`.
fn queue(h: &Harness, run: &Run, name: &str, persona: &str, blocked_by: &[&str]) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join(format!("{name}.md"));
    let body = format!("# {name}\n\n**Check:** none\n");
    std::fs::write(&ticket, &body).unwrap();
    let new = NewTask {
        ticket_snapshot: Some(body),
        check_none: true,
        blocked_by: blocked_by.iter().map(|b| b.to_string()).collect(),
        ..NewTask::work(name, &ticket.to_string_lossy(), persona)
    };
    Store::open(&ledger(h))
        .unwrap()
        .immediate(|tx| tasks::add(tx, run, &new, Actor::Orchestrator))
        .unwrap();
}

/// The command `args` of the worker `w`, run as a process of its attempt:
/// the process is the wrapper of the attempt. The shell waits for the start
/// of the test, so the test records the process before the exec of `horch`.
fn in_attempt(h: &Harness, w: &str, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "read go; exec \"$@\"", "sh"])
        .arg(Harness::horch_bin())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    h.seal(&mut cmd);
    cmd.env("HORCH_WORKER_ID", w);
    let mut child = cmd.spawn().unwrap();
    let wrapper = child.id();
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE attempts SET pid = ?1, pid_start = ?2 WHERE worker_id = ?3",
            (wrapper, horch_core::procid::start_time(wrapper).unwrap(), w),
        )
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"go\n").unwrap();
    child.wait_with_output().unwrap()
}

/// [`in_attempt`] that must succeed.
fn ok_in_attempt(h: &Harness, w: &str, args: &[&str]) -> String {
    let o = in_attempt(h, w, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

/// The next task of `persona` is bound to a new worker (FR-5). fleetd makes
/// its tree in its passes; the worker makes 1 commit in it. Gives the worker
/// and the path.
fn bind_and_work(h: &Harness, persona: &str, name: &str) -> (String, PathBuf) {
    bind_and_work_as(h, persona, name, None)
}

/// [`bind_and_work`] for a worker with the agent process `agent`.
fn bind_and_work_as(
    h: &Harness,
    persona: &str,
    name: &str,
    agent: Option<u32>,
) -> (String, PathBuf) {
    let b = bind(h, persona, agent);
    let (w, task) = (b.worker_id, b.task_id);
    let path = flat_until("the active tree of the task", h, || {
        query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .pop()
        .map(PathBuf::from)
    });
    commit_file(h, &path, &format!("{name}.txt"));
    (w, path)
}

/// The tree path that the bind `b` names.
fn bound_tree(b: &Bound) -> PathBuf {
    let tree = b
        .worktree
        .as_ref()
        .unwrap_or_else(|| panic!("no tree: {b:?}"));
    PathBuf::from(&tree.path)
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
    json_of(
        h,
        &[
            "ledger",
            "show",
            &format!("T-{seq}"),
            "--json",
            "--run",
            "alpha",
        ],
    )["task"]
        .clone()
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

/// The park of the task of the worker `w`: `horch ledger done --unlanded`
/// writes only a result file now, and fleetd never parks a task from it, so
/// the test parks the task as the old `done --unlanded` did, while fleetd is
/// down.
fn park(h: &Harness, w: &str, reason: &str) {
    h.without_fleetd(|| {
        let mut store = Store::open(&ledger(h)).unwrap();
        finish::complete(&mut store, w, "parked", Some(reason)).unwrap();
    });
}

/// A run `alpha` with the land step: T-1 for `opus-a`, and T-2 for
/// `opus-b` that waits for T-1. A worker of `opus-a` works on T-1 in its
/// tree, with 1 commit and 1 untracked file, and parks it with
/// `done --unlanded`. Gives the worker and the path of the tree.
fn parked(h: &Harness) -> (String, PathBuf, Run) {
    let run = seed_alpha(h, "land", None);
    queue(h, &run, "01-a", "opus-a", &[]);
    queue(h, &run, "02-b", "opus-b", &["T-1"]);
    let (w, path) = bind_and_work(h, "opus-a", "feat");
    std::fs::write(path.join("notes.txt"), "untracked\n").unwrap();
    park(h, &w, "a red check");
    // fleetd moves the tree of the parked task to `created`.
    until("the park of the task", h, || {
        (column(h, &path, "state") == "created").then_some(())
    });
    (w, path, run)
}

// ─── RR-14: park with the tree, requeue in the same tree ────────────────

#[test]
fn pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents() {
    let h = world("p3-unl-park");
    let (_, path, _) = parked(&h);

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
    assert!(try_bind(&h, "opus-b", None).is_err(), "T-2 waits for T-1");
}

#[test]
fn rr14_requeue_of_a_parked_task_runs_in_the_same_tree() {
    let h = world("p3-unl-requeue");
    let (_, path, _) = parked(&h);
    ok(
        &h,
        &[
            "ledger",
            "requeue",
            "T-1",
            "the check is fixed",
            "--run",
            "alpha",
        ],
    );
    // The requeue gives the tree back: `open`, still `created`.
    assert_eq!(column(&h, &path, "status"), "open");
    assert_eq!(column(&h, &path, "state"), "created");

    let b = bind(&h, "opus-a", None);
    let w = b.worker_id.clone();
    assert_eq!(b.task_id, task(&h, 1)["task_id"], "{b:?}");
    assert_eq!(
        stored(&h, &bound_tree(&b)),
        stored(&h, &path),
        "the bind names the tree {}: {b:?}",
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
    // The tree that the bind names is the same tree, with its work.
    let again = bound_tree(&b);
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
    let (_, dismissed, run) = parked(&h);
    queue(&h, &run, "03-c", "opus-c", &[]);
    let (w3, reopened) = bind_and_work(&h, "opus-c", "other");
    park(&h, &w3, "a conflict");
    until("the park of the second task", &h, || {
        (column(&h, &reopened, "state") == "created").then_some(())
    });
    // A parked task is held for the operator. Its cancel makes its tree
    // unlanded work with no task.
    ok(
        &h,
        &["ledger", "cancel", "T-1", "dropped", "--run", "alpha"],
    );
    ok(
        &h,
        &["ledger", "cancel", "T-3", "dropped", "--run", "alpha"],
    );
    pass(&h);
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

    // A reopen gives the tree to the new task, and its bind takes it.
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
            "--run",
            "alpha",
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
        "--run",
        "alpha",
    ]);
    assert!(!o.status.success(), "{}", text(&o));
    let b = bind(&h, "opus-d", None);
    let w = b.worker_id.clone();
    assert_eq!(stored(&h, &bound_tree(&b)), stored(&h, &reopened), "{b:?}");
    assert_eq!(column(&h, &reopened, "state"), "active");
    assert_eq!(
        column(&h, &reopened, "owner_attempt"),
        attempt_of(&h, 4, &w)
    );
}

#[test]
fn rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref() {
    let h = world("p3-unl-orphans");
    let run = seed_alpha(&h, "land", None);
    queue(&h, &run, "01-a", "opus", &[]);
    queue(&h, &run, "02-b", "opus", &[]);

    // T-1: its worker is reaped, and the reconcile orphans its tree.
    let mut agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let (_, orphan) = bind_and_work_as(&h, "opus", "orphan", Some(agent.id()));
    // T-2: its tree is dropped. The drop of a worker is a request: fleetd
    // rescues the tree on a verified rescue ref.
    let (w2, rescued) = bind_and_work(&h, "opus", "rescued");
    let name = rescued.file_name().unwrap().to_string_lossy().into_owned();
    let o = h
        .horch(&[
            "ledger",
            "worktree",
            "drop",
            &name,
            "--reason",
            "stop",
            "--keep-branch",
        ])
        .env("HORCH_WORKER_ID", &w2)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let mut passes = 0;
    wait_for_or(
        "the rescue of the dropped tree",
        || {
            // The rescue waits for the result of a `worktree_list` job, which
            // comes with the clock: 1 short step after 3 passes of the same clock.
            passes += 1;
            if passes % 4 == 0 {
                h.advance_fleet_clock(30);
            }
            pass_flat(&h);
            (column(&h, &rescued, "state") == "rescued").then_some(())
        },
        || dump(&h),
    );
    // The process of T-1 ends: the next pass orphans its tree, and the pass
    // after it runs the `rescue` job. The findings are read between them.
    agent.kill().unwrap();
    agent.wait().unwrap();
    pass_flat(&h);
    assert_eq!(column(&h, &orphan, "state"), "orphaned", "{}", dump(&h));
    let rescue_ref = column(&h, &rescued, "rescue_ref");
    assert!(rescue_ref.starts_with("refs/horch/rescue/"), "{rescue_ref}");

    let o = finding_of(&h, &orphan);
    assert_eq!(o["task_id"], task(&h, 1)["task_id"], "{o}");
    let detail = o["detail"].as_str().unwrap();
    assert!(detail.contains("orphaned"), "{detail}");
    assert!(detail.contains(&orphan.display().to_string()), "{detail}");
    let r = finding_of(&h, &rescued);
    assert_eq!(r["task_id"], task(&h, 2)["task_id"], "{r}");
    let detail = r["detail"].as_str().unwrap();
    // The finding of a tree gives the state when it was found: the drop
    // orphaned the tree, and the rescue came after it.
    assert!(detail.contains("Rescue ref:"), "{detail}");
    assert!(detail.contains(&rescue_ref), "{detail}");
    // The text list names the rescue ref too.
    let said = ok(&h, &["ledger", "unfinished"]);
    assert!(said.contains(&rescue_ref), "{said}");

    // 1 decision each: a dismissed tree is not listed again.
    ok(
        &h,
        &["ledger", "dismiss", &o["finding_id"].to_string(), "x"],
    );
    pass_flat(&h);
    let list = unfinished(&h);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["finding_id"], r["finding_id"]);
}

/// T-1 of a run with the land step: its first worker makes the tree
/// with 1 commit, and its process ends. The reconcile orphans the tree.
/// `before_rescue` runs on the orphaned folder. fleetd rescues the tree, and
/// the next worker takes it, commits 1 more file, finishes, and the task is
/// done. No command reads `ledger unfinished` before the land. Gives the
/// path of the tree.
fn orphan_then_land(h: &Harness, before_rescue: impl Fn(&Path)) -> PathBuf {
    // The landing runs the fast gate that the project declares.
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    git_in(h, &h.project, &["add", "fleet.toml"]);
    git_in(
        h,
        &h.project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    );
    let run = seed_alpha(h, "land", None);
    queue(h, &run, "01-a", "opus", &[]);
    let mut agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let (_, path) = bind_and_work_as(h, "opus", "feat", Some(agent.id()));
    // fleetd starts no new task of a parked idea, so it does not start the
    // next attempt of T-1 in the pass that sees the end of this one; the
    // test binds it.
    set_idea_state(h, "parked");
    agent.kill().unwrap();
    agent.wait().unwrap();
    // The pass that sees the end of the process orphans the tree. The
    // rescue job comes after it.
    pass_flat(h);
    assert_eq!(column(h, &path, "state"), "orphaned", "{}", dump(h));
    before_rescue(&path);
    // The rescue of an orphan waits for the result of a `worktree_list` job.
    // The clock moves by 1 period only when the passes of the same clock did
    // not bring it, so fleetd does not start the next attempt first.
    let mut passes = 0;
    wait_for_or(
        "the rescue of the orphaned tree",
        || {
            passes += 1;
            if passes % 8 == 0 {
                h.advance_fleet_clock(60);
            }
            pass_flat(h);
            (column(h, &path, "state") == "rescued").then_some(())
        },
        || dump(h),
    );
    let rescue_ref = column(h, &path, "rescue_ref");
    assert!(rescue_ref.starts_with("refs/horch/rescue/"), "{rescue_ref}");

    // T-1 is ready again, and its next worker takes the same tree. No pass
    // runs between the end of the wait and the bind.
    // The clock passes the backoff of the failed attempt, and the test binds
    // T-1 with the idea still parked, so no pass starts the attempt first.
    let b = wait_for_or(
        "the bind of T-1",
        || {
            h.advance_fleet_clock(3600);
            let bound = try_bind(h, "opus", None).ok();
            if bound.is_none() {
                pass_flat(h);
            }
            bound
        },
        || dump(h),
    );
    set_idea_state(h, "in_progress");
    let w2 = b.worker_id.clone();
    flat_until("the same tree under the next attempt", h, || {
        (column(h, &path, "state") == "active").then_some(())
    });
    let _ = std::fs::remove_file(path.join("scratch.txt"));
    commit_file(h, &path, "more.txt");
    ok_in_attempt(h, &w2, &["ledger", "finish", "--json"]);
    flat_until("the task is done", h, || {
        (task(h, 1)["status"] == "done").then_some(())
    });
    // fleetd lands the branch of the task in a bundle.
    until("the landing of the branch", h, || {
        (!query(
            h,
            "SELECT name FROM branches WHERE task_id IS NOT NULL AND landing_state = 'landed'",
        )
        .is_empty())
        .then_some(())
    });
    path
}

#[test]
fn rr15_landed_work_of_a_done_task_is_not_listed() {
    horch_e2e::expected::check(
        file!(),
        "rr15_landed_work_of_a_done_task_is_not_listed",
        || {
            let h = world("p3-unl-landed");
            let path = orphan_then_land(&h, |_| {});
            // The rescue saved nothing beyond the HEAD, and that HEAD landed.
            let list = unfinished(&h);
            assert!(
            list.is_empty(),
            "unfinished lists landed work: a landing through a bundle never sets worktrees.landed_commit; ticket 168 (T-207) of fleetd-finish removes this entry\n{list:?}"
        );
            // The finding of the tree is not decided: it is only not listed.
            let id = column(&h, &path, "worktree_id");
            assert_eq!(
                query(
                    &h,
                    &format!("SELECT status FROM import_findings WHERE worktree_id = '{id}'")
                ),
                ["open"]
            );
        },
    );
}

#[test]
fn rr15_rescued_work_that_did_not_land_stays_listed_after_the_task_is_done() {
    let h = world("p3-unl-not-landed");
    let path = orphan_then_land(&h, |path| {
        std::fs::write(path.join("scratch.txt"), "only on the rescue ref\n").unwrap();
    });
    // The rescue ref holds scratch.txt, and scratch.txt never landed. The
    // first read of `unfinished` is after the land, and it lists the tree.
    let f = finding_of(&h, &path);
    let rescue_ref = column(&h, &path, "rescue_ref");
    assert!(f["detail"].as_str().unwrap().contains(&rescue_ref), "{f}");
    assert_eq!(finding_of(&h, &path), f, "listed until a decision");
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
    let run = seed_alpha(&h, "land", Some(2));
    queue(&h, &run, "01-a", "opus", &[]);
    // The first limit asks for the 1 replan (`failed`), the second holds
    // the task for the operator (`escalated`).
    attempts(&h, 1..=2, "failed");
    ok(
        &h,
        &["ledger", "requeue", "T-1", "the replan", "--run", "alpha"],
    );
    attempts(&h, 3..=4, "escalated");
    let t = task(&h, 1);
    assert_eq!(t["held_reason"], "2 attempts failed", "{t}");
    assert!(
        try_bind(&h, "opus", None).is_err(),
        "an escalated task does not run"
    );

    // Your requeue sets the count of the limit to 0: the task runs again,
    // with each of its attempts.
    ok(
        &h,
        &[
            "ledger",
            "requeue",
            "T-1",
            "the cause is fixed",
            "--run",
            "alpha",
        ],
    );
    let t = task(&h, 1);
    assert_eq!(t["attempt_floor"], 4, "{t}");
    assert!(t["held_reason"].is_null(), "{t}");
    attempts(&h, 5..=6, "escalated");
}

/// Attempts `ns` of T-1, each bound to a new worker whose process ends with
/// no result. Before the last, the task is queued again; after it, its
/// state is `end`. fleetd rescues the tree of each failed attempt, as the
/// next bind refuses an `orphaned` tree.
fn attempts(h: &Harness, ns: std::ops::RangeInclusive<i64>, end: &str) {
    let last = *ns.end();
    for n in ns {
        // The clock passes the backoff of the last failure.
        h.advance_fleet_clock(3600);
        let mut agent = std::process::Command::new("/bin/sleep")
            .arg("600")
            .spawn()
            .unwrap();
        let b = bind(h, "opus", Some(agent.id()));
        assert_eq!(b.task_id, task(h, 1)["task_id"], "{b:?}");
        assert_eq!(b.attempt, n, "{b:?}");
        agent.kill().unwrap();
        agent.wait().unwrap();
        pass_flat(h);
        until("the rescue of the tree of the failed attempt", h, || {
            (query(
                h,
                "SELECT CAST(COUNT(*) AS TEXT) FROM worktrees WHERE state = 'orphaned'",
            ) == ["0"]
                && task(h, 1)["status"] != "running")
                .then_some(())
        });
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
    for (_, r, _) in REVIEWERS {
        std::fs::write(
            sweep.join("reviewers").join(format!("{r}.md")),
            format!("You are {r}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    h
}

/// The review tasks, `(T-<seq>, status)` each, in seq order.
fn reviews(h: &Harness) -> Vec<(String, String)> {
    // `status` exits 1 while the run is not complete.
    let st: Value = serde_json::from_slice(
        &h.run(&["ledger", "status", "--json", "--run", "alpha"])
            .stdout,
    )
    .unwrap();
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
    let run = seed_alpha(&h, "code-sweep", None);
    queue(&h, &run, "01-a", "opus", &[]);
    let (w, _) = bind_and_work(&h, "opus", "feat");
    // `finish` writes the result: fleetd runs the code-sweep step in its
    // passes, and the step queues the review tasks.
    ok_in_attempt(&h, &w, &["ledger", "finish", "--json"]);
    let n = flat_until("the queued reviews", &h, || {
        let n = reviews(&h).len();
        (n > 0).then_some(n)
    });
    let before = task(&h, 1);
    let (first, _) = reviews(&h)[0].clone();
    ok(
        &h,
        &[
            "ledger",
            "cancel",
            &first,
            "the reviewer cannot start",
            "--run",
            "alpha",
        ],
    );

    // The step waits while a review of its round is open. The other reviews
    // end as `done`: the fixture of the reviewers' work, which is not the
    // subject of the test.
    h.without_fleetd(|| {
        Store::open(&ledger(&h))
            .unwrap()
            .conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE kind = 'review' AND status = 'ready'",
                [],
            )
            .unwrap();
    });
    // The code-sweep queues the cancelled review again, on its own counter.
    flat_until("the cancelled review queued again", &h, || {
        (reviews(&h).len() == n + 1).then_some(())
    });
    let said = query(
        &h,
        "SELECT coalesce(note, '') FROM task_steps WHERE name = 'code-sweep' ORDER BY seq",
    );
    assert!(
        said.iter()
            .any(|n| n.contains("1 cancelled reviews are queued again")),
        "{said:?}"
    );
    let all = reviews(&h);
    assert_eq!(all.len(), n + 1, "{all:?}");
    let after = task(&h, 1);
    for key in ["attempts", "attempt_floor", "status", "current_worker_id"] {
        assert_eq!(after[key], before[key], "{key}: {before} -> {after}");
    }
    // The cancelled review keeps its 1 attempt; the new review has none yet.
    let review = |seq: &str| -> Value {
        json_of(&h, &["ledger", "show", seq, "--json", "--run", "alpha"])["task"].clone()
    };
    assert_eq!(review(&first)["status"], "cancelled");
    let (new, status) = all.last().unwrap().clone();
    assert_eq!(status, "queued");
    assert_eq!(review(&new)["attempts"], 0);
}
