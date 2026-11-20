//! The `import` and `gc` commands of `horch ledger` send 1 request to fleetd
//! and write no row themselves (RR-5, FM-24; ticket 21 of fleetd-p5).
//!
//! Each test runs the development `horch` with temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and fleet root,
//! and the watchdog off. The test process plays fleetd: it writes the
//! `fleetd` row with its own PID and start time, makes the operator token,
//! and serves the channel in-process with the handlers of the run and plan
//! kinds. The client of the channel then accepts it as the fleetd of the
//! ledger.
//!
//! The tests are unix only: the socket path must be short.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_run;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";

const TICKET: &str = "# 01: Alpha task\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`\n\n## Goal\n\nThe secret words of the ticket.\n";
const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [Alpha task](01-a.md) | | 1 |\n";

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// A fleet in temporary folders. With [`World::up`], the test process
/// serves the channel as fleetd.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    server: Option<ChannelServer>,
}

impl World {
    /// The ledger, the `fleetd` row of this process and the token, and no
    /// server: fleetd is down.
    fn down() -> World {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let tmp = tempfile::Builder::new()
            .prefix("rl")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "fleet", "proj/tickets"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let w = World {
            _tmp: tmp,
            root,
            server: None,
        };
        let store = Store::open(&w.ledger()).unwrap();
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).expect("the start time of the test");
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                params![pid, i64::try_from(start).unwrap(), util::now()],
            )
            .unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        w
    }

    /// [`World::down`], then the channel served by this process.
    fn up() -> World {
        let mut w = World::down();
        let mut handlers = Handlers::new();
        handlers_run::register(&mut handlers);
        let conn = Connection::open(w.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        w.server = Some(ChannelServer::start(&w.fleet(), conn, Generation(1), handlers).unwrap());
        w
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.proj().join(rel), text).unwrap();
    }

    fn horch(&self, args: &[&str]) -> Out {
        let root = &self.root;
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_WORKSPACE_ID", "w1")
            .env("HORCH_NO_WATCHDOG", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `horch` with `args`, which must exit 0.
    fn ok(&self, args: &[&str]) -> Out {
        let o = self.horch(args);
        assert_eq!(
            o.code, 0,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        o
    }

    fn conn(&self) -> Connection {
        Connection::open(self.ledger()).unwrap()
    }

    /// The kind and state of each `requests` row, in the order of receipt.
    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY received_at, rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop();
        }
    }
}

fn applied(kinds: &[&str]) -> Vec<(String, String)> {
    kinds
        .iter()
        .map(|k| ((*k).to_string(), "applied".to_string()))
        .collect()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn add_map_dry_run_sends_nothing() {
    let w = World::up();
    w.write("tickets/01-a.md", TICKET);
    w.write("tickets/MAP.md", MAP);
    let o = w.ok(&["ledger", "add-map", "--dry-run", "tickets/MAP.md"]);
    assert!(o.out.contains("Nothing was queued"), "{}", o.out);
    assert!(w.requests().is_empty());
    assert_eq!(w.count("SELECT COUNT(*) FROM tasks"), 0);
}

#[test]
fn import_and_gc_send_their_kinds() {
    let w = World::up();
    git(&w.proj(), &["init", "-q", "-b", "main"]);
    git(
        &w.proj(),
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "c0",
        ],
    );
    let o = w.ok(&["ledger", "import"]);
    assert!(o.out.contains("records copied"), "{}", o.out);
    let o = w.ok(&["ledger", "gc"]);
    assert!(o.out.contains("No trash ref is older than"), "{}", o.out);
    assert_eq!(w.requests(), applied(&["ledger_import", "ledger_gc"]));
}

#[test]
fn fleetd_down_queues_a_request_file() {
    let w = World::down();
    git(&w.proj(), &["init", "-q", "-b", "main"]);
    let o = w.ok(&["ledger", "import"]);
    assert!(o.err.contains("fleetd is down: request "), "{}", o.err);
    assert!(o.err.contains("fleetd applies it at start"), "{}", o.err);
    let files: Vec<_> = std::fs::read_dir(w.fleet().join("run").join("requests"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let line = std::fs::read_to_string(&files[0]).unwrap();
    assert!(line.contains("\"ledger_import\""), "{line}");
    assert!(w.requests().is_empty());
}
