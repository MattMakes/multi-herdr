//! The gate tests of Phase 1 for the reconcile pass (`docs/specs/fleetd/design.md`,
//! RR-4, RR-7, FM-11, and "Migration plan" item 3: "the fact scans of
//! 10,000 outboxes and worktree folders keep the heartbeat under 30 s and go
//! on at their cursors after a restart").
//!
//! Each test runs the `horch` binary with a real fleetd in stand-in mode
//! (`docs/tickets/fleetd-p1/MAP.md`, Q8) on the ledger of its harness. A
//! `sleep` child of the test is the agent of each worker, and only fleetd
//! sees that an agent died. The tests make their ledger rows in 2 ways:
//!
//! - The RR-4 and RR-7 tests run a plan of 3 tasks. The test binds each
//!   task to a worker with `horch_e2e::bind` (FR-5), then runs `finish` and
//!   `done` as the worker.
//! - The FM-11 test binds 1 task with `horch_e2e::bind`, then writes the
//!   supervisor process of the worker and the agent of the orchestrator in
//!   the ledger. The scan test writes its 10,000 rows of each kind in the
//!   ledger directly, with no `horch` command.

#![cfg(unix)]

use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::scans::outbox_dir;
use horch_core::procid;
use horch_core::runtime::fault::ABORT_EXIT_CODE;
use horch_core::taskledger::{util, Store};
use horch_e2e::harness::{Harness, DELIVERY_PROBE_FAULT};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

const SIGKILL: i32 = 9;

/// The fault points of the pass (`crates/horch-core/src/runtime/fault.rs`).
const ABORT_AFTER_COMMIT: &str = "abort-after-commit";
const DROP_WATCH_EVENTS: &str = "drop-watch-events";

/// The hang limit of the design: `horch fleetd check` restarts a fleetd
/// whose heartbeat is older than this, in awake time.
const HANG_LIMIT_MS: u64 = 30_000;

/// 1 pass each second, so that a test waits less.
const FAST_PASSES: &str = "pass_period_s = 1\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Run `horch` as the worker `worker`. It must succeed.
fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let o = h
        .horch(args)
        .env("HORCH_WORKER_ID", worker)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "horch {} as {worker}: {}",
        args.join(" "),
        text(&o)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

/// A harness with its own ledger file and a git repository, and the
/// `fleet.toml` text `config` in its fleet root.
fn world(name: &str, config: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    std::fs::create_dir_all(fleet_root(&h)).unwrap();
    std::fs::write(fleet_root(&h).join("fleet.toml"), config).unwrap();
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// The fleet root of the harness: the folder `fleet` next to its ledger
/// (MAP.md, Q4).
fn fleet_root(h: &Harness) -> PathBuf {
    h.root.join("fleet")
}

/// Poll `f` every 100 ms for at most `secs` seconds.
fn wait_until<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "after {secs} s: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ─── fleetd ─────────────────────────────────────────────────────────────

/// The PID and start time of the fleetd that the `fleetd` row of the ledger
/// names (design, "Ledger"). `None` while no row exists.
fn fleetd_row(db: &Path) -> Option<(u32, Option<u64>)> {
    let store = Store::open_read_only(db).ok()?;
    let (pid, started): (i64, i64) = store
        .conn()
        .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()))
}

/// Stop the process `pid` only while it is the one that started at `started`.
fn stop(pid: u32, started: Option<u64>) {
    if let Some(started) = started.or_else(|| procid::start_time(pid)) {
        procid::signal_same(pid, started, SIGKILL);
    }
}

/// Each fleetd of a test: the processes that the test started, and the one
/// that the `fleetd` row names. Drop stops them all, also when the test
/// fails.
struct Fleetds {
    db: PathBuf,
    children: Vec<Child>,
    seen: Vec<(u32, Option<u64>)>,
}

impl Fleetds {
    fn new(h: &Harness) -> Self {
        Fleetds {
            db: db(h),
            children: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// The fleetd that the `fleetd` row names, recorded for the teardown.
    fn current(&mut self) -> Option<(u32, Option<u64>)> {
        let row = fleetd_row(&self.db)?;
        if !self.seen.contains(&row) {
            self.seen.push(row);
        }
        Some(row)
    }

    /// `SIGKILL` the fleetd that the test started last, and reap it.
    fn kill_last(&mut self) {
        let child = self.children.last_mut().expect("a fleetd runs");
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        let _ = self.current();
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        for &(pid, started) in &self.seen {
            stop(pid, started);
        }
    }
}

/// `horch fleetd` in the sealed environment of `h`, with the fault points
/// `fault` armed on this process only.
fn spawn_fleetd(h: &Harness, fleetds: &mut Fleetds, fault: Option<&str>) -> u32 {
    let n = fleetds.children.len();
    let out = h.root.join(format!("fleetd-{n}.stdout"));
    let err = h.root.join(format!("fleetd-{n}.stderr"));
    let mut cmd = Command::new(Harness::horch_bin());
    h.seal(&mut cmd);
    if let Some(fault) = fault {
        cmd.env("HORCH_FAULT", format!("{DELIVERY_PROBE_FAULT},{fault}"));
    }
    let child = cmd
        .arg("fleetd")
        .stdin(Stdio::null())
        .stdout(File::create(out).unwrap())
        .stderr(File::create(err).unwrap())
        .spawn()
        .unwrap();
    let pid = child.id();
    fleetds.children.push(child);
    pid
}

/// The output of the fleetd that the test started as its `n`-th.
fn fleetd_output(h: &Harness, n: usize) -> String {
    let read = |ext: &str| {
        std::fs::read_to_string(h.root.join(format!("fleetd-{n}.{ext}"))).unwrap_or_default()
    };
    format!("stdout: {}\nstderr: {}", read("stdout"), read("stderr"))
}

/// Start `horch fleetd` with the fault points `fault`, and wait until the
/// `fleetd` row names it. Returns its PID.
fn start_fleetd(h: &Harness, fleetds: &mut Fleetds, fault: Option<&str>) -> u32 {
    let pid = spawn_fleetd(h, fleetds, fault);
    let n = fleetds.children.len() - 1;
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if fleetds.current().is_some_and(|(p, _)| p == pid) {
            return pid;
        }
        if let Some(status) = fleetds.children[n].try_wait().unwrap() {
            panic!(
                "horch fleetd exited at start: {status}\n{}",
                fleetd_output(h, n)
            );
        }
        assert!(
            Instant::now() < until,
            "horch fleetd (PID {pid}) runs, but after 30 s the fleetd row names {:?}\n{}",
            fleetd_row(&fleetds.db),
            fleetd_output(h, n)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The heartbeat of fleetd (MAP.md, Q20), when one is written.
fn heartbeat(h: &Harness) -> Option<Value> {
    let path = fleet_root(h).join("run").join("heartbeat.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The awake time of a heartbeat, in ms.
fn awake_ms(hb: &Value) -> u64 {
    hb["awake"]["awake_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("a heartbeat with no awake time: {hb}"))
}

/// Watch the heartbeat of the fleetd `pid` until it wrote `passes` more
/// heartbeats. Returns the largest awake time between 2 heartbeats, in ms.
fn watch_heartbeats(h: &Harness, pid: u32, passes: usize) -> u64 {
    let mut last: Option<u64> = None;
    let mut gaps = Vec::new();
    wait_until(
        &format!("fleetd PID {pid} wrote {passes} heartbeats"),
        120,
        || {
            let hb = heartbeat(h).filter(|hb| hb["pid"].as_u64() == Some(u64::from(pid)))?;
            let at = awake_ms(&hb);
            match last {
                Some(before) if at != before => {
                    gaps.push(at.saturating_sub(before));
                    last = Some(at);
                }
                None => last = Some(at),
                Some(_) => {}
            }
            (gaps.len() >= passes).then_some(())
        },
    );
    gaps.into_iter().max().unwrap_or(0)
}

/// Wait until the fleetd `pid` wrote 2 more heartbeats: at least 1 whole
/// pass ran after this call.
fn after_a_whole_pass(h: &Harness, pid: u32) {
    watch_heartbeats(h, pid, 2);
}

// ─── agents and workers ─────────────────────────────────────────────────

/// The agent of a worker: a `sleep` child of the test. Drop kills it, so a
/// failed assertion leaks no process.
struct Agent(Child);

impl Agent {
    fn start() -> Agent {
        Agent(Command::new("/bin/sleep").arg("600").spawn().unwrap())
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// `SIGKILL`, then reap it, so that no zombie looks alive.
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 1 worker of persona `opus` whose agent is a live [`Agent`].
struct Worker {
    id: String,
    agent: Agent,
}

/// A new worker of persona `opus` with a live [`Agent`], bound to the next
/// startable task (FR-5). Returns the worker and the seq of its task.
fn bind(h: &Harness) -> (Worker, i64) {
    let agent = Agent::start();
    let bound = horch_e2e::bind::bind(h, "opus", Some(agent.pid()));
    let seq = horch_core::taskledger::Store::open_read_only(&db(h))
        .and_then(|store| {
            Ok(store.conn().query_row(
                "SELECT seq FROM tasks WHERE task_id = ?1",
                [&bound.task_id],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let w = Worker {
        id: bound.worker_id,
        agent,
    };
    (w, seq)
}

/// The work of `w` on its bound task `seq`: its worktree, 1 commit, the
/// finish steps and `horch ledger done`. Then its agent exits, as an agent
/// does after `done`; only fleetd ends the attempt.
fn complete(h: &Harness, w: &mut Worker, seq: i64) {
    let name = format!("t{seq}");
    let out = ok_as(h, &w.id, &["ledger", "worktree", "new", &name]);
    let path = PathBuf::from(
        out.lines()
            .find_map(|l| l.strip_prefix("path: "))
            .unwrap_or_else(|| panic!("no worktree path in: {out}")),
    );
    let file = format!("{name}.txt");
    std::fs::write(path.join(&file), "work\n").unwrap();
    for args in [
        &["add", file.as_str()][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", &name],
    ] {
        let o = h.git_cmd(args).current_dir(&path).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }
    ok_as(h, &w.id, &["ledger", "finish"]);
    ok_as(h, &w.id, &["ledger", "done", &format!("{name} is done")]);
    w.agent.kill();
}

/// A run of `tasks` tasks for `opus`, whose check passes. The watchdog
/// numbers of the run never reap a quiet worker during a test: only a dead
/// agent ends an attempt. The run and its tasks are fixtures in the ledger
/// (`horch_e2e::ledger_seed`), since Phase 6 retired `horch ledger run new`.
fn plan(h: &Harness, tasks: usize) {
    let run = seed_run(h, "alpha", None, "w1:p1");
    Store::open(&db(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE runs SET finish_steps = '[\"check\"]', check_every_s = 3600,
                 stall_rounds = 1000
             WHERE run_id = ?1",
            [&run.run_id],
        )
        .unwrap();
    for n in 1..=tasks {
        let p = ticket(h, &format!("t{n}"), &format!("# T{n}\n**Check:** `true`\n"));
        seed_task(h, &run, &p, "opus");
    }
}

/// The status of the task `seq`.
fn task_status(h: &Harness, seq: i64) -> String {
    Store::open_read_only(&db(h))
        .unwrap()
        .conn()
        .query_row("SELECT status FROM tasks WHERE seq = ?1", [seq], |r| {
            r.get(0)
        })
        .unwrap()
}

fn live_attempts(h: &Harness) -> i64 {
    Store::open_read_only(&db(h))
        .unwrap()
        .conn()
        .query_row(
            "SELECT count(*) FROM attempts WHERE state <> 'ended'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

/// The end state of 1 task.
#[derive(Debug, PartialEq, Eq)]
struct TaskOutcome {
    seq: i64,
    status: String,
    /// The `attempts` count of the task row.
    attempts: i64,
    /// The state and the outcome of each attempt row of the task, in order.
    ends: Vec<(String, Option<String>)>,
}

fn outcomes(h: &Harness) -> Vec<TaskOutcome> {
    let store = Store::open_read_only(&db(h)).unwrap();
    let conn = store.conn();
    let mut tasks = conn
        .prepare("SELECT task_id, seq, status, attempts FROM tasks ORDER BY seq")
        .unwrap();
    let rows: Vec<(String, i64, String, i64)> = tasks
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    rows.into_iter()
        .map(|(id, seq, status, attempts)| {
            let mut stmt = conn
                .prepare("SELECT state, outcome FROM attempts WHERE task_id = ?1 ORDER BY id")
                .unwrap();
            let ends = stmt
                .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            TaskOutcome {
                seq,
                status,
                attempts,
                ends,
            }
        })
        .collect()
}

/// The 3 tasks of the plan, run to their end with a fleetd that has the
/// fault points `fault`. The agent of the worker of task 2 dies, so task 2
/// completes only when fleetd ends its attempt and queues it again. Only
/// fleetd ends the attempt of each task after its agent exits. With
/// `kill_fleetd`, fleetd gets `SIGKILL` after task 1 is done; the agent of
/// task 2 dies and task 3 is done while no fleetd runs; then fleetd starts
/// again.
fn run_plan(h: &Harness, fault: Option<&str>, kill_fleetd: bool) -> Vec<TaskOutcome> {
    let mut fleetds = Fleetds::new(h);
    let mut pid = start_fleetd(h, &mut fleetds, fault);
    plan(h, 3);
    let (mut w1, s1) = bind(h);
    let (mut w2, s2) = bind(h);
    let (mut w3, s3) = bind(h);
    assert_eq!([s1, s2, s3], [1, 2, 3], "each bind takes the next task");
    complete(h, &mut w1, 1);
    if kill_fleetd {
        fleetds.kill_last();
    }
    w2.agent.kill();
    complete(h, &mut w3, 3);
    if kill_fleetd {
        assert_eq!(
            task_status(h, 2),
            "running",
            "no fleetd runs, so nothing ends the attempt of the dead agent"
        );
        pid = start_fleetd(h, &mut fleetds, fault);
    }
    wait_until(
        "fleetd queues task 2 again after its agent died",
        30,
        || matches!(task_status(h, 2).as_str(), "queued" | "ready").then_some(()),
    );
    // The retry waits for its backoff of 30 s on the fleet clock (rung 6).
    h.advance_fleet_clock(30);
    let (mut w4, s4) = bind(h);
    assert_eq!(s4, 2, "the new worker is bound to task 2");
    complete(h, &mut w4, 2);
    wait_until("fleetd ends each attempt whose agent exited", 30, || {
        (live_attempts(h) == 0).then_some(())
    });
    after_a_whole_pass(h, pid);
    let last = fleetds.children.len() - 1;
    assert!(
        fleetds.children[last].try_wait().unwrap().is_none(),
        "fleetd exited during the run\n{}",
        fleetd_output(h, last)
    );
    outcomes(h)
}

/// The end state of the plan of [`run_plan`]: each task done once, task 2
/// after 1 more attempt.
fn plan_done() -> Vec<TaskOutcome> {
    let done = |seq: i64, ends: &[&str]| -> TaskOutcome {
        let attempts = ends
            .iter()
            .map(|o| ("ended".to_string(), Some((*o).to_string())))
            .collect();
        TaskOutcome {
            seq,
            status: "done".to_string(),
            attempts: ends.len() as i64,
            ends: attempts,
        }
    };
    vec![
        done(1, &["done"]),
        done(2, &["no_result", "done"]),
        done(3, &["done"]),
    ]
}

// ─── pass cursors ───────────────────────────────────────────────────────

/// 1 row of `pass_cursors`: `after_key`, `lap_started_at` and
/// `prev_lap_started_at`.
type CursorRow = (Option<String>, String, Option<String>);

fn cursor(h: &Harness, kind: &str) -> Option<CursorRow> {
    Store::open_read_only(&db(h))
        .unwrap()
        .conn()
        .query_row(
            "SELECT after_key, lap_started_at, prev_lap_started_at FROM pass_cursors WHERE kind = ?1",
            [kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
}

/// The scans of 10,000 folders of each kind.
const FOLDERS: usize = 10_000;

/// The kinds of `pass_cursors` of the 2 folder scans.
const FOLDER_SCANS: [&str; 2] = ["outboxes", "worktree_folders"];

/// `FOLDERS` tasks of the run of `h`, each `running` with 1 live attempt of
/// its own worker, whose agent is `agent`; and `FOLDERS` open worktrees.
/// Each attempt has its outbox and each worktree its folder, each with 1
/// small file, and no git work. The keys sort in the order of their number.
fn seed_folders(h: &Harness, agent: &Agent) {
    let root = fleet_root(h);
    let pid = agent.pid();
    let start = procid::start_time(pid).expect("the start time of the agent") as i64;
    let now = util::now();
    let project = h.project.to_string_lossy().into_owned();
    let mut store = Store::open(&db(h)).unwrap();
    store
        .immediate(|tx| {
            let (run, host): (String, String) = tx.query_row(
                "SELECT r.run_id, w.host FROM runs r
                 JOIN workers w ON w.worker_id = r.orchestrator_worker_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            // FR-12: each task belongs to an idea.
            let idea = horch_core::taskledger::schema::run_idea(tx, &run, &now, true)?
                .expect("a run that gets a task gets an idea");
            let mut task = tx.prepare(
                "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                     claimable_by, status, attempts, current_worker_id, created_at, updated_at,
                     idea_id)
                 VALUES (?1, ?2, ?3, ?1, 'tickets/scan.md', 'opus', 'opus', 'running', 1, ?4,
                     ?5, ?5, ?6)",
            )?;
            let mut worker = tx.prepare(
                "INSERT INTO workers (worker_id, run_id, kind, persona, host, agent_pid,
                     agent_started, task_id, attempt, status, spawned_at, started_at, claimed_at,
                     last_checkin_at)
                 VALUES (?1, ?2, 'worker', 'opus', ?3, ?4, ?5, ?6, 1, 'working', ?7, ?7, ?7, ?7)",
            )?;
            let mut attempt = tx.prepare(
                "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6)",
            )?;
            let mut tree = tx.prepare(
                "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch,
                     base, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'main', 'open', ?7)",
            )?;
            for n in 1..=FOLDERS {
                let task_id = format!("S-{n:05}");
                let worker_id = format!("SW-{n:05}");
                let attempt_id = format!("{task_id}/A1");
                task.execute((&task_id, &run, n as i64, &worker_id, &now, &idea))?;
                worker.execute((&worker_id, &run, &host, pid, start, &task_id, &now))?;
                attempt.execute((&attempt_id, &task_id, &worker_id, pid, start, &now))?;
                let outbox = outbox_dir(&root, &task_id, &attempt_id);
                std::fs::create_dir_all(&outbox)?;
                std::fs::write(outbox.join("progress.jsonl"), "")?;
                let folder = h.root.join("trees").join(format!("{n:05}"));
                std::fs::create_dir_all(&folder)?;
                std::fs::write(folder.join("README"), "tree\n")?;
                let path = folder.to_string_lossy();
                let branch = format!("scan-{n:05}");
                let id = format!("WT-{n:05}");
                tree.execute((&id, &task_id, &worker_id, &project, &path, &branch, &now))?;
            }
            Ok(())
        })
        .unwrap();
}

// ─── the tests ──────────────────────────────────────────────────────────

/// Wait until the `pass_cursors` row of each kind of [`FOLDER_SCANS`]
/// differs from its row in `before`: a pass stored each folder cursor again.
/// Returns the new rows.
fn cursors_moved(h: &Harness, before: &[Option<CursorRow>]) -> Vec<CursorRow> {
    wait_until("a pass moves the cursor of each folder scan", 120, || {
        let now: Vec<Option<CursorRow>> = FOLDER_SCANS.iter().map(|k| cursor(h, k)).collect();
        let moved = now.iter().zip(before).all(|(n, b)| n.is_some() && n != b);
        moved.then(|| now.into_iter().flatten().collect())
    })
}

/// The gate test of PL-5 and FM-12 for the fact scans: with 10,000 outboxes
/// and 10,000 worktree folders, each heartbeat comes less than 30 s of
/// awake time after the one before, and `horch fleetd check` finds the
/// heartbeat fresh. Each folder scan goes on after the key at its stored
/// cursor after a `SIGKILL` and a new start, in the same lap.
///
/// The kill comes after 3 passes that each moved each cursor, not after a
/// fixed time, so the test does not depend on the speed of the host. Then
/// the test moves each stored cursor to the key of the middle folder, in the
/// same lap. The first pass after the new start must move each cursor past
/// that key. A scan budget of 1 ms reads far fewer than the 5,000 keys before
/// the middle, so a scan that starts again at the first folder cannot pass.
#[test]
fn scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart()
{
    horch_e2e::expected::check(file!(), "scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart", || {
    let h = world("p1-scan", &format!("{FAST_PASSES}scan_budget_ms = 1\n"));
    plan(&h, 0);
    let agent = Agent::start();
    seed_folders(&h, &agent);
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&h, &mut fleetds, None);

    let mut before = vec![None; FOLDER_SCANS.len()];
    for _ in 0..3 {
        before = cursors_moved(&h, &before).into_iter().map(Some).collect();
    }
    fleetds.kill_last();
    let middle = FOLDERS / 2;
    let middle_keys = [format!("S-{middle:05}/A1"), format!("WT-{middle:05}")];
    let mut stored = Vec::new();
    for (kind, key) in FOLDER_SCANS.iter().zip(middle_keys) {
        let (after, lap, prev) = cursor(&h, kind).unwrap_or_else(|| panic!("no cursor {kind}"));
        let after = after.unwrap_or_else(|| panic!("the scan of {kind} read no folder"));
        assert!(
            prev.is_none(),
            "the scan of {kind} ended a lap of {FOLDERS} folders in 3 passes"
        );
        assert!(
            after < key,
            "the scan of {kind} is at {after} after 3 passes, past the middle key {key}"
        );
        Store::open(&db(&h))
            .unwrap()
            .conn()
            .execute(
                "UPDATE pass_cursors SET after_key = ?1 WHERE kind = ?2",
                [&key, *kind],
            )
            .unwrap();
        stored.push((Some(key), lap, prev));
    }

    let second = start_fleetd(&h, &mut fleetds, None);
    let first_pass = cursors_moved(&h, &stored.iter().cloned().map(Some).collect::<Vec<_>>());
    for ((kind, (after, lap, _)), (now_after, now_lap, now_prev)) in
        FOLDER_SCANS.iter().zip(&stored).zip(first_pass)
    {
        assert_eq!(
            (&now_lap, &now_prev),
            (lap, &None),
            "the scan of {kind} started a new lap after the restart, not at its cursor {after:?}"
        );
        assert!(
            now_after > *after,
            "the scan of {kind} is at {now_after:?}, not after its cursor {after:?}"
        );
    }

    let gap = watch_heartbeats(&h, second, 4);
    assert!(
        gap < HANG_LIMIT_MS,
        "2 heartbeats are {gap} ms of awake time apart with {FOLDERS} folders of each kind"
    );
    let check = ok(&h, &["fleetd", "check"]);
    assert!(
        check.contains(&format!(
            "fleetd PID {second} runs, and its heartbeat is fresh"
        )),
        "{check}"
    );
    });
}

/// RR-4: the watcher reports no file event, and the plan still ends with
/// each task done. The pass every second finds the dead agent and queues
/// its task again, and the scans read the outboxes and worktree folders at
/// their cursors.
#[test]
fn rr4_tasks_complete_with_all_file_watch_events_dropped() {
    horch_e2e::expected::check(
        file!(),
        "rr4_tasks_complete_with_all_file_watch_events_dropped",
        || {
            let h = world("p1-rr4", FAST_PASSES);
            let end = run_plan(&h, Some(DROP_WATCH_EVENTS), false);
            assert_eq!(end, plan_done());
            for kind in FOLDER_SCANS {
                let after = cursor(&h, kind).and_then(|(after, _, _)| after);
                assert!(
                    after.is_some(),
                    "with no file event, the scan of {kind} read no folder"
                );
            }
        },
    );
}

/// The awake time of the last heartbeat of the fleetd `pid`, after no new
/// heartbeat came for 2 s: the passes that an earlier event woke ended.
fn settled_heartbeat(h: &Harness, pid: u32) -> u64 {
    let mut last: Option<(u64, Instant)> = None;
    wait_until(
        &format!("the heartbeat of fleetd PID {pid} settles"),
        30,
        || {
            let hb = heartbeat(h).filter(|hb| hb["pid"].as_u64() == Some(u64::from(pid)))?;
            let at = awake_ms(&hb);
            match last {
                Some((before, since)) if before == at => {
                    (since.elapsed() >= Duration::from_secs(2)).then_some(at)
                }
                _ => {
                    last = Some((at, Instant::now()));
                    None
                }
            }
        },
    )
}

/// Wait at most 3 s for a heartbeat of the fleetd `pid` after the one at
/// `before`: `event` woke a pass long before the period of 30 s.
fn woke_by(h: &Harness, pid: u32, before: u64, event: &str) {
    wait_until(
        &format!("{event} wakes a pass of fleetd PID {pid} before its period of 30 s"),
        3,
        || {
            let hb = heartbeat(h).filter(|hb| hb["pid"].as_u64() == Some(u64::from(pid)))?;
            (awake_ms(&hb) != before).then_some(())
        },
    );
}

/// RR-4: a file event wakes a pass at once; the period of 30 s is only the
/// fallback. A request file, a `result.json` in the outbox of a live
/// attempt and a job result file each start a pass within 3 s.
#[test]
fn rr4_file_request_and_job_events_wake_pass_before_period() {
    let h = world("p1-rr4-wake", "pass_period_s = 30\n");
    plan(&h, 1);
    let run = fleet_root(&h).join("run");
    for dir in ["requests", "jobs"] {
        std::fs::create_dir_all(run.join(dir)).unwrap();
    }
    let mut fleetds = Fleetds::new(&h);
    let pid = start_fleetd(&h, &mut fleetds, None);
    let (_w, seq) = bind(&h);
    let (task, attempt): (String, String) = Store::open_read_only(&db(&h))
        .unwrap()
        .conn()
        .query_row(
            "SELECT a.task_id, a.id FROM attempts a JOIN tasks t ON t.task_id = a.task_id
             WHERE t.seq = ?1",
            [seq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let outbox = outbox_dir(&fleet_root(&h), &task, &attempt);
    let before = settled_heartbeat(&h, pid);
    std::fs::create_dir_all(&outbox).unwrap();
    // A pass after the outbox exists watches it: an event or the period.
    wait_until("a pass after the outbox exists", 40, || {
        let hb = heartbeat(&h).filter(|hb| hb["pid"].as_u64() == Some(u64::from(pid)))?;
        (awake_ms(&hb) != before).then_some(())
    });
    let before = settled_heartbeat(&h, pid);
    std::fs::write(
        run.join("requests").join("wake.json"),
        "{\"kind\": \"wake\"}\n",
    )
    .unwrap();
    woke_by(&h, pid, before, "a request file");

    let before = settled_heartbeat(&h, pid);
    std::fs::write(outbox.join("result.json"), "{}\n").unwrap();
    woke_by(&h, pid, before, "a result.json in the outbox");

    let before = settled_heartbeat(&h, pid);
    std::fs::write(run.join("jobs").join("J-1-1.json"), "{}\n").unwrap();
    woke_by(&h, pid, before, "a job result file");
}

/// RR-7: the same plan, once with 1 fleetd and once with fleetd killed by
/// `SIGKILL` in the middle and started again, ends in the same state: each
/// task done, with the same attempts. No task is lost or done 2 times.
#[test]
fn rr7_fleetd_killed_mid_run_ends_as_a_clean_run() {
    horch_e2e::expected::check(
        file!(),
        "rr7_fleetd_killed_mid_run_ends_as_a_clean_run",
        || {
            let clean = run_plan(&world("p1-rr7-clean", FAST_PASSES), None, false);
            let killed = run_plan(&world("p1-rr7-kill", FAST_PASSES), None, true);
            assert_eq!(clean, plan_done(), "the clean run");
            assert_eq!(killed, clean, "the run with fleetd killed mid-run");
        },
    );
}

/// A process that appends 1 line to `file` for each polite stop that it
/// gets, then exits. It leads its own process group. Drop kills it.
struct Polite(Child);

impl Polite {
    /// Start it, and wait until its shell has set the trap of the polite
    /// stop: a stop before the trap would end it with no line.
    fn start(file: &Path) -> Polite {
        let ready = file.with_extension("ready");
        let polite = Polite(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(
                    r#"trap 'echo stop >> "$STOPS"; exit 0' TERM; : > "$READY"; while :; do sleep 0.1; done"#,
                )
                .env("STOPS", file)
                .env("READY", &ready)
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        wait_until("the polite process set its trap", 30, || {
            ready.exists().then_some(())
        });
        polite
    }

    fn exited(&mut self) -> bool {
        self.0.try_wait().unwrap().is_some()
    }
}

impl Drop for Polite {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn lines(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |t| t.lines().count())
}

/// The count of events that move `entity` `id` from `from` to `to`.
fn transitions(h: &Harness, entity: &str, id: &str, from: &str, to: &str) -> i64 {
    Store::open_read_only(&db(h))
        .unwrap()
        .conn()
        .query_row(
            "SELECT count(*) FROM events
             WHERE entity = ?1 AND entity_id = ?2 AND from_state = ?3 AND to_state = ?4",
            [entity, id, from, to],
            |r| r.get(0),
        )
        .unwrap()
}

/// The attempts of orchestrators: the respawns, which have no task
/// (MAP.md, Q6).
fn orchestrator_attempts(h: &Harness) -> Vec<String> {
    let store = Store::open_read_only(&db(h)).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT id FROM attempts WHERE task_id IS NULL ORDER BY id")
        .unwrap();
    let ids = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    ids
}

/// The launches of an orchestrator respawn: each `pane run` in the fake
/// herdr of a command with `HORCH_ORCHESTRATOR_RESPAWN`.
fn respawn_launches(h: &Harness) -> usize {
    h.calls_of("herdr")
        .iter()
        .filter(|c| c["argv"][0] == "pane" && c["argv"][1] == "run")
        .filter(|c| {
            c["argv"][3]
                .as_str()
                .is_some_and(|cmd| cmd.contains("HORCH_ORCHESTRATOR_RESPAWN"))
        })
        .count()
}

/// The harness of the FM-11 test: the fake herdr has the workspace `w1`,
/// and each `horch` command runs in it, so the run records `w1` as the
/// workspace of its orchestrator.
fn fleet_world(name: &str) -> Harness {
    let mut h = world(name, FAST_PASSES);
    let quota = horch_e2e::harness::fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    let mut create = Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

/// FM-11: fleetd dies after the commit of part 2 and before its effects.
/// The pass ended the attempt of a dead agent and queued its task again,
/// and wrote the respawn of a dead orchestrator as an attempt in
/// `launching`. It did not stop the supervisor process of the worker and
/// did not launch the orchestrator. The next fleetd does each effect once:
/// 1 stop of the supervisor and 1 launch of the orchestrator. It does not
/// end the attempt, queue the task or write a respawn a second time.
#[test]
fn fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart() {
    horch_e2e::expected::check(
        file!(),
        "fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart",
        || {
            let h = fleet_world("p1-fm11");
            plan(&h, 1);
            let (mut w, seq) = bind(&h);
            assert_eq!(seq, 1);
            let stops = h.root.join("stops.txt");
            let mut supervisor = Polite::start(&stops);
            let pid = supervisor.0.id();
            let start = procid::start_time(pid).expect("the start time of the supervisor") as i64;
            let mut gone = Agent::start();
            let orch_pid = gone.pid();
            let orch_start =
                procid::start_time(orch_pid).expect("the start time of the agent") as i64;
            gone.kill();
            let store = Store::open(&db(&h)).unwrap();
            store
        .conn()
        .execute(
            "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3 WHERE worker_id = ?1",
            (&w.id, pid, start),
        )
        .unwrap();
            // The orchestrator of the run registered, and its agent is gone.
            store
                .conn()
                .execute(
                    "UPDATE workers SET agent_pid = ?1, agent_started = ?2
             WHERE worker_id = (SELECT orchestrator_worker_id FROM runs)",
                    (orch_pid, orch_start),
                )
                .unwrap();
            let (task, attempt): (String, String) = store
                .conn()
                .query_row(
                    "SELECT task_id, id FROM attempts WHERE task_id IS NOT NULL",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            drop(store);
            w.agent.kill();

            let mut fleetds = Fleetds::new(&h);
            spawn_fleetd(&h, &mut fleetds, Some(ABORT_AFTER_COMMIT));
            let status = wait_until("the fleetd with the fault exits", 30, || {
                fleetds.children[0].try_wait().unwrap()
            });
            let _ = fleetds.current();
            assert_eq!(
                status.code(),
                Some(ABORT_EXIT_CODE),
                "{}",
                fleetd_output(&h, 0)
            );
            assert_eq!(
                (
                    task_status(&h, 1),
                    transitions(&h, "attempt", &attempt, "live", "ended")
                ),
                ("ready".to_string(), 1),
                "the pass committed the end of the attempt and the requeue before it died"
            );
            let respawn = orchestrator_attempts(&h);
            assert_eq!(
                respawn.len(),
                1,
                "the pass committed 1 respawn before it died"
            );
            std::thread::sleep(Duration::from_millis(500));
            assert_eq!(lines(&stops), 0, "no stop before the restart");
            assert!(
                !supervisor.exited(),
                "the supervisor runs until the restart"
            );
            assert_eq!(respawn_launches(&h), 0, "no launch before the restart");

            let fleetd = start_fleetd(&h, &mut fleetds, None);
            wait_until("the restarted fleetd stops the supervisor", 30, || {
                supervisor.exited().then_some(())
            });
            wait_until("the restarted fleetd launches the respawn", 30, || {
                (respawn_launches(&h) > 0).then_some(())
            });
            for _ in 0..3 {
                after_a_whole_pass(&h, fleetd);
            }
            assert_eq!(lines(&stops), 1, "1 stop of the supervisor");
            assert_eq!(respawn_launches(&h), 1, "1 launch of the respawn");
            assert_eq!(
                orchestrator_attempts(&h),
                respawn,
                "the respawn has 1 attempt"
            );
            assert_eq!(
                transitions(&h, "attempt", &attempt, "live", "ended"),
                1,
                "the attempt ended once"
            );
            assert_eq!(
                transitions(&h, "task", &task, "running", "ready"),
                1,
                "the task was queued again once"
            );
            assert_eq!(
                (task_status(&h, 1), outcomes(&h)[0].attempts),
                ("ready".to_string(), 1),
                "the task waits for its next attempt"
            );
        },
    );
}
