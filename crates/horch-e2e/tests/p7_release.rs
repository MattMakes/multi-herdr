//! The release test of Phase 7 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 7; R1 of
//! `docs/tickets/fleetd-p7/MAP.md`).
//!
//! An interactive worker starts on Phase 6. After the update, it ends as
//! before, and the next attempt of its task runs in exec mode. The update
//! changes no ledger row, and the new attempt gets the window 256000.
//!
//! The Phase 6 ledger is built by the real migration steps up to
//! [`schema::PHASE_6_VERSION`], the run and its idea in flight by the real
//! functions, then the rows of a run in flight are written as a Phase 6
//! binary left them: 1 task `running` with its live interactive push
//! attempt, its worker, its lease and its task folder, and the `fleetd` row
//! of the Phase 6 fleetd. The attempt folder has no exec stream
//! ([`TRANSCRIPT_JSONL`]), so the attempt is interactive.
//!
//! The interactive worker is a stand-in: a `sh` process, recorded as the
//! agent of the attempt. It waits on its stdin, then runs
//! `horch ledger handoff` as its child, so its attempt is in the process
//! chain of the command, and exits as the pane of a handed-off worker
//! does. The old fleetd is a stand-in too: a `sleep` process that the
//! `fleetd` row names.
//!
//! The update: the test stops the old fleetd, migrates the ledger to
//! [`schema::PHASE_7_VERSION`] and starts `horch fleetd` of this branch,
//! with the fake herdr and the fake agents. Each process uses the temporary
//! folders of its harness.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use horch_core::execution::lifecycle::TRANSCRIPT_JSONL;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::procid;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::Connection;
use serde_json::Value;

/// The fleet clock of the Phase 6 `fleetd` row at the update.
const CLOCK: i64 = 6000;
/// The lease of the live attempt ends 1 day of the fleet clock after the
/// update: no lease ends in a test.
const LEASE_END: i64 = CLOCK + 86_400;

/// The persona of the task. Its file names no mode, and its CLI, Claude
/// Code, has an exec adapter: a new attempt runs exec. Its fake agent is
/// `fake-claude`.
const PERSONA: &str = "opus";
/// The worker of the live interactive attempt.
const WORKER: &str = "w-interactive";

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The guard of [`Flight::wait_claude_conformance`] against a hang. A
/// conformance run ends long before it on any host.
const HANG_GUARD: Duration = Duration::from_secs(1200);

/// The window of a new attempt (FR-9; `teammates/_base/context-windows.md`).
const WINDOW: &str = "256000";

/// The script of the interactive worker. `$1` is the `horch` binary, `$2`
/// the handoff file and `$3` the folder of the outputs. It waits for 1
/// line, runs `horch ledger handoff`, writes its exit code, and exits.
const INTERACTIVE_WORKER: &str = r#"read go
"$1" ledger handoff "$2" >"$3/handoff.out" 2>&1
echo $? >"$3/handoff.code"
"#;

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

/// The interactive worker: the `sh` process of [`INTERACTIVE_WORKER`],
/// started before the update. Killed at the end of the test.
struct InteractiveWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    started: u64,
    /// The folder of its outputs.
    out: PathBuf,
}

impl InteractiveWorker {
    fn start(h: &Harness, handoff: &std::path::Path) -> InteractiveWorker {
        let out = h.root.join(WORKER);
        std::fs::create_dir_all(&out).unwrap();
        let mut cmd = Command::new("/bin/sh");
        h.seal(&mut cmd);
        cmd.env("HORCH_WORKER_ID", WORKER)
            .env("HORCH_ROLE", WORKER)
            .arg("-c")
            .arg(INTERACTIVE_WORKER)
            .arg("interactive-worker")
            .arg(Harness::horch_bin())
            .arg(handoff)
            .arg(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take();
        let started = procid::start_time(child.id()).expect("the start time of sh");
        InteractiveWorker {
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

    /// Run `horch ledger handoff`. It must pass. Gives the exit status of
    /// the worker, which exits after the command.
    fn hand_off(&mut self) -> ExitStatus {
        let stdin = self.stdin.as_mut().expect("the stdin of the worker");
        stdin.write_all(b"go\n").unwrap();
        stdin.flush().unwrap();
        let read = |name: &str| std::fs::read_to_string(self.out.join(name)).unwrap_or_default();
        let code = wait_for_or(
            "the interactive worker to run horch ledger handoff",
            || read("handoff.code").trim().parse::<i32>().ok(),
            || read("handoff.out"),
        );
        assert_eq!(code, 0, "horch ledger handoff: {}", read("handoff.out"));
        let child = &mut self.child;
        wait_for_or(
            "the interactive worker to exit",
            || child.try_wait().unwrap(),
            || read("handoff.out"),
        )
    }
}

impl Drop for InteractiveWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The run in flight on Phase 6: the ledger, the old fleetd, the
/// interactive worker and fleetd of this branch after the update. The
/// fields drop in this order: each process stops before the teardown of
/// the harness checks that none is left.
struct Flight {
    fleetds: Fleetds,
    worker: InteractiveWorker,
    old_fleetd: OldFleetd,
    task: String,
    h: Harness,
}

impl Flight {
    /// The Phase 6 run of the test `name`, with its interactive worker and
    /// the old fleetd. fleetd of this branch does not run yet.
    fn on_phase_6(name: &str) -> Flight {
        let h = world(name);
        let old_fleetd = OldFleetd::start();
        let task = phase_6_run(&h);
        let handoff = task_dir(&h, &task).join("handoff.md");
        let worker = InteractiveWorker::start(&h, &handoff);
        phase_6_attempt(&h, &task, &old_fleetd, &worker);
        phase_6_task_folder(&h, &task);
        std::fs::write(&handoff, "# Handoff\n\nThe next attempt goes on.\n").unwrap();
        let fleetds = Fleetds::new(&h);
        Flight {
            fleetds,
            worker,
            old_fleetd,
            task,
            h,
        }
    }

    /// The update of the ledger: stop the old fleetd and migrate the
    /// ledger to the schema of Phase 7.
    fn migrate(&mut self) {
        self.old_fleetd.stop();
        let mut conn = read(&self.h);
        schema::migrate_to(&mut conn, schema::PHASE_7_VERSION).unwrap();
    }

    /// The whole update: [`Flight::migrate`], then start fleetd of this
    /// branch and wait for 2 passes.
    fn update(&mut self) {
        self.migrate();
        self.fleetds.start(&self.h, "fleetd");
        self.wait_passes(2);
    }

    /// The `at` of the heartbeat of fleetd: it changes at the end of each
    /// pass.
    fn heartbeat_at(&self) -> Option<String> {
        let path = self.h.fleet_root().join("run").join("heartbeat.json");
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
                || self.diagnostics(),
            );
        }
    }

    /// Each docs job of the ledger: its kind, subject, state and result.
    fn docs_jobs(&self) -> Vec<String> {
        let conn = read(&self.h);
        let mut stmt = conn
            .prepare(
                "SELECT kind || ' ' || subject || ' ' || state || ' ' || COALESCE(result_json, '-')
                 FROM jobs WHERE kind IN ('docs_sync', 'conformance') ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    /// The conformance runs of Claude Code, the CLI of [`PERSONA`]: how
    /// many are queued or running, and how many have a result.
    fn claude_conformance(&self) -> (i64, i64) {
        read(&self.h)
            .query_row(
                "SELECT
                    count(*) FILTER (WHERE state IN ('queued', 'running')),
                    count(*) FILTER (WHERE state IN ('passed', 'failed'))
                 FROM jobs WHERE kind = 'conformance' AND subject LIKE 'claude %'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// Wait until the conformance run of Claude Code that the update queued
    /// has a result, and for 1 more pass, so the health of [`PERSONA`] is
    /// known before the next attempt is due. The run must pass: a failed
    /// run marks [`PERSONA`] unhealthy, and dispatch holds its task (FM-19;
    /// MAP.md Q15).
    ///
    /// The run is 9 launches of the fake CLI, and its time depends on the
    /// host. So this waits for the event, the result of the job, while the
    /// job is queued or running: the time limit of the job in fleetd bounds
    /// the wait. [`HANG_GUARD`] only stops a test that hangs.
    fn wait_claude_conformance(&self) {
        let until = Instant::now() + HANG_GUARD;
        loop {
            let (active, ended) = self.claude_conformance();
            if active == 0 && ended > 0 {
                break;
            }
            assert!(
                Instant::now() < until,
                "no result of the conformance run of claude after {} s:\n{}",
                HANG_GUARD.as_secs(),
                self.docs_jobs().join("\n")
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        self.wait_passes(1);
        let failed: i64 = read(&self.h)
            .query_row(
                "SELECT count(*) FROM jobs WHERE kind = 'conformance'
                    AND subject LIKE 'claude %' AND state = 'failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            failed,
            0,
            "the conformance run of claude of the update failed:\n{}",
            self.docs_jobs().join("\n")
        );
    }

    /// The id of attempt `n` of the task.
    fn attempt(&self, n: u32) -> String {
        format!("{}/A{n}", self.task)
    }

    /// Whether attempt `n` runs in exec mode: its attempt folder holds the
    /// exec stream, which the wrapper opens before it starts the CLI
    /// (`workers::attempt_mode`).
    fn is_exec(&self, n: u32) -> bool {
        attempt_dir(&self.h.fleet_root(), &self.task, &format!("A{n}"))
            .join(TRANSCRIPT_JSONL)
            .exists()
    }

    /// The state and the outcome of attempt `n`. `None` while no row exists.
    fn attempt_row(&self, n: u32) -> Option<(String, Option<String>)> {
        read(&self.h)
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [self.attempt(n)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    /// Hand off the interactive attempt, and wait until fleetd ends it.
    fn hand_off(&mut self) -> ExitStatus {
        let status = self.worker.hand_off();
        wait_for_or(
            "fleetd to end the interactive attempt",
            || {
                self.attempt_row(1)
                    .filter(|(state, _)| state == "ended")
                    .map(|_| ())
            },
            || self.diagnostics(),
        );
        status
    }

    /// The call of `fake-claude` that ran in exec mode with `START.md` of
    /// attempt `A2`: the agent of the next attempt, started by fleetd
    /// through the wrapper. A conformance run of the update is not it.
    fn exec_call(&self) -> Value {
        let outbox = format!("{}/attempts/A2/outbox", self.task);
        wait_for_or(
            "the agent of the next attempt to run in exec mode",
            || {
                self.h.calls_of("claude").into_iter().find(|c| {
                    c["exec_prompt"]
                        .as_str()
                        .is_some_and(|p| p.contains(&outbox))
                })
            },
            || self.diagnostics(),
        )
    }

    /// What a failed wait shows: the calls of the fakes, the output of
    /// fleetd, the attempts and the events of the task.
    fn diagnostics(&self) -> String {
        let calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .take(40)
            .map(|c| format!("{} {}", c["fake"], c["argv"]))
            .collect();
        let conn = read(&self.h);
        let mut stmt = conn
            .prepare(
                "SELECT id || ' ' || via || ' ' || state || ' ' || COALESCE(outcome, '-') FROM attempts
                 UNION ALL
                 SELECT actor || ' ' || kind || ' ' || text FROM events WHERE task_id = ?1",
            )
            .unwrap();
        let rows: Vec<String> = stmt
            .query_map([&self.task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        format!(
            "calls:\n{}\nfleetd.stderr:\n{}\nledger:\n{}",
            calls.join("\n"),
            self.fleetds.output("fleetd", "stderr"),
            rows.join("\n"),
        )
    }
}

/// A harness with its own ledger, fleet root, data folder and git
/// repository, no watchdog, a healthy quota, the workspace `w1` of the
/// fake herdr, whose panes run their commands, and an empty local folder as
/// the docs pages of the `docs_sync` runs that the update queues: no test
/// uses the network.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let docs = h.root.join("docs-fixture");
    std::fs::create_dir_all(&docs).unwrap();
    h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
    declare_gates(&h);
    // The workspace of the project, as `horch fleet` names it: a push
    // launch finds its pane there.
    let ws = h.fleet_workspace();
    assert_eq!(ws, "w1", "the seed of the run names w1");
    h
}

/// The project declares its gates and commits the file, so no hold of a
/// missing fast gate stops the dispatch (Phase 4, Q-C).
fn declare_gates(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {o:?}");
    }
}

/// A connection of the test to the ledger.
fn read(h: &Harness) -> Connection {
    Connection::open(h.ledger_db()).unwrap()
}

/// The task folder of `task`.
fn task_dir(h: &Harness, task: &str) -> PathBuf {
    h.fleet_root().join("tasks").join(task)
}

/// The Phase 6 ledger of `h` with the run `r1`, its idea in flight
/// (FR-12), its orchestrator seat in the pane `w1:p1`, and 1 task. Gives
/// the task id.
fn phase_6_run(h: &Harness) -> String {
    let mut conn = read(h);
    schema::migrate_to(&mut conn, schema::PHASE_6_VERSION).unwrap();
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = "# T1\n\nWrite the file.\n";
    std::fs::write(dir.join("t1.md"), ticket).unwrap();
    let run = runs::create(
        &conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            workspace_id: Some("w1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(&conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let seat = workers::insert(
        &conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            pane_id: Some("w1:p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    runs::set_orchestrator(&conn, &run.run_id, &seat).unwrap();
    let mut new = NewTask::work("t1", "tickets/t1.md", PERSONA);
    new.ticket_snapshot = Some(ticket.to_string());
    tasks::add(&conn, &run, &new, Actor::Orchestrator)
        .unwrap()
        .task_id
}

/// The rows of the live interactive attempt `A1` of `task`, as a Phase 6
/// binary left them: the task `running`, the worker of `agent` `working`,
/// the attempt `live` by push, its lease, and the `fleetd` row that names
/// `old` with the fleet clock at [`CLOCK`].
fn phase_6_attempt(h: &Harness, task: &str, old: &OldFleetd, agent: &InteractiveWorker) {
    let conn = read(h);
    let now = util::now();
    let host = util::host_name();
    let attempt = format!("{task}/A1");
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, ?1, ?2, ?3, ?4, ?3)",
        rusqlite::params![old.child.id(), old.started, &now, CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, host, agent_pid, agent_started,
                last_checkin_at, task_id, attempt, status, spawned_at, started_at, claimed_at)
            SELECT ?1, run_id, 'worker', ?2, ?3, ?4, ?5, ?6, task_id, 1, 'working', ?6, ?6, ?6
            FROM tasks WHERE task_id = ?7",
        rusqlite::params![
            WORKER,
            PERSONA,
            &host,
            agent.pid(),
            agent.started,
            &now,
            task
        ],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET status = 'running', attempts = 1, current_worker_id = ?1, updated_at = ?2
            WHERE task_id = ?3",
        (WORKER, &now, task),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at,
                active_at, via)
            VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, ?7, 'push')",
        rusqlite::params![
            &attempt,
            task,
            WORKER,
            agent.pid(),
            agent.started,
            &now,
            CLOCK
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
            VALUES (?1, ?2, ?3, ?4, 'claim')",
        (task, &attempt, LEASE_END, &now),
    )
    .unwrap();
}

/// The task folder and the outbox of the interactive attempt, as the
/// launch of Phase 6 made them: `task.yaml`, `START.md` and an empty outbox
/// with mode 0700. No exec stream: the attempt is interactive.
fn phase_6_task_folder(h: &Harness, task: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = task_dir(h, task);
    let attempt = attempt_dir(&h.fleet_root(), task, "A1");
    let outbox = attempt.join("outbox");
    std::fs::create_dir_all(&outbox).unwrap();
    for d in [
        h.fleet_root().join("tasks"),
        dir.clone(),
        dir.join("attempts"),
        attempt,
        outbox,
    ] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        dir.join("task.yaml"),
        format!("task: {task}\nattempt: {task}/A1\n"),
    )
    .unwrap();
    std::fs::write(dir.join("START.md"), "# T1\n").unwrap();
}

/// Each row of each table of the ledger, as SQL text, by table.
fn all_rows(conn: &Connection) -> BTreeMap<String, Vec<String>> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let mut rows = BTreeMap::new();
    for table in tables {
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
            .unwrap()
            .query_map([&table], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let quoted: Vec<String> = columns.iter().map(|c| format!("quote(\"{c}\")")).collect();
        let sql = format!(
            "SELECT {} FROM \"{table}\" ORDER BY 1",
            quoted.join(" || '|' || ")
        );
        let mut found: Vec<String> = conn
            .prepare(&sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        found.sort();
        rows.insert(table, found);
    }
    rows
}

#[test]
fn interactive_worker_live_at_the_update_ends_as_before() {
    let mut f = Flight::on_phase_6("p7r-ends");
    f.update();

    // fleetd of Phase 7 does not end the live interactive attempt.
    assert!(f.worker.alive(), "fleetd stops no live interactive worker");
    assert_eq!(
        f.attempt_row(1),
        Some(("live".into(), None)),
        "{}",
        f.diagnostics()
    );
    assert!(!f.is_exec(1), "the live attempt stays interactive");
    assert_eq!(f.attempt_row(2), None, "no 2nd attempt while A1 is live");

    // It ends as before: the old worker command hands off, and the worker
    // exits by itself.
    let status = f.hand_off();
    assert!(status.success(), "the worker exits by itself: {status:?}");
    assert_eq!(
        f.attempt_row(1),
        Some(("ended".into(), Some("handed_off".into()))),
        "{}",
        f.diagnostics()
    );
    assert!(!f.is_exec(1), "the ended attempt was interactive");
}

#[test]
fn next_attempt_of_its_task_runs_in_exec_mode() {
    horch_e2e::expected::check(
        file!(),
        "next_attempt_of_its_task_runs_in_exec_mode",
        || {
            let mut f = Flight::on_phase_6("p7r-next");
            f.update();
            f.wait_claude_conformance();
            f.hand_off();

            let call = f.exec_call();
            let prompt = call["exec_prompt"].as_str().unwrap_or_default();
            assert!(
                prompt.contains("handoff.md"),
                "START.md of the exec attempt names the handoff file: {prompt}"
            );
            wait_for_or(
                "the wrapper to open the exec stream of A2",
                || f.is_exec(2).then_some(()),
                || f.diagnostics(),
            );
            let via: String = read(&f.h)
                .query_row(
                    "SELECT via FROM attempts WHERE id = ?1",
                    [f.attempt(2)],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(via, "push");
        },
    );
}

#[test]
fn update_changes_no_ledger_row() {
    let mut f = Flight::on_phase_6("p7r-rows");
    let before = all_rows(&read(&f.h));
    f.migrate();
    let conn = read(&f.h);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, schema::PHASE_7_VERSION);
    let after = all_rows(&conn);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the update adds and drops no table"
    );
    for (table, rows) in &before {
        assert_eq!(
            Some(rows),
            after.get(table),
            "the update changes no row of {table}"
        );
    }
    assert!(
        !before["attempts"].is_empty() && !before["events"].is_empty(),
        "the ledger has a run in flight"
    );
}

#[test]
fn new_attempt_after_the_update_gets_256000() {
    horch_e2e::expected::check(file!(), "new_attempt_after_the_update_gets_256000", || {
        let mut f = Flight::on_phase_6("p7r-window");
        f.update();
        f.wait_claude_conformance();
        f.hand_off();

        let call = f.exec_call();
        let argv: Vec<String> = serde_json::from_value(call["argv"].clone()).unwrap();
        let settings = argv
            .windows(2)
            .find(|w| w[0] == "--settings")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| panic!("the exec launch has --settings: {argv:?}"));
        let settings: Value = serde_json::from_str(&settings)
            .unwrap_or_else(|e| panic!("--settings is inline JSON ({e}): {settings}"));
        assert_eq!(
            settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"].as_str(),
            Some(WINDOW),
            "the window of the new attempt: {settings}"
        );
    });
}
