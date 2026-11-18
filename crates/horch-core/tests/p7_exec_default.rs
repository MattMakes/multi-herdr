//! Exec mode is the default worker mode (fleetd-p7 ticket 19; FR-6,
//! FM-18; `docs/tickets/fleetd-p7/MAP.md`, Q3 and Q5).
//!
//! - A profile whose file names no mode runs exec when its CLI has an exec
//!   adapter, and interactive otherwise.
//! - The orchestrator seats, the top-tier seats and `idea-lead` stay
//!   interactive.
//! - An attempt keeps the mode of its launch: it is exec only when its
//!   attempt folder holds the exec stream `transcript.jsonl`.
//! - An exec attempt has no background wait. An interactive attempt keeps
//!   it.
//!
//! Each test uses a temporary ledger and fleet root. No test starts an
//! agent CLI.

use std::path::{Path, PathBuf};

use horch_core::execution::lifecycle::TRANSCRIPT_JSONL;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::worker_profiles::domain::profile::Mode;
use horch_core::harness::HarnessKind;
use horch_core::roster::Roster;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker, BACKGROUND_WAIT};
use horch_core::taskledger::Store;

fn builtin_mode(name: &str) -> Mode {
    Roster::builtin()
        .unwrap()
        .require(name)
        .unwrap()
        .profile()
        .mode
}

#[test]
fn mode_absent_is_exec_in_phase_7() {
    let roster = Roster::builtin().unwrap();
    for name in ["opus", "codex-sol", "antigravity"] {
        let t = roster.require(name).unwrap();
        assert!(t.mode.is_none(), "{name} names a mode");
        assert_eq!(t.profile().mode, Mode::Exec, "{name}");
    }
}

#[test]
fn opencode_pi_and_prime_default_to_interactive() {
    let roster = Roster::builtin().unwrap();
    let mut seen = 0;
    for name in roster.names() {
        let t = roster.require(name).unwrap();
        if matches!(
            t.agent,
            HarnessKind::OpenCode | HarnessKind::Pi | HarnessKind::Prime
        ) {
            seen += 1;
            assert_eq!(t.profile().mode, Mode::Interactive, "{}", t.name);
        }
    }
    assert!(seen >= 3, "only {seen} OpenCode, pi or Prime teammates");
}

#[test]
fn orchestrator_seats_and_idea_lead_are_interactive() {
    for name in [
        "orchestrator",
        "orchestrator-codex",
        "fable-creative",
        "astra-creative",
        "idea-lead",
    ] {
        assert_eq!(builtin_mode(name), Mode::Interactive, "{name}");
    }
}

/// A temporary ledger with 1 claimed worker of `opus` and its attempt.
struct Fleet {
    _dir: tempfile::TempDir,
    store: Store,
    worker: String,
    attempt_dir: PathBuf,
}

impl Fleet {
    fn new() -> Fleet {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (worker, task_id) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "exec".into(),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                let task = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("t", "t.md", "opus"),
                    Actor::Orchestrator,
                )?;
                let worker = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("worker-1".into()),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                tasks::claim(tx, &worker)?;
                Ok((worker, task.task_id))
            })
            .unwrap();
        let attempt: String = store
            .conn()
            .query_row(
                "SELECT id FROM attempts WHERE worker_id = ?1 AND task_id = ?2",
                [&worker, &task_id],
                |r| r.get(0),
            )
            .unwrap();
        let n = attempt
            .rsplit_once('/')
            .map_or(attempt.as_str(), |(_, a)| a);
        // The fleet root of a ledger with no `FLEET_ROOT`: `fleet` beside it.
        let attempt_dir = attempt_dir(&dir.path().join("fleet"), &task_id, n);
        std::fs::create_dir_all(&attempt_dir).unwrap();
        Fleet {
            _dir: dir,
            store,
            worker,
            attempt_dir,
        }
    }

    /// The wrapper opened the exec stream of the attempt.
    fn exec_stream(&self) {
        std::fs::write(self.attempt_dir.join(TRANSCRIPT_JSONL), "").unwrap();
    }

    fn declare_background_wait(&self) {
        workers::set_wait(
            self.store.conn(),
            &self.worker,
            BACKGROUND_WAIT,
            "build",
            &[],
            3600,
        )
        .unwrap();
    }

    fn background_waits(&self) -> usize {
        workers::waiting(self.store.conn(), &self.worker)
            .unwrap()
            .iter()
            .filter(|w| w.kind == BACKGROUND_WAIT)
            .count()
    }
}

#[test]
fn background_wait_is_refused_for_an_exec_attempt() {
    let f = Fleet::new();
    f.exec_stream();
    assert_eq!(workers::attempt_mode(f.store.conn(), &f.worker), Mode::Exec);
    f.declare_background_wait();
    assert_eq!(f.background_waits(), 0);
}

#[test]
fn background_wait_still_holds_for_an_interactive_attempt() {
    let f = Fleet::new();
    assert_eq!(
        workers::attempt_mode(f.store.conn(), &f.worker),
        Mode::Interactive
    );
    f.declare_background_wait();
    assert_eq!(f.background_waits(), 1);
}

#[test]
fn live_interactive_attempt_keeps_its_mode_after_the_update() {
    // The attempt launched interactive, before the update: no exec stream.
    let f = Fleet::new();
    f.declare_background_wait();
    // After the update, the profile of its persona is exec.
    assert_eq!(builtin_mode("opus"), Mode::Exec);
    // The live attempt keeps its interactive treatment and its wait.
    assert_eq!(
        workers::attempt_mode(f.store.conn(), &f.worker),
        Mode::Interactive
    );
    assert_eq!(f.background_waits(), 1);
    f.declare_background_wait();
    assert_eq!(f.background_waits(), 1);
}
