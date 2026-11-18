//! `horch fleet stop` sends 1 request to fleetd, the pane of a fleet
//! orchestrator sends none (FM-16), and `horch sessions` opens the ledger
//! read-only (RR-5, FM-24; ticket 24 of fleetd-p5).
//!
//! Each test runs the development binary with its own ledger, state, data
//! and fleet folders, and the watchdog off. The fleetd of a test is the
//! channel server of ticket 13 in a thread of the test process, with the
//! handlers of ticket 19: the `fleetd` row names the test process.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::CheckSandboxes;
use horch_core::fleetd::task_execution::domain::dispatch::Holds;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::orchestrator::Holder;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";
const WORKSPACE: &str = "w1";
const PANE: &str = "p1";

struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

/// A project, a ledger with 1 active run, a fleet root with its token, a
/// fake herdr, and (after [`World::fleetd`]) a fleetd that serves the
/// channel.
struct World {
    // The server drops first: its stop needs the folders of `_tmp`.
    server: Option<ChannelServer>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run: Run,
}

impl World {
    fn new() -> World {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let tmp = tempfile::Builder::new()
            .prefix("rr")
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["proj", "home", "state", "data"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let fleet = root.join("f");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::set_permissions(&fleet, std::fs::Permissions::from_mode(0o700)).unwrap();
        token_file::ensure(&fleet).unwrap();
        let herdr = root.join("herdr");
        std::fs::write(
            &herdr,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"result\":{{\"pane\":{{\"pane_id\":\"{PANE}\",\"workspace_id\":\"{WORKSPACE}\"}},\"workspaces\":[],\"panes\":[]}}}}'\n"
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
            server: None,
            _tmp: tmp,
            root,
            run,
        }
    }

    fn db(&self) -> PathBuf {
        self.root.join("l.db")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("f")
    }

    fn ledger(&self) -> Connection {
        Connection::open(self.db()).unwrap()
    }

    /// Start the fleetd of this test: the `fleetd` row names this process at
    /// generation 1, and the channel server applies the worker kinds.
    fn fleetd(&mut self) {
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        self.ledger()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, 'now', 0, 'now')",
                params![i64::from(pid), i64::try_from(start).unwrap()],
            )
            .unwrap();
        let deps = WorkerDeps {
            guard: Generation(1),
            config: Arc::new(FleetConfig::default()),
            needs_you: Arc::new(NeedsYouList),
            git: Arc::new(Git::new("git")),
            holds: Arc::new(Holds::default),
            sandboxes: Arc::new(AllSandboxes),
            lock_holder: Arc::new(|_: &str| None::<Holder>),
        };
        let mut handlers = Handlers::new();
        register(&mut handlers, &deps);
        let conn = Connection::open(self.db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        self.server =
            Some(ChannelServer::start(&self.fleet(), conn, Generation(1), handlers).unwrap());
    }

    /// `horch <args>` in the project, with `extra` env.
    fn horch(&self, args: &[&str], extra: &[(&str, &Path)]) -> (i32, String, String) {
        let root = &self.root;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
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
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_NO_WATCHDOG", "1");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let o = cmd.output().unwrap();
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    }

    /// The kind and state of each `requests` row, oldest first.
    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.ledger();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn run_column(&self, column: &str) -> Option<String> {
        self.ledger()
            .query_row(
                &format!("SELECT {column} FROM runs WHERE run_id = ?1"),
                [&self.run.run_id],
                |r| r.get(0),
            )
            .unwrap()
    }
}

fn applied(kind: &str) -> Vec<(String, String)> {
    vec![(kind.to_string(), "applied".to_string())]
}

#[test]
fn fleet_stop_sends_orchestrator_stop() {
    let mut w = World::new();
    w.fleetd();
    let (code, out, err) = w.horch(&["fleet", "stop"], &[]);
    assert_eq!(code, 0, "horch fleet stop: {out}{err}");
    assert_eq!(
        out,
        "Run r1: the orchestrator stop is recorded. horch does not respawn it.\n"
    );
    assert_eq!(w.requests(), applied("orchestrator_stop"));
    assert!(w.run_column("orchestrator_stopped_at").is_some());

    // A second stop keeps the first time, and says so.
    let (code, out, err) = w.horch(&["fleet", "stop"], &[]);
    assert_eq!(code, 0, "horch fleet stop: {out}{err}");
    assert_eq!(out, "Run r1: the orchestrator stop was recorded before.\n");
}

#[test]
fn fleet_stop_with_fleetd_down_queues_the_request() {
    let w = World::new();
    let (code, out, err) = w.horch(&["fleet", "stop"], &[]);
    assert_eq!(code, 0, "horch fleet stop: {out}{err}");
    assert!(err.contains("fleetd is down: request"), "{err}");
    assert!(w.requests().is_empty());
    assert!(w.run_column("orchestrator_stopped_at").is_none());
    let queued = std::fs::read_dir(w.fleet().join("run/requests"))
        .unwrap()
        .count();
    assert_eq!(queued, 1);
}

/// FM-16 (ticket 45 of fleetd-p6): the pane of a fleet orchestrator is
/// refused before it sends the `orchestrator_claim` request.
#[test]
fn fleet_orchestrator_pane_sends_no_orchestrator_claim() {
    let mut w = World::new();
    w.fleetd();
    let (code, out, err) = w.horch(
        &[
            "pane-launch",
            "--role",
            "orchestrator",
            "--kind",
            "fleet-orchestrator",
            "--model",
            "opus",
        ],
        &[("HERDR_PANE_ID", Path::new(PANE))],
    );
    assert_ne!(code, 0, "{out}{err}");
    assert!(err.contains("horch idea add"), "{out}{err}");
    assert_eq!(w.requests(), Vec::<(String, String)>::new(), "{out}{err}");
    assert_eq!(w.run_column("workspace_id"), None);
}

#[test]
fn sessions_opens_the_ledger_read_only() {
    let w = World::new();
    {
        let store = Store::open(&w.db()).unwrap();
        workers::insert(
            store.conn(),
            &NewWorker {
                run_id: w.run.run_id.clone(),
                persona: "opus".into(),
                role: Some("opus-1".into()),
                workspace_id: Some(WORKSPACE.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    // A writable open sets the WAL journal mode again; a read-only open
    // cannot.
    {
        let conn = w.ledger();
        let mode: String = conn
            .query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }
    let (code, out, err) = w.horch(&["sessions", "--all"], &[]);
    assert_eq!(code, 0, "horch sessions: {out}{err}");
    assert!(out.contains("opus-1"), "{out}{err}");
    let mode: String = w
        .ledger()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        mode, "delete",
        "horch sessions opened the ledger for a write"
    );
}
