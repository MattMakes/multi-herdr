//! A cancel of a task with a live attempt reaches `cancelled` through the
//! pass, and the pass reports no I1 item for the task while the cancel
//! waits (`docs/tickets/fleetd-finish/192-cancel-with-live-process-never-completes.md`).
//!
//! Each test uses a temp ledger and a temp fleet root, and runs the pass as
//! a library call. The agent is a real process in its own process group. A
//! thread of the test reaps it, as the pane of a real agent does.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PoolFacts, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::cancel;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

/// The most passes that a cancel may take.
const PASSES: i64 = 10;

/// A temp ledger at generation 1, a fleet root and 1 run.
struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = runs::create(&conn, &dir, &new_run).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
    std::fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        dir,
        _temp: temp,
        conn,
        run,
    }
}

impl World {
    fn settings(&self) -> Settings {
        let horch = self.dir.join("job.sh");
        if !horch.exists() {
            std::fs::write(&horch, "#!/bin/sh\nexec sleep 30\n").unwrap();
            horch_platform::fs::make_executable(&horch).unwrap();
        }
        let config = FleetConfig {
            stop_grace_s: 1,
            ..FleetConfig::default()
        };
        Settings {
            fleet_root: self.dir.join("fleet"),
            generation: Generation(1),
            config,
            horch,
            faults: Faults::default(),
        }
    }

    /// A `running` task of persona `dev`, bound to a worker with the agent
    /// `agent`: 1 live attempt. Gives the task id.
    fn running_task(&self, agent: Recorded) -> String {
        let mut new = NewTask::work("t", "tickets/t.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        bind_live(&self.conn, &task.task_id, &worker, Some(agent)).unwrap();
        let task = tasks::get(&self.conn, &task.task_id).unwrap();
        assert_eq!(task.status, "running");
        task.task_id
    }

    /// `horch ledger cancel` of `task`: its first transaction.
    fn cancel(&mut self, task: &str) {
        let tx = self.conn.transaction().unwrap();
        let t = tasks::get(&tx, task).unwrap();
        let out = cancel::request(&tx, &t, "the operator dropped it").unwrap();
        assert!(matches!(out, cancel::Cancel::Stopping { .. }), "{out:?}");
        tx.commit().unwrap();
    }

    fn status(&self, task: &str) -> String {
        tasks::get(&self.conn, task).unwrap().status
    }

    /// The I1 items of `task` that the pass opened, open or not.
    fn i1_items(&self, task: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT text FROM needs_you
                 WHERE kind = 'invariant_breach' AND entity_id = ?1 AND key LIKE '%:I1:%'",
            )
            .unwrap();
        stmt.query_map([task], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// 1 pass at the fleet clock `clock`.
    fn pass_at(&mut self, clock: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
            .unwrap();
        let ports = Ports {
            outside: &NoPanes,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host: &Launcher,
        };
        let report = Pass::new(self.settings(), ports)
            .run(&mut self.conn)
            .unwrap();
        assert!(report.failed.is_empty(), "{report:?}");
    }

    /// Passes 1 s of fleet clock apart, until `task` is `cancelled` or
    /// [`PASSES`] passes ran. Each I1 item of the task fails the test.
    fn passes_until_cancelled(&mut self, task: &str) {
        for n in 1..=PASSES {
            self.pass_at(n);
            let items = self.i1_items(task);
            assert!(items.is_empty(), "pass {n}: {items:?}");
            if self.status(task) == "cancelled" {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("{task} is {} after {PASSES} passes", self.status(task));
    }
}

/// An agent process in its own process group. A thread reaps it when it
/// exits; `exited` gets its exit then.
struct Agent {
    recorded: Recorded,
    exited: mpsc::Receiver<()>,
}

impl Agent {
    fn run(script: &str) -> Agent {
        let mut child = Command::new("/bin/sh")
            .args(["-c", script])
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let recorded = Recorded {
            pid,
            pid_start: horch_platform::process::start_time(pid),
        };
        let (tx, exited) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = child.wait();
            let _ = tx.send(());
        });
        // The shell starts its children first.
        std::thread::sleep(Duration::from_millis(300));
        Agent { recorded, exited }
    }

    /// Whether it exited and was reaped within 10 s.
    fn exits(&self) -> bool {
        self.exited.recv_timeout(Duration::from_secs(10)).is_ok()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if self.exited.try_recv().is_err() {
            // SAFETY: a signal to the process group that this test started.
            unsafe { libc::kill(-(self.recorded.pid as i32), libc::SIGKILL) };
        }
    }
}

/// The outside of the pass: no herdr snapshot, no pools, no launch.
struct NoPanes;

impl Outside for NoPanes {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no launch of {} in these tests", attempt.id)
    }

    fn pools(&self) -> Option<PoolFacts> {
        None
    }
}

/// The agent runs at the cancel: the pass stops it, sees it gone, and
/// moves the task to `cancelled`.
#[test]
fn a_cancel_of_a_task_with_a_live_agent_completes() {
    let mut w = world();
    let agent = Agent::run("while :; do sleep 0.1; done");
    let task = w.running_task(agent.recorded);

    w.cancel(&task);
    w.passes_until_cancelled(&task);

    assert!(agent.exits(), "the pass stopped the agent");
}

/// The agent ignores the polite stop: the kill of the stop ladder ends it.
#[test]
fn a_cancel_of_a_task_whose_agent_ignores_the_polite_stop_completes() {
    let mut w = world();
    let agent = Agent::run("trap '' TERM INT HUP; while :; do sleep 0.1; done");
    let task = w.running_task(agent.recorded);

    w.cancel(&task);
    w.passes_until_cancelled(&task);

    assert!(agent.exits(), "the pass killed the agent");
}

/// The agent already finished before the cancel: the pass sees it gone and
/// moves the task to `cancelled`.
#[test]
fn a_cancel_of_a_task_whose_agent_already_finished_completes() {
    let mut w = world();
    let agent = Agent::run("exit 0");
    let task = w.running_task(agent.recorded);
    assert!(agent.exits());

    w.cancel(&task);
    w.passes_until_cancelled(&task);
}
