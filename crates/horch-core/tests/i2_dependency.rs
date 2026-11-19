//! I2, Phase 4 part: no task is `ready` while a dependency is not met. A
//! normal dependency is met when its task is `done` and its task branch is
//! `landed`. A dependency with no branch row keeps the rule of today:
//! `done` (docs/specs/fleetd/design.md, I2, "The task state machine";
//! docs/tickets/fleetd-p4/26-i2-dependency-met-when-landed.md).
//!
//! Each test has its own temporary ledger and fleet root. No test reads the
//! load, the limits or the clock of the machine: the fleet clock is the
//! `fleetd` row, and the config is the default.

use std::collections::BTreeMap;

use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewBranch, NewFeature};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";

struct World {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    conn: Connection,
    run: Run,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
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
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some("fleet/r1"),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    tx.commit().unwrap();
    let root = dir.join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    World {
        _temp: temp,
        root,
        conn,
        run,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

impl World {
    /// A new task `title`; it waits for each task of `after`.
    fn task(&self, title: &str, after: &[&str]) -> String {
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        new.blocked_by = after.iter().map(|s| s.to_string()).collect();
        tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    /// The task `task` is `done` (verified).
    fn done(&self, task: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [task],
            )
            .unwrap();
    }

    /// The task branch of `task`, in `building`.
    fn branch(&mut self, task: &str) -> String {
        let name = format!("fleet/r1/{task}");
        let new = NewBranch {
            name: &name,
            feature_id: FEATURE,
            task_id: Some(task),
            owner: "worker",
            due_at: 100,
        };
        let tx = self.conn.transaction().unwrap();
        rows::insert_branch(&tx, None, &new, cause("task bound")).unwrap();
        tx.commit().unwrap();
        name
    }

    fn set_landing(&self, branch: &str, state: &str) {
        self.conn
            .execute(
                "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
                params![branch, state],
            )
            .unwrap();
    }

    fn status(&self, task: &str) -> String {
        tasks::get(&self.conn, task).unwrap().status
    }

    fn pass(&mut self) {
        let settings = Settings {
            fleet_root: self.root.clone(),
            generation: Generation(1),
            config: FleetConfig::default(),
            horch: "/bin/false".into(),
            faults: Faults::default(),
        };
        let ports = Ports {
            outside: &Quiet,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host: &Launcher,
        };
        Pass::new(settings, ports).run(&mut self.conn).unwrap();
    }
}

/// An outside with no pane, no hold and no folder.
struct Quiet;

impl Outside for Quiet {
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

    fn attempt_spec(&self, _attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no attempt starts in this test")
    }
}

#[test]
fn dependent_of_a_done_but_unlanded_task_stays_pending() {
    let mut w = world();
    let dep = w.task("a", &[]);
    let dependent = w.task("b", &[&dep]);
    let branch = w.branch(&dep);
    w.set_landing(&branch, "verifying");
    w.done(&dep);

    w.pass();
    assert_eq!(w.status(&dependent), "pending");

    w.set_landing(&branch, "in_bundle");
    w.pass();
    assert_eq!(w.status(&dependent), "pending");
}

#[test]
fn dependent_of_a_landed_task_becomes_ready() {
    let mut w = world();
    let dep = w.task("a", &[]);
    let dependent = w.task("b", &[&dep]);
    let branch = w.branch(&dep);
    w.done(&dep);
    w.set_landing(&branch, "landed");

    w.pass();
    assert_eq!(w.status(&dependent), "ready");
}

#[test]
fn dependency_without_a_branch_row_keeps_the_done_rule() {
    let mut w = world();
    let open = w.task("a", &[]);
    let dependent = w.task("b", &[&open]);
    w.pass();
    assert_eq!(w.status(&dependent), "pending");

    w.done(&open);
    w.pass();
    assert_eq!(w.status(&dependent), "ready");
}
