#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::{DutyEffects, WatchdogDuties};
use horch_core::fleetd::fleet_runtime::application::pass::{
    ActCtx, DutyBudget, DutyCtx, RunDuties,
};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::task_execution::domain::task::Facts;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::orchestrator;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::watchdog::{Observation, Observer};
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";

const NEW_COMMAND: &str = "horch idea add";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run: Run,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::Builder::new()
            .prefix("p6f")
            .tempdir_in("/tmp")
            .unwrap();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
        for dir in ["proj", "home", "state", "data", "f"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let herdr = root.join("herdr");
        std::fs::write(
            &herdr,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{calls}'\nprintf '%s\\n' '{reply}'\n",
                calls = root.join("herdr-calls.txt").display(),
                reply = "{\"result\":{\"workspace\":{\"workspace_id\":\"w1\"},\
                         \"root_pane\":{\"pane_id\":\"p1\",\"workspace_id\":\"w1\"},\
                         \"pane\":{\"pane_id\":\"p1\",\"workspace_id\":\"w1\"},\
                         \"workspaces\":[{\"workspace_id\":\"w1\"}],\"panes\":[]}}",
            ),
        )
        .unwrap();
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        let store = Store::open(&root.join("l.db")).unwrap();
        let run = runs::create(
            store.conn(),
            &root.join("proj"),
            &NewRun {
                slug: "r1".into(),
                ..Default::default()
            },
        )
        .unwrap();
        World {
            _tmp: tmp,
            root,
            run,
        }
    }

    fn db(&self) -> PathBuf {
        self.root.join("l.db")
    }

    fn ledger(&self) -> Connection {
        Connection::open(self.db()).unwrap()
    }

    fn horch(&self, args: &[&str]) -> (i32, String, String) {
        let root = &self.root;
        let o = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(root.join("proj"))
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.db())
            .env("HORCH_PROJECT_DIR", root.join("proj"))
            .env("HORCH_HERDR_BIN", root.join("herdr"))
            .env("FLEET_ROOT", root.join("f"))
            .env("HORCH_NO_WATCHDOG", "1")
            .env("HORCH_NO_UPDATE", "1")
            .output()
            .unwrap();
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    }

    fn herdr_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("herdr-calls.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn orchestrators(&self) -> i64 {
        self.ledger()
            .query_row(
                "SELECT count(*) FROM workers WHERE kind = 'orchestrator'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
}

#[test]
fn fleet_start_launches_no_orchestrator() {
    let w = World::new();
    let proj = w.root.join("proj");
    let (code, out, err) = w.horch(&["fleet", "--no-update", "--cwd", &proj.to_string_lossy()]);
    assert_eq!(code, 0, "horch fleet: {out}{err}");
    assert!(
        out.contains("Add an idea: horch idea add \"...\""),
        "the start names the new command: {out}"
    );
    let calls = w.herdr_calls();
    assert!(
        calls.iter().any(|c| c.starts_with("workspace create")),
        "the start creates the herdr workspace: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.contains("pane-launch")),
        "no pane runs an orchestrator: {calls:?}"
    );
    assert!(
        !out.contains("Launching orchestrator") && !out.contains("orchestrator pane"),
        "the start launches no orchestrator: {out}{err}"
    );
    assert_eq!(w.orchestrators(), 0, "no orchestrator row");
}

#[test]
fn orchestrator_claim_is_refused_with_the_new_command() {
    let w = World::new();

    let mut conn = w.ledger();
    let tx = conn.transaction().unwrap();
    let err = orchestrator::claim(&tx, &w.run.run_id, "w1", Some("p1"), &DeadOrchestrator)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(NEW_COMMAND),
        "the claim names the new command: {err}"
    );
    drop(tx);

    let err = orchestrator::launch_run().unwrap_err().to_string();
    assert!(
        err.contains(NEW_COMMAND),
        "the launch names the new command: {err}"
    );

    let (code, out, err) = w.horch(&[
        "pane-launch",
        "--role",
        "orchestrator",
        "--kind",
        "fleet-orchestrator",
        "--model",
        "opus",
    ]);
    assert_ne!(code, 0, "the pane launch is refused: {out}{err}");
    assert!(
        err.contains(NEW_COMMAND),
        "the refusal names the new command: {err}"
    );
    assert_eq!(w.orchestrators(), 0, "no orchestrator row");
    assert_eq!(
        w.run_workspace(),
        None,
        "the refused claim moves the run to no workspace"
    );
}

impl World {
    fn run_workspace(&self) -> Option<String> {
        self.ledger()
            .query_row(
                "SELECT workspace_id FROM runs WHERE run_id = ?1",
                [&self.run.run_id],
                |r| r.get(0),
            )
            .unwrap()
    }
}

struct DeadOrchestrator;

impl orchestrator::Probe for DeadOrchestrator {
    fn holder(&self, _workspace: &str) -> Option<orchestrator::Holder> {
        None
    }

    fn alive(&self, _pid: u32, _started: Option<u64>) -> bool {
        false
    }

    fn workspace_open(&self, _workspace: &str) -> Option<bool> {
        Some(true)
    }
}

struct Quiet;

impl Observer for Quiet {
    fn observe(&self, _w: &Worker) -> Observation {
        Observation {
            agent_alive: true,
            herdr_status: Some("idle".into()),
            ..Default::default()
        }
    }
}

struct NoEffects;

impl DutyEffects for NoEffects {
    fn deliver(&self, _run: &Run) {}

    fn retry_cleanups(&self) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[test]
fn no_respawn_duty_in_the_pass() {
    let w = World::new();
    let mut conn = w.ledger();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, 'now', 0, 'now')",
        params![i64::from(std::process::id())],
    )
    .unwrap();
    let old = workers::insert(
        &conn,
        &NewWorker {
            run_id: w.run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE workers SET status = 'working', agent_pid = 999999, agent_started = 1
         WHERE worker_id = ?1",
        [&old],
    )
    .unwrap();
    conn.execute(
        "UPDATE runs SET workspace_id = 'w1' WHERE run_id = ?1",
        [&w.run.run_id],
    )
    .unwrap();
    runs::set_orchestrator(&conn, &w.run.run_id, &old).unwrap();

    let duties = WatchdogDuties::new(&Quiet, &NoEffects);
    let facts = Facts::default();
    let time_up = || false;
    let budget = || DutyBudget {
        rows: PassBudget::new(100),
        time_up: &time_up,
    };
    for _ in 0..3 {
        duties.observe(&conn, budget()).unwrap();
        let tx = conn.transaction().unwrap();
        let ctx = DutyCtx {
            guard: Generation(1),
            clock: FleetSeconds(0),
            facts: &facts,
            budget: budget(),
        };
        duties.run(&tx, &ctx).unwrap();
        tx.commit().unwrap();
        let fleet = w.root.join("f");
        let act = ActCtx {
            fleet_root: Path::new(&fleet),
            clock: FleetSeconds(0),
        };
        let report = duties.act(&conn, &act);
        assert!(
            !report.done.iter().any(|d| d.contains("respawn")),
            "the pass respawns nothing: {report:?}"
        );
    }
    assert_eq!(
        w.orchestrators(),
        1,
        "no new orchestrator row takes its place"
    );
    let attempts: i64 = conn
        .query_row(
            "SELECT count(*) FROM attempts WHERE task_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 0, "no respawn attempt");
    let respawns: i64 = conn
        .query_row(
            "SELECT orchestrator_respawns FROM runs WHERE run_id = ?1",
            [&w.run.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(respawns, 0, "no respawn is counted");
}
