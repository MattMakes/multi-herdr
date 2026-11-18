//! The wrapper (`horch worker`) reports the facts of its agent through 3
//! files of its attempt folder, and writes no ledger row
//! (docs/specs/fleetd/design.md, "Workspace layout and file protocol",
//! protocol rules 1 to 3; RR-5, RR-8; docs/tickets/fleetd-p5/
//! 30-wrapper-writes-files.md).
//!
//! - `heartbeat.json`: a snapshot, replaced atomically, with a `seq` that
//!   grows by 1 on each write.
//! - `exit.json`: the exit of the agent, 1 time.
//! - `session.json`: the session id of the CLI and its harness, 1 time.
//!
//! Each test has its own temporary ledger and fleet root. No test starts a
//! `horch` binary, waits for a time or reads the clock of this machine.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::supervisor::{
    write_exit, AgentFacts, ExitFile, HeartbeatFile, HeartbeatWriter, SessionFile, SessionLookup,
    EXIT_JSON, HEARTBEAT_JSON, SESSION_JSON,
};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

/// A ledger with 1 worker and its live attempt, and a fleet root.
struct World {
    _tmp: tempfile::TempDir,
    db: PathBuf,
    fleet_root: PathBuf,
    worker: String,
    task: String,
    attempt_id: String,
}

impl World {
    /// `with_attempt`: the worker has a live attempt.
    fn new(with_attempt: bool) -> World {
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
        if with_attempt {
            store
                .conn()
                .execute(
                    "INSERT INTO attempts (id, task_id, worker_id, state) VALUES (?1, ?2, ?3, 'live')",
                    [&attempt_id, &task, &worker],
                )
                .unwrap();
        }
        World {
            _tmp: tmp,
            db,
            fleet_root,
            worker,
            task,
            attempt_id,
        }
    }

    fn dir(&self) -> PathBuf {
        self.fleet_root
            .join("tasks")
            .join(&self.task)
            .join("attempts")
            .join("A1")
    }

    fn writer(&self, facts: &Arc<AgentFacts>, lookup: SessionLookup) -> HeartbeatWriter {
        HeartbeatWriter::new(
            self.db.clone(),
            self.worker.clone(),
            self.fleet_root.clone(),
            facts.clone(),
            "claude".into(),
            lookup,
        )
    }

    /// The worker row columns that the wrapper wrote before Phase 5.
    fn worker_row(&self) -> (Option<String>, Option<i64>, Option<String>, Option<i64>) {
        let store = Store::open_read_only(&self.db).unwrap();
        store
            .conn()
            .query_row(
                "SELECT last_checkin_at, agent_pid, session_id, exit_code
                 FROM workers WHERE worker_id = ?1",
                [&self.worker],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    }
}

fn no_session() -> SessionLookup {
    Box::new(|| (None, None))
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn wrapper_writes_heartbeat_with_a_growing_seq() {
    let w = World::new(true);
    let facts = AgentFacts::new(Some("w1:p2".into()));
    let mut writer = w.writer(&facts, no_session());
    let path = w.dir().join(HEARTBEAT_JSON);

    writer.beat().unwrap();
    let first: HeartbeatFile = read(&path);
    assert_eq!(first.seq, 1);
    assert_eq!(first.task, w.task);
    assert_eq!(first.attempt, "A1");
    assert_eq!(first.pid, None, "no agent PID before the agent starts");
    assert_eq!(first.proc_state, None);
    assert_eq!(first.mux_pane.as_deref(), Some("w1:p2"));
    assert!(!first.written_at.is_empty());

    let me = std::process::id();
    facts.agent_started(me);
    writer.beat().unwrap();
    writer.beat().unwrap();
    let third: HeartbeatFile = read(&path);
    assert_eq!(third.seq, 3);
    assert_eq!(third.pid, Some(me));
    assert_eq!(third.proc_state.as_deref(), Some("running"));

    // A second writer of the same wrapper (the repair resume) goes on
    // with the same seq: fleetd uses only a higher seq.
    let mut resumed = w.writer(&facts, no_session());
    resumed.beat().unwrap();
    assert_eq!(read::<HeartbeatFile>(&path).seq, 4);
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn wrapper_with_no_attempt_writes_no_file() {
    let w = World::new(false);
    let facts = AgentFacts::new(None);
    let mut writer = w.writer(&facts, Box::new(|| (Some("s-1".into()), None)));
    writer.beat().unwrap();
    assert!(!w.fleet_root.join("tasks").exists());
    assert!(!write_exit(&w.db, &w.worker, &w.fleet_root, Some(0), false).unwrap());
    assert!(!w.fleet_root.join("tasks").exists());
}

/// The interim beat of a worker with no attempt (W8, removed by fleetd-p5
/// ticket 42) runs only while the worker has no attempt.
#[test]
fn wrapper_with_an_attempt_runs_no_interim_ledger_beat() {
    let runs = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counted = || {
        let runs = runs.clone();
        Box::new(move || {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }) as horch_core::taskledger::supervisor::NoAttemptBeat
    };
    let w = World::new(true);
    let facts = AgentFacts::new(None);
    let mut writer = w
        .writer(&facts, no_session())
        .with_no_attempt_beat(counted());
    writer.beat().unwrap();
    writer.beat().unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(w.dir().join(HEARTBEAT_JSON).is_file());

    let w = World::new(false);
    let mut writer = w
        .writer(&facts, no_session())
        .with_no_attempt_beat(counted());
    writer.beat().unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(!w.fleet_root.join("tasks").exists());
}

#[test]
fn wrapper_writes_exit_json_once() {
    let w = World::new(true);
    assert!(write_exit(&w.db, &w.worker, &w.fleet_root, Some(3), true).unwrap());
    let path = w.dir().join(EXIT_JSON);
    let exit: ExitFile = read(&path);
    assert_eq!(exit.task, w.task);
    assert_eq!(exit.attempt, "A1");
    assert_eq!(exit.code, Some(3));
    assert!(exit.repaired);
    assert!(!exit.at.is_empty());
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);

    let before = std::fs::read(&path).unwrap();
    let again = write_exit(&w.db, &w.worker, &w.fleet_root, None, false);
    assert!(again.is_err(), "a second exit is refused");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "exit.json is unchanged"
    );
    assert!(!w.dir().join(format!("{EXIT_JSON}.tmp")).exists());
}

#[test]
fn wrapper_writes_session_json_when_it_finds_the_session() {
    let w = World::new(true);
    let found: Arc<Mutex<(Option<String>, Option<String>)>> = Arc::default();
    let lookup: SessionLookup = {
        let found = found.clone();
        Box::new(move || found.lock().unwrap().clone())
    };
    let facts = AgentFacts::new(None);
    let mut writer = w.writer(&facts, lookup);
    let path = w.dir().join(SESSION_JSON);

    writer.beat().unwrap();
    assert!(!path.exists(), "no session id yet, no session.json");

    *found.lock().unwrap() = (Some("sess-1".into()), Some("/t/sess-1.jsonl".into()));
    writer.beat().unwrap();
    let session: SessionFile = read(&path);
    assert_eq!(session.task, w.task);
    assert_eq!(session.attempt, "A1");
    assert_eq!(session.session_id, "sess-1");
    assert_eq!(session.harness, "claude");
    assert_eq!(session.transcript.as_deref(), Some("/t/sess-1.jsonl"));
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);

    // 1 time: a later lookup, or a second writer, changes nothing.
    *found.lock().unwrap() = (Some("sess-2".into()), None);
    writer.beat().unwrap();
    let found2 = found.clone();
    let mut resumed = w.writer(&facts, Box::new(move || found2.lock().unwrap().clone()));
    resumed.beat().unwrap();
    assert_eq!(read::<SessionFile>(&path).session_id, "sess-1");
}

#[cfg(unix)]
#[test]
fn wrapper_opens_no_writable_ledger() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new(true);
    let before = w.worker_row();
    std::fs::set_permissions(&w.db, std::fs::Permissions::from_mode(0o444)).unwrap();

    let facts = AgentFacts::new(None);
    facts.agent_started(std::process::id());
    let mut writer = w.writer(&facts, Box::new(|| (Some("sess-1".into()), None)));
    writer.beat().unwrap();
    writer.beat().unwrap();
    assert!(write_exit(&w.db, &w.worker, &w.fleet_root, Some(0), false).unwrap());

    assert!(w.dir().join(HEARTBEAT_JSON).is_file());
    assert!(w.dir().join(SESSION_JSON).is_file());
    assert!(w.dir().join(EXIT_JSON).is_file());
    assert_eq!(mode(&w.db), 0o444);
    assert_eq!(w.worker_row(), before, "the wrapper wrote no worker column");
    let store = Store::open_read_only(&w.db).unwrap();
    let attempt: (String, Option<i64>, Option<i64>) = store
        .conn()
        .query_row(
            "SELECT state, heartbeat_seq, exit_code FROM attempts WHERE id = ?1",
            [&w.attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(attempt, ("live".into(), None, None));
}

#[cfg(unix)]
#[test]
fn heartbeat_is_replaced_atomically() {
    use std::os::unix::fs::MetadataExt;
    let w = World::new(true);
    let facts = AgentFacts::new(None);
    let mut writer = w.writer(&facts, no_session());
    writer.beat().unwrap();
    let path = w.dir().join(HEARTBEAT_JSON);
    let tmp = w.dir().join(format!("{HEARTBEAT_JSON}.tmp"));

    // A link that the agent puts at the temp name is not followed.
    let target = w.dir().join("target");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, &tmp).unwrap();

    // A reader never sees a part of a heartbeat while the writer beats.
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (done, path) = (done.clone(), path.clone());
        std::thread::spawn(move || {
            let mut reads = 0u64;
            while !done.load(Ordering::SeqCst) {
                let bytes = std::fs::read(&path).expect("heartbeat.json always exists");
                serde_json::from_slice::<HeartbeatFile>(&bytes).expect("a whole heartbeat");
                reads += 1;
            }
            reads
        })
    };
    let mut inodes = std::collections::HashSet::new();
    for _ in 0..200 {
        writer.beat().unwrap();
        inodes.insert(std::fs::metadata(&path).unwrap().ino());
    }
    done.store(true, Ordering::SeqCst);
    assert!(reader.join().unwrap() > 0);

    assert_eq!(read::<HeartbeatFile>(&path).seq, 201);
    assert!(inodes.len() > 1, "each beat renames a new file into place");
    assert!(
        !tmp.exists() && tmp.symlink_metadata().is_err(),
        "no temp file stays"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}
