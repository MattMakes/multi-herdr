//! Phase 0 red tests for dispatch and the result of a worker (design
//! `docs/specs/fleetd/design.md`, FM-2 and FM-6). Both turn green in Phase 4.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

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

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

/// A harness in workspace `w1` (root pane `w1:p1`) whose panes run their
/// commands, like `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

/// Run the run-creation command `run`, then queue 1 task for `persona`.
fn queue_one(h: &Harness, run: &[&str], persona: &str) {
    ok(h, run);
    let p = ticket(h, "t0", "# T0\n");
    ok(
        h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            persona,
        ],
    );
}

/// Each file named `name` under `dir`, at any depth.
fn files_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.file_name().is_some_and(|n| n == name) {
                found.push(p);
            }
        }
    }
    found
}

/// The worker id of the 1 worker of kind `worker` in `st`.
fn the_worker(st: &Value) -> String {
    st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["kind"] == "worker")
        .unwrap_or_else(|| panic!("no worker: {st}"))["worker"]["worker_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The status row of task `seq` in `st`.
fn task_row(st: &Value, seq: i64) -> Value {
    st["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["task"]["seq"] == seq)
        .unwrap_or_else(|| panic!("no task {seq}: {st}"))
        .clone()
}

#[test]
fn fm2_agent_that_never_claims_has_its_task_bound_at_spawn() {
    horch_e2e::expected::check(
        file!(),
        "fm2_agent_that_never_claims_has_its_task_bound_at_spawn",
        || {
            let h = fleet("p0-fm2", "exec,stay");
            queue_one(&h, &["ledger", "run", "new", "alpha"], "opus");
            let o = h.run(&[
                "spawn",
                "opus",
                "--from-pane",
                "w1:p1",
                "--no-tile",
                "--force",
            ]);
            assert!(o.status.success(), "{}", text(&o));
            // The fake agent never runs `horch ledger claim`. Push dispatch
            // binds the task to the attempt before the launch.
            // `status` exits 1 while work remains.
            let o = h.run(&["ledger", "status", "--json"]);
            let st: Value =
                serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
            let id = the_worker(&st);
            let row = task_row(&st, 1);
            assert!(
                row["worker"]["worker_id"] == id.as_str() && row["task"]["status"] != "queued",
                "the task is not bound to worker {id} at spawn: {}",
                text(&o)
            );
            // fleetd writes task.yaml for the attempt before the launch
            // (design, "Workspace layout and file protocol").
            let tasks = h.state.join("tasks");
            let yaml = files_named(&tasks, "task.yaml");
            assert!(
                !yaml.is_empty(),
                "no task.yaml under {} at spawn: {}",
                tasks.display(),
                text(&o)
            );
        },
    );
}

#[test]
fn fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt() {
    horch_e2e::expected::check(
        file!(),
        "fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt",
        || {
            let h = fleet("p0-fm6", "exec,stay");
            queue_one(
                &h,
                &[
                    "ledger",
                    "run",
                    "new",
                    "beta",
                    "--check-every",
                    "1",
                    "--stall-rounds",
                    "1",
                ],
                "opus",
            );
            let w = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
                .trim()
                .to_string();
            // The test plays the agent: a child process that it ends.
            let mut agent = std::process::Command::new("/bin/sleep")
                .arg("600")
                .spawn()
                .unwrap();
            horch_core::taskledger::supervisor::agent_started(
                &h.root.join("ledger.sqlite3"),
                &w,
                agent.id(),
            )
            .unwrap();
            ok_as(&h, &w, &["ledger", "claim"]);
            let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "work"]);
            let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
            std::fs::write(path.join("work.rs"), "fn work() {}\n").unwrap();
            git_in(&h, &path, &["add", "."]);
            git_in(
                &h,
                &path,
                &["commit", "--quiet", "--no-gpg-sign", "-m", "work"],
            );
            let commit = git_in(&h, &path, &["rev-parse", "HEAD"]);
            // The agent ends with no `done` and no result.
            agent.kill().unwrap();
            agent.wait().unwrap();

            let watch = h.run(&["ledger", "watch", "--once", "--force", "--json"]);
            assert!(watch.status.success(), "{}", text(&watch));
            let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
            assert_eq!(t["task"]["status"], "queued", "{t}: {}", text(&watch));
            // The retry waits for its backoff of 30 s on the fleet clock
            // (rung 6).
            h.advance_fleet_clock(30);

            // The next attempt starts by push dispatch (Phase 4: no claim).
            // Its task.yaml names the same worktree, with the commit in it.
            let o = h.run(&[
                "spawn",
                "opus",
                "--from-pane",
                "w1:p1",
                "--no-tile",
                "--force",
            ]);
            assert!(o.status.success(), "{}", text(&o));
            let shown = format!("worktree: {}", path.display());
            let tasks = h.state.join("tasks");
            let next = files_named(&tasks, "task.yaml").into_iter().find(|f| {
                let y = std::fs::read_to_string(f).unwrap_or_default();
                y.lines().any(|l| l.trim() == "attempt: A2") && y.lines().any(|l| l.trim() == shown)
            });
            assert!(
                next.is_some(),
                "no task.yaml of attempt A2 under {} names the worktree {}: {}",
                tasks.display(),
                path.display(),
                text(&o)
            );
            assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD"]), commit);
        },
    );
}
