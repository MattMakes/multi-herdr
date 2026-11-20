//! The release test of Phase 5 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 5; Q3 of
//! `docs/tickets/fleetd-p5/MAP.md`; FM-24).
//!
//! An old worker on Phase 4 runs `horch ledger progress` and
//! `horch ledger done` after the update. Each one has 1 effect, and only
//! fleetd writes the ledger. The run is not stopped.
//!
//! The Phase 4 ledger is built by the real migration steps up to
//! [`schema::PHASE_4_VERSION`], then the rows of a run in flight are
//! written as a Phase 4 binary left them:
//! - a live claim attempt with no outbox, and a live push attempt with its
//!   task folder and its outbox;
//! - a task whose attempt ended;
//! - the `fleetd` row of the Phase 4 fleetd.
//!
//! Each old worker is a stand-in: a `sh` process, recorded as the wrapper
//! and the agent of its attempt. It waits on its stdin, then runs the 2
//! commands as its children, so its attempt is in their process chain.
//! The old fleetd is a stand-in too: a `sleep` process that the `fleetd`
//! row names.
//!
//! The update: the test stops the old fleetd, migrates the ledger to
//! [`schema::PHASE_5_VERSION`] and starts `horch fleetd` of this branch.
//! Its first pass makes the outbox of the claim attempt (Q3). The test then
//! stops fleetd with `SIGSTOP`, so no pass runs while the old worker runs
//! its commands: the ledger has no row of them. After `SIGCONT`, fleetd
//! ingests the files, and each command has 1 effect, also after 2 more
//! passes.
//!
//! Each `horch` process other than fleetd runs with
//! `HORCH_TEST_ASSERT_READ_ONLY=1`: a writable open of the ledger panics,
//! and the command fails (ticket 42).

#![cfg(unix)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use horch_core::procid;
use horch_core::taskledger::model::TaskStatus;
use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::schema;
use horch_core::taskledger::store::ASSERT_READ_ONLY_ENV;
use horch_core::taskledger::util;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::Connection;
use serde_json::Value;

/// The fleet clock of the Phase 4 `fleetd` row at the update.
const CLOCK: i64 = 5000;
/// The lease of each live attempt ends 1 day of the fleet clock after the
/// update: no lease ends in a test.
const LEASE_END: i64 = CLOCK + 86_400;

/// The claim attempt, its task and its worker.
const CLAIM_TASK: &str = "T-1";
const CLAIM_ATTEMPT: &str = "T-1/A1";
const CLAIM_WORKER: &str = "w-claim";
/// The push attempt, its task and its worker.
const PUSH_TASK: &str = "T-2";
const PUSH_ATTEMPT: &str = "T-2/A1";
const PUSH_WORKER: &str = "w-push";
/// The task whose attempt ended before the update, the attempt and its
/// worker.
const ENDED_TASK: &str = "T-3";
const ENDED_ATTEMPT: &str = "T-3/A1";
const ENDED_WORKER: &str = "w-ended";

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The script of an old worker. `$0` is its name, `$1` the `horch`
/// binary, `$2` the text of `progress`, `$3` the summary of `done` and `$4`
/// the folder of the outputs. It waits for 1 line, runs the 2 commands,
/// writes the exit code of each, then waits for the end of the test.
const OLD_WORKER: &str = r#"read go
"$1" ledger progress "$2" >"$4/progress.out" 2>&1
echo $? >"$4/progress.code"
"$1" ledger done "$3" >"$4/done.out" 2>&1
echo $? >"$4/done.code"
read end
"#;

/// A harness with its own ledger, fleet root, data folder and git
/// repository, no watchdog, and the read-only seam for each process.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let root = fleet_root(&h);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("FLEET_ROOT", root.to_string_lossy());
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(ASSERT_READ_ONLY_ENV, "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn fleet_root(h: &Harness) -> PathBuf {
    h.root.join("fleet")
}

/// The folder of attempt `A1` of `task`.
fn attempt_dir(h: &Harness, task: &str) -> PathBuf {
    fleet_root(h)
        .join("tasks")
        .join(task)
        .join("attempts")
        .join("A1")
}

fn outbox(h: &Harness, task: &str) -> PathBuf {
    attempt_dir(h, task).join("outbox")
}

/// A read connection of the test to the ledger. The test process has no
/// seam: it reads only.
fn read(h: &Harness) -> Connection {
    Connection::open(db(h)).unwrap()
}

/// A `sleep` process: the old fleetd. Killed at the end of the test.
struct OldFleetd {
    child: Child,
    started: u64,
}

impl OldFleetd {
    fn start() -> OldFleetd {
        let child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let started = procid::start_time(child.id()).expect("the start time of sleep");
        OldFleetd { child, started }
    }

    /// Stop it, and wait until it is gone.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for OldFleetd {
    fn drop(&mut self) {
        self.stop();
    }
}

/// An old worker: the `sh` process of [`OLD_WORKER`], started before the
/// update. Killed at the end of the test.
struct OldWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    started: u64,
    /// The folder of its outputs.
    out: PathBuf,
}

impl OldWorker {
    fn start(h: &Harness, name: &str, progress: &str, summary: &str) -> OldWorker {
        let out = h.root.join(name);
        std::fs::create_dir_all(&out).unwrap();
        let mut cmd = Command::new("/bin/sh");
        h.seal(&mut cmd);
        // The environment of a worker of Phase 4. The outbox writer reads
        // none of it: it finds the attempt by the process chain.
        cmd.env("HORCH_WORKER_ID", name)
            .env("HORCH_ROLE", name)
            .arg("-c")
            .arg(OLD_WORKER)
            .arg("old-worker")
            .arg(Harness::horch_bin())
            .arg(progress)
            .arg(summary)
            .arg(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take();
        let started = procid::start_time(child.id()).expect("the start time of sh");
        OldWorker {
            child,
            stdin,
            started,
            out,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid(), Some(self.started))
    }

    /// Run `progress` and `done`, and wait until both ended. Each must
    /// pass.
    fn run_commands(&mut self) {
        let stdin = self.stdin.as_mut().expect("the stdin of the old worker");
        stdin.write_all(b"go\n").unwrap();
        stdin.flush().unwrap();
        for name in ["progress", "done"] {
            let code = wait_for_or(
                &format!("the old worker to run {name}"),
                || {
                    let text =
                        std::fs::read_to_string(self.out.join(format!("{name}.code"))).ok()?;
                    text.trim().parse::<i32>().ok()
                },
                || {
                    std::fs::read_to_string(self.out.join(format!("{name}.out")))
                        .unwrap_or_default()
                },
            );
            let output =
                std::fs::read_to_string(self.out.join(format!("{name}.out"))).unwrap_or_default();
            assert_eq!(code, 0, "horch ledger {name}: {output}");
        }
    }
}

impl Drop for OldWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The run in flight on Phase 4: the ledger, the old fleetd, the 2 old
/// workers and fleetd of this branch after the update.
struct Flight {
    fleetds: Fleetds,
    claim: OldWorker,
    push: OldWorker,
    old_fleetd: OldFleetd,
    h: Harness,
}

/// The progress text and the done summary of the old worker of `task`.
fn texts(task: &str) -> (String, String) {
    (
        format!("{task}: the old worker reports progress after the update"),
        format!("{task}: the old worker is done after the update"),
    )
}

impl Flight {
    /// The Phase 4 run in `h`, with its 2 old workers and the old fleetd.
    fn on_phase_4(name: &str) -> Flight {
        let h = world(name);
        let old_fleetd = OldFleetd::start();
        let (p, d) = texts(CLAIM_TASK);
        let claim = OldWorker::start(&h, CLAIM_WORKER, &p, &d);
        let (p, d) = texts(PUSH_TASK);
        let push = OldWorker::start(&h, PUSH_WORKER, &p, &d);
        phase_4_ledger(&h, &old_fleetd, &claim, &push);
        phase_4_push_folder(&h);
        let fleetds = Fleetds::new(&h);
        Flight {
            h,
            old_fleetd,
            claim,
            push,
            fleetds,
        }
    }

    /// The update: stop the old fleetd, migrate the ledger to the next
    /// schema version and start fleetd of this branch. Wait for its first
    /// pass: it makes the outbox of the claim attempt (Q3). Gives the PID
    /// of fleetd.
    fn update(&mut self) -> u32 {
        self.old_fleetd.stop();
        let mut conn = Connection::open(db(&self.h)).unwrap();
        schema::migrate_to(&mut conn, schema::PHASE_5_VERSION).unwrap();
        drop(conn);
        let pid = self.fleetds.start(&self.h, "fleetd");
        let claim_outbox = outbox(&self.h, CLAIM_TASK);
        wait_for_or(
            "the first pass to make the outbox of the claim attempt",
            || claim_outbox.is_dir().then_some(()),
            || self.fleetds.output("fleetd", "stderr"),
        );
        pid
    }

    /// The `at` of the heartbeat of fleetd: it changes at the end of each
    /// pass.
    fn heartbeat_at(&self) -> Option<String> {
        let path = fleet_root(&self.h).join("run").join("heartbeat.json");
        let hb: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        hb["at"].as_str().map(str::to_string)
    }

    /// Wait for the end of `n` passes of fleetd after this call.
    fn wait_passes(&self, n: usize) {
        for _ in 0..n {
            let before = self.heartbeat_at();
            wait_for_or(
                "1 more pass of fleetd",
                || {
                    let now = self.heartbeat_at();
                    (now.is_some() && now != before).then_some(())
                },
                || self.fleetds.output("fleetd", "stderr"),
            );
        }
    }
}

/// Send `signal` to fleetd `pid`.
fn signal(pid: u32, signal: i32) {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill(2) with a PID of a child of this test and a signal
    // number; it reads no memory of this process.
    let r = unsafe { libc::kill(pid, signal) };
    assert_eq!(r, 0, "kill {signal} {pid}");
}

/// The rows of the Phase 4 ledger of `h`: run `r1` of its project.
/// - [`CLAIM_TASK`] is `running` with the live claim attempt
///   [`CLAIM_ATTEMPT`] of `claim`.
/// - [`PUSH_TASK`] is `running` with the live push attempt [`PUSH_ATTEMPT`]
///   of `push`.
/// - [`ENDED_TASK`] is held: its attempt [`ENDED_ATTEMPT`] ended, and its
///   worker is stopped.
/// - The `fleetd` row names `old` with the fleet clock at [`CLOCK`].
fn phase_4_ledger(h: &Harness, old: &OldFleetd, claim: &OldWorker, push: &OldWorker) {
    let mut conn = Connection::open(db(h)).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_4_VERSION).unwrap();
    let now = util::now();
    let host = util::host_name();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, ?1, ?2, ?3, ?4, ?3)",
        rusqlite::params![old.child.id(), old.started, &now, CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(&h.project), &now),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r1', 'p', 'zeta', 'active', ?1)",
        [&now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                status, held_reason, attempts, current_worker_id, created_at, updated_at)
            VALUES (?1, 'r1', 1, 'T1', 'tickets/t1.md', 'opus', 'opus', 'running', NULL, 1, ?2, ?7, ?7),
                   (?3, 'r1', 2, 'T2', 'tickets/t2.md', 'opus', 'opus', 'running', NULL, 1, ?4, ?7, ?7),
                   (?5, 'r1', 3, 'T3', 'tickets/t3.md', 'opus', 'opus', 'escalated', 'its worker died', 1, ?6, ?7, ?7)",
        (
            CLAIM_TASK,
            CLAIM_WORKER,
            PUSH_TASK,
            PUSH_WORKER,
            ENDED_TASK,
            ENDED_WORKER,
            &now,
        ),
    )
    .unwrap();
    for (worker, task, agent) in [
        (CLAIM_WORKER, CLAIM_TASK, claim),
        (PUSH_WORKER, PUSH_TASK, push),
    ] {
        conn.execute(
            "INSERT INTO workers (worker_id, run_id, kind, persona, host, agent_pid, agent_started,
                    last_checkin_at, task_id, attempt, status, spawned_at, started_at, claimed_at)
                VALUES (?1, 'r1', 'worker', 'opus', ?2, ?3, ?4, ?5, ?6, 1, 'working', ?5, ?5, ?5)",
            rusqlite::params![worker, &host, agent.pid(), agent.started, &now, task],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, host, last_checkin_at, task_id,
                attempt, status, spawned_at, started_at, claimed_at)
            VALUES (?1, 'r1', 'worker', 'opus', ?2, ?3, ?4, 1, 'stopped', ?3, ?3, ?3)",
        (ENDED_WORKER, &host, &now, ENDED_TASK),
    )
    .unwrap();
    for (id, task, worker, agent, via) in [
        (CLAIM_ATTEMPT, CLAIM_TASK, CLAIM_WORKER, claim, "claim"),
        (PUSH_ATTEMPT, PUSH_TASK, PUSH_WORKER, push, "push"),
    ] {
        conn.execute(
            "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at,
                    active_at, via)
                VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, ?7, ?8)",
            rusqlite::params![
                id,
                task,
                worker,
                agent.pid(),
                agent.started,
                &now,
                CLOCK,
                via
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
                VALUES (?1, ?2, ?3, ?4, 'claim')",
            (task, id, LEASE_END, &now),
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, state, started_at, ended_at, outcome, via)
            VALUES (?1, ?2, ?3, 'ended', ?4, ?4, 'crashed', 'push')",
        (ENDED_ATTEMPT, ENDED_TASK, ENDED_WORKER, &now),
    )
    .unwrap();
}

/// The task folder and the outbox of the push attempt, as `horch spawn`
/// of Phase 4 made them: `task.yaml`, `START.md` and an empty outbox with
/// mode 0700.
fn phase_4_push_folder(h: &Harness) {
    use std::os::unix::fs::PermissionsExt;
    let task_dir = fleet_root(h).join("tasks").join(PUSH_TASK);
    let outbox = outbox(h, PUSH_TASK);
    std::fs::create_dir_all(&outbox).unwrap();
    for dir in [
        fleet_root(h).join("tasks"),
        task_dir.clone(),
        task_dir.join("attempts"),
        attempt_dir(h, PUSH_TASK),
        outbox,
    ] {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        task_dir.join("task.yaml"),
        format!("task: {PUSH_TASK}\nattempt: {PUSH_ATTEMPT}\n"),
    )
    .unwrap();
    std::fs::write(task_dir.join("START.md"), "# T2\n").unwrap();
}

/// The events of `kind` of `task` with the text `text`.
fn events(conn: &Connection, task: &str, kind: &str, text: &str) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM events WHERE task_id = ?1 AND kind = ?2 AND text = ?3",
        (task, kind, text),
        |r| r.get(0),
    )
    .unwrap()
}

/// The `ingested` rows of the file `name` in the outbox of `task`, as
/// `outcome` each.
fn ingested(conn: &Connection, task: &str, name: &str) -> Vec<String> {
    let pattern = format!("%/tasks/{task}/attempts/A1/outbox/{name}");
    let mut stmt = conn
        .prepare("SELECT outcome FROM ingested WHERE path LIKE ?1 ORDER BY path")
        .unwrap();
    let rows = stmt
        .query_map([pattern], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows
}

fn task_status(conn: &Connection, task: &str) -> String {
    conn.query_row("SELECT status FROM tasks WHERE task_id = ?1", [task], |r| {
        r.get(0)
    })
    .unwrap()
}

/// The moves of `task` from `running` to `verifying`, from its events. An
/// event shows the words that a reader sees.
fn moves_to_verifying(conn: &Connection, task: &str) -> i64 {
    let pattern = format!(
        "%task {task}: {} -> {}",
        TaskStatus::Running.shown(),
        TaskStatus::Verifying.shown()
    );
    conn.query_row(
        "SELECT count(*) FROM events WHERE task_id = ?1 AND kind = 'transition' AND text LIKE ?2",
        (task, pattern),
        |r| r.get(0),
    )
    .unwrap()
}

/// Each event of `task`, for the message of a failed assertion.
fn seen(conn: &Connection, task: &str) -> String {
    let mut stmt = conn
        .prepare("SELECT actor || ' ' || kind || ' ' || text FROM events WHERE task_id = ?1 ORDER BY event_id")
        .unwrap();
    let rows = stmt
        .query_map([task], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows.join("\n")
}

/// The old worker of `task` runs `progress` and `done` after the update.
/// Before a pass of fleetd, the ledger has no row of them. After it, each
/// one has 1 effect, also after 2 more passes.
fn old_worker_runs_progress_and_done_after_the_update(name: &str, task: &str) {
    let mut f = Flight::on_phase_4(name);
    let pid = f.update();
    let (progress, summary) = texts(task);

    // No pass runs while the old worker runs its commands.
    signal(pid, libc::SIGSTOP);
    let worker = if task == CLAIM_TASK {
        &mut f.claim
    } else {
        &mut f.push
    };
    worker.run_commands();
    let conn = read(&f.h);
    assert!(
        outbox(&f.h, task).join("progress.jsonl").is_file(),
        "progress wrote its line"
    );
    let result: Value =
        serde_json::from_slice(&std::fs::read(outbox(&f.h, task).join("result.json")).unwrap())
            .unwrap();
    assert_eq!(
        result["summary"],
        summary.as_str(),
        "done wrote result.json"
    );
    assert_eq!(
        events(&conn, task, "progress", &progress),
        0,
        "{}",
        seen(&conn, task)
    );
    assert!(ingested(&conn, task, "result.json").is_empty());
    assert!(ingested(&conn, task, "progress.jsonl").is_empty());
    assert_eq!(task_status(&conn, task), TaskStatus::Running.as_str());
    signal(pid, libc::SIGCONT);

    wait_for_or(
        "fleetd to ingest the progress line and result.json",
        || {
            let done = events(&conn, task, "progress", &progress) > 0
                && !ingested(&conn, task, "result.json").is_empty();
            done.then_some(())
        },
        || {
            format!(
                "{}\n{}",
                seen(&conn, task),
                f.fleetds.output("fleetd", "stderr")
            )
        },
    );
    f.wait_passes(2);
    let log = seen(&conn, task);
    assert_eq!(events(&conn, task, "progress", &progress), 1, "{log}");
    assert_eq!(ingested(&conn, task, "result.json"), ["accepted"], "{log}");
    assert_eq!(
        ingested(&conn, task, "progress.jsonl"),
        ["accepted"],
        "{log}"
    );
    assert_eq!(
        task_status(&conn, task),
        TaskStatus::Verifying.as_str(),
        "{log}"
    );
    assert_eq!(moves_to_verifying(&conn, task), 1, "{log}");
}

#[test]
fn old_claim_worker_runs_progress_and_done_after_the_update_with_1_effect_each() {
    old_worker_runs_progress_and_done_after_the_update("p5-rel-claim", CLAIM_TASK);
}

#[test]
fn old_push_worker_runs_progress_and_done_after_the_update_with_1_effect_each() {
    old_worker_runs_progress_and_done_after_the_update("p5-rel-push", PUSH_TASK);
}

/// The state of each attempt, `id state`, in id order.
fn attempt_states(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT id || ' ' || state FROM attempts ORDER BY id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows
}

/// The status of each task, `id status`, in id order.
fn task_states(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT task_id || ' ' || status FROM tasks ORDER BY task_id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows
}

#[test]
fn update_stops_no_run() {
    let mut f = Flight::on_phase_4("p5-rel-run");
    let conn = read(&f.h);
    let attempts = attempt_states(&conn);
    let tasks = task_states(&conn);
    assert_eq!(
        attempts,
        [
            format!("{CLAIM_ATTEMPT} live"),
            format!("{PUSH_ATTEMPT} live"),
            format!("{ENDED_ATTEMPT} ended"),
        ]
    );

    f.update();
    f.wait_passes(2);
    let log = [CLAIM_TASK, PUSH_TASK, ENDED_TASK]
        .map(|t| seen(&conn, t))
        .join("\n");
    let run: String = conn
        .query_row("SELECT status FROM runs WHERE run_id = 'r1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(run, "active", "{log}");
    assert_eq!(attempt_states(&conn), attempts, "{log}");
    assert_eq!(task_states(&conn), tasks, "{log}");
    assert!(f.claim.alive(), "the old claim worker goes on\n{log}");
    assert!(f.push.alive(), "the old push worker goes on\n{log}");
    let stopped: i64 = conn
        .query_row("SELECT count(*) FROM polite_stops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stopped, 0, "fleetd stops no process\n{log}");
}
