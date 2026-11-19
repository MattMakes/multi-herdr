//! fleetd reads the wrapper files of an attempt, `heartbeat.json`,
//! `exit.json` and `session.json`, and writes the rows that the wrapper
//! wrote before (docs/tickets/fleetd-p5/31-ingest-wrapper-files.md).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol",
//! protocol rules 2 and 7; RR-5, RR-8 and SR-6.
//!
//! - `heartbeat.json` is a snapshot: fleetd uses it only when its `seq` is
//!   higher than the last `seq` it used for the attempt.
//! - `exit.json` and `session.json` are messages: ingested 1 time, by path
//!   and hash.
//!
//! The wrapper writer of ticket 30 writes the files. Each test has its own
//! temporary ledger and fleet root, and gives the time and the fleet clock.
//! The process facts come through a seam, except in the stop test, which
//! stops 2 real child processes of the test.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write as _};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use horch_core::execution::legacy::STATUS_WORKING;
use horch_core::execution::records::{NewRecord, SessionLedger};
use horch_core::execution::{ExecutionStatus, FailureKind};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::task_execution::adapters::attempt_files::{
    self, Found, OsProcesses, ProcessFacts, WrapperRead,
};
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::domain::task::Reap;
use horch_core::taskledger::model::{Actor, StopRequester, WorkerStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::stop::{self, Panes, StopOptions};
use horch_core::taskledger::supervisor::{AgentFacts, AttemptFiles, HEARTBEAT_JSON, SESSION_JSON};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use horch_platform::channel::User;

const T1: &str = "2026-11-18T10:00:00Z";
const T2: &str = "2026-11-18T10:00:10Z";
const HEARTBEAT_MAX_BYTES: u64 = 4096;

/// A ledger with 1 worker and its live attempt, and a fleet root.
struct World {
    _tmp: tempfile::TempDir,
    store: Store,
    fleet_root: PathBuf,
    worker: String,
    attempt: AttemptRef,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("ledger.sqlite3");
        let fleet_root = tmp.path().join("fleet");
        std::fs::create_dir_all(&fleet_root).unwrap();
        let mut store = Store::open(&db).unwrap();
        let (worker, task) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                let task = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("a", "tickets/a.md", "opus"),
                    Actor::Orchestrator,
                )?;
                let worker = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-x".into()),
                        ..Default::default()
                    },
                )?;
                Ok((worker, task.task_id))
            })
            .unwrap();
        let attempt_id = format!("{task}/A1");
        store
            .conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state) VALUES (?1, ?2, ?3, 'live')",
                [&attempt_id, &task, &worker],
            )
            .unwrap();
        World {
            _tmp: tmp,
            store,
            fleet_root,
            worker,
            attempt: AttemptRef {
                task_id: task,
                attempt_id,
            },
        }
    }

    /// The writer of the wrapper of the attempt (ticket 30).
    fn files(&self) -> AttemptFiles {
        AttemptFiles::new(&self.fleet_root, self.attempt.clone())
    }

    fn dir(&self) -> PathBuf {
        self.files().dir().to_path_buf()
    }

    /// Part 1 of a pass: read the wrapper files of the attempt.
    fn read(&self) -> WrapperRead {
        let owner = User::current().unwrap();
        let mut reads = attempt_files::read(
            self.store.conn(),
            &self.fleet_root,
            std::slice::from_ref(&self.attempt),
            &owner,
            HEARTBEAT_MAX_BYTES,
        )
        .unwrap();
        assert_eq!(reads.len(), 1);
        reads.pop().unwrap()
    }

    /// Part 2 of a pass: apply `read` in 1 transaction at time `at`.
    fn ingest(&mut self, read: &WrapperRead, at: &str, procs: &dyn ProcessFacts) {
        let reads = std::slice::from_ref(read);
        self.store
            .immediate(|tx| attempt_files::ingest(tx, None, reads, FleetSeconds(100), at, procs))
            .unwrap();
    }

    /// Read and apply the wrapper files at time `at`.
    fn pass(&mut self, at: &str, procs: &dyn ProcessFacts) {
        let read = self.read();
        self.ingest(&read, at, procs);
    }

    /// The session records of the project `/r`, in the ledger file of the
    /// world, read as stored: no worker row is laid over them.
    fn sessions(&self) -> SessionLedger {
        // The world is fleetd here: it writes with the writer of fleetd.
        SessionLedger::for_project(self._tmp.path(), "/r").with_writer(std::sync::Arc::new(
            horch_core::taskledger::store::LedgerWriter::fleetd(),
        ))
    }

    /// A live session record `record` of the worker, as the launch writes it.
    fn add_record(&self, record: &str) {
        self.sessions()
            .add(NewRecord {
                record_id: record,
                agent: "claude",
                tier: "opus",
                model: "opus",
                role: "opus-x",
                session_id: Some("s-1"),
                task: "T-1: a",
                phase: None,
            })
            .unwrap();
    }

    fn worker_row(&self) -> workers::Worker {
        workers::get(self.store.conn(), &self.worker).unwrap()
    }

    fn heartbeat_seq(&self) -> Option<i64> {
        self.store
            .conn()
            .query_row(
                "SELECT heartbeat_seq FROM attempts WHERE id = ?1",
                [&self.attempt.attempt_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Write `bytes` as `name` of the attempt folder, as the wrapper does:
    /// mode 0600, then a rename.
    fn write_raw(&self, name: &str, bytes: &[u8]) {
        let dir = self.dir();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .unwrap();
        let tmp = dir.join(format!("{name}.tmp"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .unwrap();
        f.write_all(bytes).unwrap();
        drop(f);
        std::fs::rename(&tmp, dir.join(name)).unwrap();
    }

    /// A heartbeat of the attempt, in the shape of the wrapper.
    fn heartbeat_json(&self, seq: u64, pid: u32, agent_state: &str) -> serde_json::Value {
        serde_json::json!({
            "task": self.attempt.task_id,
            "attempt": "A1",
            "pid": pid,
            "pgid": null,
            "mux_pane": null,
            "proc_state": "running",
            "agent_state": agent_state,
            "last_output_at": null,
            "tokens": null,
            "seq": seq,
            "written_at": T1,
        })
    }
}

/// The process facts of the test: each PID with its start time and parent.
#[derive(Default)]
struct FakeProcesses(BTreeMap<u32, (u64, Option<u32>)>);

impl FakeProcesses {
    /// An agent 5004242 (start 77), a child of the wrapper 5004241 (start 66).
    fn agent() -> FakeProcesses {
        let mut p = FakeProcesses::default();
        p.0.insert(5004241, (66, Some(1)));
        p.0.insert(5004242, (77, Some(5004241)));
        p
    }
}

impl ProcessFacts for FakeProcesses {
    fn start_time(&self, pid: u32) -> Option<u64> {
        self.0.get(&pid).map(|p| p.0)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        self.0.get(&pid).and_then(|p| p.1)
    }
}

fn agent_facts(pid: u32) -> std::sync::Arc<AgentFacts> {
    let facts = AgentFacts::new(None);
    facts.agent_started(pid);
    facts
}

#[test]
fn heartbeat_with_a_higher_seq_renews_the_checkin() {
    let mut w = World::new();
    let procs = FakeProcesses::agent();
    let facts = agent_facts(5004242);
    w.files().write_heartbeat(&facts).unwrap();
    w.pass(T1, &procs);

    let row = w.worker_row();
    assert_eq!(row.last_checkin_at.as_deref(), Some(T1));
    assert_eq!(w.heartbeat_seq(), Some(1));
    assert_eq!(
        (row.agent_pid, row.agent_started),
        (Some(5004242), Some(77))
    );
    assert_eq!(
        (row.supervisor_pid, row.supervisor_started),
        (Some(5004241), Some(66))
    );

    w.files().write_heartbeat(&facts).unwrap();
    w.pass(T2, &procs);
    assert_eq!(w.worker_row().last_checkin_at.as_deref(), Some(T2));
    assert_eq!(w.heartbeat_seq(), Some(2));
    // A snapshot is not a message: it has no `ingested` row.
    assert_eq!(w.count("SELECT count(*) FROM ingested"), 0);
    let active_at = "SELECT count(*) FROM attempts WHERE active_at = 100";
    assert_eq!(w.count(active_at), 0, "no output, no activity");

    // Output of the agent after the last checkin is activity.
    let mut hb = w.heartbeat_json(3, 5004242, "working");
    hb["last_output_at"] = "2026-11-18T10:00:15Z".into();
    w.write_raw(HEARTBEAT_JSON, hb.to_string().as_bytes());
    w.pass("2026-11-18T10:00:20Z", &procs);
    assert_eq!(w.count(active_at), 1);
}

#[test]
fn heartbeat_with_an_old_seq_is_not_used() {
    let mut w = World::new();
    let mut procs = FakeProcesses::agent();
    w.write_raw(
        HEARTBEAT_JSON,
        w.heartbeat_json(5, 5004242, "working")
            .to_string()
            .as_bytes(),
    );
    w.pass(T1, &procs);
    assert_eq!(w.heartbeat_seq(), Some(5));

    // The same seq, read again in the next pass, renews nothing.
    w.pass(T2, &procs);
    assert_eq!(w.worker_row().last_checkin_at.as_deref(), Some(T1));

    // A lower seq with another agent is not used either.
    procs.0.insert(5004243, (88, Some(5004241)));
    w.write_raw(
        HEARTBEAT_JSON,
        w.heartbeat_json(4, 5004243, "working")
            .to_string()
            .as_bytes(),
    );
    w.pass(T2, &procs);
    let row = w.worker_row();
    assert_eq!(row.last_checkin_at.as_deref(), Some(T1));
    assert_eq!(row.agent_pid, Some(5004242));
    assert_eq!(w.heartbeat_seq(), Some(5));
}

#[test]
fn heartbeat_over_4_kib_is_rejected() {
    let mut w = World::new();
    let procs = FakeProcesses::agent();
    let big = "x".repeat(HEARTBEAT_MAX_BYTES as usize);
    w.write_raw(
        HEARTBEAT_JSON,
        w.heartbeat_json(1, 5004242, &big).to_string().as_bytes(),
    );
    let read = w.read();
    let Some(Found::Rejected { reason, .. }) = read.heartbeat.as_ref().map(|f| &f.found) else {
        panic!("{read:?}");
    };
    assert!(reason.contains("4096"), "{reason}");
    w.ingest(&read, T1, &procs);

    let row = w.worker_row();
    assert_eq!(row.last_checkin_at, None);
    assert_eq!(row.agent_pid, None);
    assert_eq!(w.heartbeat_seq(), None);
    assert_eq!(
        w.count("SELECT count(*) FROM ingested WHERE outcome = 'rejected'"),
        1
    );
    // The rejected path is not read again.
    assert!(w.read().heartbeat.is_none());
}

#[test]
fn exit_json_settles_the_attempt_once() {
    let mut w = World::new();
    let procs = FakeProcesses::default();
    w.files().write_exit(Some(3), false).unwrap();
    let read = w.read();
    assert!(matches!(
        read.exit.as_ref().map(|f| &f.found),
        Some(Found::Valid { .. })
    ));
    w.ingest(&read, T1, &procs);
    // The same read applied 2 times, and a new read: 1 effect.
    w.ingest(&read, T2, &procs);
    w.pass(T2, &procs);

    let row = w.worker_row();
    assert_eq!(row.exit_code, Some(3));
    assert_eq!(row.status().unwrap(), WorkerStatus::LaunchFailed);
    let exited = format!(
        "SELECT count(*) FROM events WHERE kind = 'exited' AND worker_id = '{}'",
        w.worker
    );
    assert_eq!(w.count(&exited), 1);
    assert_eq!(
        w.count("SELECT count(*) FROM ingested WHERE outcome = 'accepted'"),
        1
    );
}

#[test]
fn session_json_writes_the_session_record_once() {
    let mut w = World::new();
    let procs = FakeProcesses::default();
    assert!(w
        .files()
        .write_session("s-1", "claude", Some("/t/s-1.jsonl"), Some("r-1"))
        .unwrap());
    let read = w.read();
    w.ingest(&read, T1, &procs);
    w.ingest(&read, T2, &procs);
    w.pass(T2, &procs);

    let row = w.worker_row();
    assert_eq!(row.session_id.as_deref(), Some("s-1"));
    assert_eq!(row.transcript_path.as_deref(), Some("/t/s-1.jsonl"));
    assert_eq!(row.legacy_record_id.as_deref(), Some("r-1"));
    let sessions = format!(
        "SELECT count(*) FROM events WHERE kind = 'session' AND worker_id = '{}'",
        w.worker
    );
    assert_eq!(w.count(&sessions), 1);
    assert_eq!(
        w.count("SELECT count(*) FROM ingested WHERE outcome = 'accepted'"),
        1
    );
}

/// The record id of `session.json` is a claim of a file in the task
/// folder: an id of another form, or longer than 64 bytes, rejects the file
/// and writes no column of the worker row.
#[test]
fn session_json_with_a_bad_record_id_is_rejected() {
    for bad in ["r 1", "../r-1", "", &"r".repeat(65)] {
        let mut w = World::new();
        let procs = FakeProcesses::default();
        let file = serde_json::json!({
            "task": w.attempt.task_id,
            "attempt": "A1",
            "session_id": "s-1",
            "harness": "claude",
            "transcript": null,
            "record_id": bad,
            "written_at": T1,
        });
        w.write_raw(SESSION_JSON, file.to_string().as_bytes());
        let read = w.read();
        w.ingest(&read, T1, &procs);

        let row = w.worker_row();
        assert_eq!(row.legacy_record_id, None, "{bad:?}");
        assert_eq!(row.session_id, None, "{bad:?}");
        let rejected = "SELECT count(*) FROM ingested WHERE outcome = 'rejected' AND reason LIKE 'record_id %'";
        assert_eq!(w.count(rejected), 1, "{bad:?}");
    }
}

/// The ingest of `exit.json` ends the session record of the attempt in the
/// same transaction as the worker row: the record leaves `working` and
/// holds the exit state. `session.json` of the same pass names the record.
/// A second ingest of the same file changes nothing.
#[test]
fn exit_json_ends_the_session_record_of_the_attempt() {
    let mut w = World::new();
    let procs = FakeProcesses::default();
    w.add_record("r-1");
    assert!(w
        .sessions()
        .get("r-1")
        .unwrap()
        .execution_status()
        .is_live());
    w.files()
        .write_session("s-1", "claude", None, Some("r-1"))
        .unwrap();
    w.files().write_exit(Some(3), false).unwrap();
    let read = w.read();
    w.ingest(&read, T1, &procs);

    let record = w.sessions().get("r-1").unwrap();
    assert_eq!(
        record.execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) }
        }
    );
    assert_ne!(record.status, STATUS_WORKING);
    assert_eq!(record.exit_code, Some(3));
    assert_eq!(record.finished_at.as_deref(), Some(T1));
    let stored = w.sessions().store().stored_json().unwrap();
    w.ingest(&read, T2, &procs);
    w.pass(T2, &procs);
    assert_eq!(w.sessions().store().stored_json().unwrap(), stored);
}

/// Rule: an exit whose worker has no session record in this ledger file
/// ends only the worker row. Here `session.json` names the record `r-9`,
/// which the ledger does not hold (a moved state dir keeps its records in
/// another file). The ingest succeeds and writes no session record.
#[test]
fn exit_json_with_no_session_record_ends_only_the_worker_row() {
    let mut w = World::new();
    let procs = FakeProcesses::default();
    w.files()
        .write_session("s-9", "claude", None, Some("r-9"))
        .unwrap();
    w.files().write_exit(Some(3), false).unwrap();
    w.pass(T1, &procs);

    let row = w.worker_row();
    assert_eq!(row.legacy_record_id.as_deref(), Some("r-9"));
    assert_eq!(row.exit_code, Some(3));
    assert_eq!(row.status().unwrap(), WorkerStatus::LaunchFailed);
    assert_eq!(w.count("SELECT count(*) FROM session_records"), 0);
}

/// Rule: a crash writes no `exit.json`, so no ingest ends the stored
/// record. The pass sees that the process is gone and ends the attempt
/// and the worker row (`EndAttempt`). A reader with the task ledger
/// overlay, as `horch sessions`, shows the record ended from the worker
/// row. The stored record keeps what the launch wrote.
#[test]
fn crash_with_no_exit_json_ends_the_record_through_the_worker_row() {
    let mut w = World::new();
    let procs = FakeProcesses::default();
    w.add_record("r-1");
    w.files()
        .write_session("s-1", "claude", None, Some("r-1"))
        .unwrap();
    w.pass(T1, &procs);
    let reap = Reap {
        attempt: w.attempt.attempt_id.clone(),
        worker: w.worker.clone(),
        outcome: "no_result".into(),
        end_worker: true,
        task: None,
        cleanup: Default::default(),
    };
    w.store
        .immediate(|tx| commands::end_attempt(tx, &reap))
        .unwrap();

    assert_eq!(w.worker_row().status().unwrap(), WorkerStatus::Incomplete);
    assert!(w
        .sessions()
        .get("r-1")
        .unwrap()
        .execution_status()
        .is_live());
    let db = w._tmp.path().join("ledger.sqlite3");
    let overlaid = w
        .sessions()
        .with_task_ledger(&db, Path::new("/r"))
        .get("r-1")
        .unwrap();
    assert!(matches!(
        overlaid.execution_status(),
        ExecutionStatus::Failed { .. }
    ));
}

#[test]
fn idempotent_agent_status_changed() {
    let mut w = World::new();
    let procs = FakeProcesses::agent();
    w.write_raw(
        HEARTBEAT_JSON,
        w.heartbeat_json(1, 5004242, "working")
            .to_string()
            .as_bytes(),
    );
    let read = w.read();
    w.ingest(&read, T1, &procs);
    let snapshot = |w: &World| {
        let row = w.worker_row();
        (
            row.last_checkin_at,
            row.agent_pid,
            row.agent_started,
            row.supervisor_pid,
            row.supervisor_started,
            w.heartbeat_seq(),
            w.count("SELECT count(*) FROM events"),
        )
    };
    let before = snapshot(&w);
    // The same snapshot again, applied and read, changes nothing.
    w.ingest(&read, T2, &procs);
    w.pass(T2, &procs);
    assert_eq!(snapshot(&w), before);
}

/// A heartbeat whose agent is not a child of the recorded wrapper, while
/// that wrapper runs, does not move the PIDs that a stop uses.
#[test]
fn heartbeat_pid_outside_the_recorded_wrapper_is_not_used() {
    let mut w = World::new();
    let mut procs = FakeProcesses::agent();
    w.files().write_heartbeat(&agent_facts(5004242)).unwrap();
    w.pass(T1, &procs);

    procs.0.insert(5009000, (5, Some(5008999)));
    procs.0.insert(5008999, (4, Some(1)));
    w.write_raw(
        HEARTBEAT_JSON,
        w.heartbeat_json(9, 5009000, "working")
            .to_string()
            .as_bytes(),
    );
    w.pass(T2, &procs);
    let row = w.worker_row();
    assert_eq!(row.agent_pid, Some(5004242));
    assert_eq!(row.supervisor_pid, Some(5004241));
    // The wrapper still beats: the checkin moves.
    assert_eq!(row.last_checkin_at.as_deref(), Some(T2));
}

/// No pane: the stop test closes no pane.
struct NoPanes;

impl Panes for NoPanes {
    fn exists(&self, _pane: &str) -> bool {
        false
    }

    fn close(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// `horch ledger stop` stops a spawned worker by the PIDs that fleetd took
/// from its `heartbeat.json`: a wrapper `sh` and its agent `sleep`. The
/// wrapper outlives its agent, so only its stop ends it.
#[test]
fn horch_ledger_stop_stops_a_spawned_worker_by_the_ingested_pid() {
    let mut w = World::new();
    let mut wrapper = Command::new("/bin/sh")
        .args(["-c", "sleep 600 & echo $!; wait; exec sleep 600"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let wrapper_pid = wrapper.id();
    let mut line = String::new();
    BufReader::new(wrapper.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let agent_pid: u32 = line.trim().parse().unwrap();
    let agent_start = horch_core::procid::start_time(agent_pid).unwrap();
    // Reap the wrapper as soon as it ends, so its stop sees no zombie.
    let reaper = std::thread::spawn(move || wrapper.wait().unwrap());

    w.files().write_heartbeat(&agent_facts(agent_pid)).unwrap();
    w.pass(T1, &OsProcesses);
    let row = w.worker_row();
    assert_eq!(row.agent_pid, Some(i64::from(agent_pid)));
    assert_eq!(row.supervisor_pid, Some(i64::from(wrapper_pid)));

    let opts = StopOptions {
        grace: Duration::from_secs(30),
        poll: Duration::from_millis(10),
        final_status: WorkerStatus::Stopped,
    };
    let report = stop::stop_worker(
        &mut w.store,
        &w.worker.clone(),
        StopRequester::Operator,
        "the test stops it",
        &NoPanes,
        &stop::Os,
        &opts,
    )
    .unwrap();
    let ended = reaper.join().unwrap();
    assert!(!ended.success(), "{ended:?}");
    assert!(
        !horch_core::procid::alive(agent_pid, Some(agent_start)),
        "the agent {agent_pid} still runs"
    );
    assert_eq!(
        report.steps[0].pid,
        Some(i64::from(agent_pid)),
        "{report:?}"
    );
    let wrapper_step = report.steps.iter().find(|s| s.target == "supervisor");
    assert_eq!(
        wrapper_step.and_then(|s| s.pid),
        Some(i64::from(wrapper_pid)),
        "{report:?}"
    );
    assert_eq!(w.worker_row().status().unwrap(), WorkerStatus::Stopped);
}
