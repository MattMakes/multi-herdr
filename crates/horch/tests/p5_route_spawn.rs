//! `horch spawn` changes no row itself: each change is 1 request to fleetd
//! (RR-5, FM-24; ticket 23 of fleetd-p5).
//!
//! Each test runs the development `horch` with its own ledger, state, data
//! and fleet folders, and serves the channel from this test process: the
//! in-process channel server of ticket 13 with the worker handlers of
//! ticket 19, or 1 server that loses its reply. The `fleetd` row names this
//! process, so the client accepts the server.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::{socket_path, ChannelServer};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::parse_request;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::CheckSandboxes;
use horch_core::fleetd::task_execution::domain::dispatch::Holds;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, WorktreeMode};
use horch_core::taskledger::orchestrator::Holder;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";

const HEALTHY_MACHINE: &str = r#"{"os":"macos","arch":"arm64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// A ledger at generation 1 whose `fleetd` row is this process, the
/// operator token, and the active run `r1` of the project folder.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run_id: String,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "fleet", "data", "state"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("machine.json"), HEALTHY_MACHINE).unwrap();
        let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
            params![pid, start as i64, util::now()],
        )
        .unwrap();
        let run = runs::create(
            &conn,
            &root,
            &NewRun {
                slug: "r1".into(),
                base_branch: Some("main".into()),
                ..Default::default()
            },
        )
        .unwrap();
        token_file::ensure(&root.join("fleet")).unwrap();
        World {
            _tmp: tmp,
            root,
            run_id: run.run_id,
        }
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.ledger()).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        conn
    }

    /// A ready shared-checkout task of `persona`, so no git tree is needed.
    fn task(&self, title: &str, persona: &str) -> String {
        let conn = self.conn();
        let run = runs::get(&conn, &self.run_id).unwrap();
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), persona);
        new.worktree_mode = WorktreeMode::Shared;
        new.ticket_snapshot = Some(format!("# {title}\n"));
        tasks::add(&conn, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn handlers(&self) -> Handlers {
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
        handlers
    }

    /// The in-process channel server of the fleet.
    fn fleetd(&self) -> ChannelServer {
        ChannelServer::start(&self.fleet(), self.conn(), Generation(1), self.handlers()).unwrap()
    }

    fn horch(&self, args: &[&str], env: &[(&str, &str)]) -> Out {
        let root = &self.root;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", root)
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("HORCH_HERDR_BIN", root.join("no-herdr"))
            .env("HORCH_NO_WATCHDOG", "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// Each `requests` row: its kind and its state.
    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The attempts of `task`: each id, state and worker.
    fn attempts(&self, task: &str) -> Vec<(String, String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, state, worker_id FROM attempts WHERE task_id = ?1 ORDER BY id")
            .unwrap();
        stmt.query_map([task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn task_row(&self, task: &str) -> (String, Option<String>) {
        let t = tasks::get(&self.conn(), task).unwrap();
        (t.status, t.current_worker_id)
    }

    /// The names of the files in the request folder.
    fn request_files(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.fleet().join("run").join("requests")) else {
            return Vec::new();
        };
        entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}

/// The rows of a bind of today: the task is leased to the worker of the
/// reply, with 1 push attempt in `launching` that fleetd launches.
fn assert_bound(w: &World, task: &str, out: &Out) {
    let attempts = w.attempts(task);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    let (_, state, worker) = &attempts[0];
    assert_eq!(state, "launching");
    assert_eq!(
        out.out.trim(),
        worker,
        "stdout names the worker of the bind"
    );
    assert_eq!(w.task_row(task), ("leased".into(), Some(worker.clone())));
    assert!(out.err.contains("horch spawn: bound"), "{}", out.err);
    assert!(out.err.contains("fleetd opens the pane"), "{}", out.err);
}

#[test]
fn spawn_sends_1_spawn_request_and_binds_through_fleetd() {
    let w = World::new();
    let task = w.task("a", "opus");
    let server = w.fleetd();
    let out = w.horch(&["spawn", "opus", "--run", "r1"], &[]);
    server.stop().unwrap();
    assert_eq!(out.code, 0, "stdout: {}\nstderr: {}", out.out, out.err);
    assert_eq!(w.requests(), [("spawn".into(), "applied".into())]);
    assert_bound(&w, &task, &out);
    assert!(w.request_files().is_empty(), "{:?}", w.request_files());

    // A refused request prints its reason and exits 1, and binds nothing.
    let server = w.fleetd();
    let out = w.horch(&["spawn", "opus", "--run", "r1"], &[]);
    server.stop().unwrap();
    assert_eq!(out.code, 1, "stdout: {}\nstderr: {}", out.out, out.err);
    assert!(out.err.contains("refused: "), "{}", out.err);
    assert!(
        out.err.contains("no ready task for persona opus"),
        "{}",
        out.err
    );
    assert_eq!(
        w.requests(),
        [
            ("spawn".into(), "applied".into()),
            ("spawn".into(), "refused".into())
        ]
    );
}

/// 1 server that applies the first request and closes the connection with
/// no reply: the reply is lost.
fn lose_the_reply(w: &World) -> std::thread::JoinHandle<()> {
    let socket = socket_path(&w.fleet());
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let mut conn = w.conn();
    let handlers = w.handlers();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = Vec::new();
        BufReader::new(&stream)
            .read_until(b'\n', &mut line)
            .unwrap();
        let request = parse_request(&line, 65_536).unwrap();
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        handle(
            &mut conn,
            Generation(1),
            &request,
            Some("test"),
            &decision,
            &handlers,
        )
        .unwrap();
        drop(stream);
        drop(listener);
        std::fs::remove_file(&socket).unwrap();
    })
}

#[test]
fn spawn_retry_after_a_lost_reply_binds_1_time() {
    let w = World::new();
    let task = w.task("a", "opus");
    let server = lose_the_reply(&w);
    let out = w.horch(&["spawn", "opus", "--run", "r1"], &[]);
    server.join().unwrap();
    assert_eq!(out.code, 0, "stdout: {}\nstderr: {}", out.out, out.err);
    assert_eq!(w.requests(), [("spawn".into(), "applied".into())]);
    assert_bound(&w, &task, &out);
    assert!(
        w.request_files().is_empty(),
        "the client read the stored result and sent no second request: {:?}",
        w.request_files()
    );
}

#[test]
fn ledger_spawn_sends_spawn() {
    let w = World::new();
    let first = w.task("a", "opus");
    let target = w.task("b", "opus");
    let server = w.fleetd();
    let run_id = w.run_id.clone();
    let out = w.horch(
        &["spawn", "opus"],
        &[("HORCH_RUN_ID", &run_id), ("HORCH_CLAIM_TARGET", &target)],
    );
    server.stop().unwrap();
    assert_eq!(out.code, 0, "stdout: {}\nstderr: {}", out.out, out.err);
    assert_eq!(w.requests(), [("spawn".into(), "applied".into())]);
    assert_bound(&w, &target, &out);
    assert!(w.attempts(&first).is_empty(), "the target binds first");
    assert_eq!(w.task_row(&first), ("ready".into(), None));
}
