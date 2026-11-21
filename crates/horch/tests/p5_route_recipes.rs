#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";
const WORKSPACE: &str = "w1";
const PANE: &str = "p1";

struct World {
    server: Option<ChannelServer>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run: Run,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::Builder::new()
            .prefix("rr")
            .tempdir_in("/tmp")
            .unwrap();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
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
        };
        let mut handlers = Handlers::new();
        register(&mut handlers, &deps);
        let conn = Connection::open(self.db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        self.server =
            Some(ChannelServer::start(&self.fleet(), conn, Generation(1), handlers).unwrap());
    }

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
