#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util, Store};

const PATH: &str = "/usr/bin:/bin";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
        let w = World { _tmp: tmp, root };
        std::fs::create_dir_all(w.proj()).unwrap();
        std::fs::create_dir_all(w.root.join("home")).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        std::fs::write(w.proj().join("t1.md"), "# Task 1\n").unwrap();
        w.git(&["add", "t1.md"]);
        w.git(&["commit", "-q", "-m", "init"]);
        let ticket = w.proj().join("t1.md");
        let task = NewTask::work("Task 1", &ticket.to_string_lossy(), "dev");
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let mut store = Store::open(&w.ledger()).unwrap();
        store
            .immediate(|tx| {
                let run = runs::create(tx, &w.proj(), &new)?;
                schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                tasks::add(tx, &run, &task, Actor::Orchestrator)
            })
            .unwrap();
        w
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn trace(&self) -> PathBuf {
        self.root.join("sql-trace.txt")
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", self.root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "n")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "n")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    fn read(&self, args: &[&str]) -> String {
        let before = self.files();
        let root = &self.root;
        let o = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1")
            .env("HORCH_TEST_ASSERT_READ_ONLY", "1")
            .env("HORCH_TEST_SQL_TRACE", self.trace())
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            matches!(o.status.code(), Some(0 | 1)) && !out.is_empty(),
            "horch {args:?} exited {:?}: {out}{err}",
            o.status.code()
        );
        assert_eq!(
            std::fs::read_to_string(self.trace()).ok(),
            None,
            "horch {args:?} ran a statement that is not a read"
        );
        assert!(
            before == self.files(),
            "horch {args:?} changed the ledger files"
        );
        out
    }

    /// The bytes of the ledger and of its WAL. A missing WAL and an empty
    /// one are the same: a read-only connection opens the WAL and SQLite
    /// creates the file empty. The shm is left out: a reader writes its
    /// read mark there.
    fn files(&self) -> Vec<Option<Vec<u8>>> {
        let main = std::fs::read(self.ledger()).ok();
        let wal = std::fs::read(path_with(&self.ledger(), "-wal"))
            .ok()
            .filter(|b| !b.is_empty());
        vec![main, wal]
    }

    fn status_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.read(&["ledger", "status", "--json"])).unwrap()
    }
}

fn path_with(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn assert_board(w: &World) {
    let st = w.status_json();
    assert_eq!(st["run"]["slug"], "r1", "{st}");
    assert_eq!(st["tasks"].as_array().map(Vec::len), Some(1), "{st}");
}

fn live_writer(w: &World) -> rusqlite::Connection {
    let c = rusqlite::Connection::open(w.ledger()).unwrap();
    c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    c.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('px', '/px', 'now')",
        [],
    )
    .unwrap();
    c
}

#[test]
fn status_writes_nothing_with_no_wal_file() {
    let w = World::new();
    assert!(!path_with(&w.ledger(), "-wal").exists());
    assert_board(&w);
}

#[test]
fn status_writes_nothing_while_a_writer_holds_frames_that_are_not_checkpointed() {
    let w = World::new();
    let writer = live_writer(&w);
    let wal = std::fs::metadata(path_with(&w.ledger(), "-wal")).unwrap();
    assert!(wal.len() > 0, "the WAL holds frames");
    assert_board(&w);
    drop(writer);
}

#[test]
fn status_writes_nothing_with_a_wal_and_no_shm() {
    let w = World::new();
    let writer = live_writer(&w);
    let mut keep: std::os::raw::c_int = 1;
    // SAFETY: the handle is the open connection of `writer`, and `keep`
    // lives for the call.
    unsafe {
        rusqlite::ffi::sqlite3_file_control(
            writer.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,
            (&mut keep as *mut std::os::raw::c_int).cast(),
        );
    }
    writer
        .execute(
            "INSERT INTO projects (project_id, root, created_at) VALUES ('py', '/py', 'now')",
            [],
        )
        .unwrap();
    drop(writer);
    let _ = std::fs::remove_file(path_with(&w.ledger(), "-shm"));
    assert!(path_with(&w.ledger(), "-wal").exists());
    assert_board(&w);
}

#[test]
fn status_writes_nothing_on_a_schema_1_version_lower() {
    let w = World::new();
    let c = rusqlite::Connection::open(w.ledger()).unwrap();
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    c.pragma_update(None, "user_version", v - 1).unwrap();
    drop(c);
    assert_board(&w);
}

#[test]
fn each_reader_command_of_the_ledger_cli_writes_nothing() {
    let w = World::new();
    let _writer = live_writer(&w);
    for args in [
        &["ledger", "status"][..],
        &["ledger", "status", "--json"],
        &["ledger", "show", "T-1"],
        &["ledger", "show", "T-1", "--json"],
        &["ledger", "events"],
        &["ledger", "events", "--json"],
        &["ledger", "run", "list"],
        &["ledger", "run", "list", "--json"],
        &["ledger", "unfinished"],
        &["ledger", "unfinished", "--json"],
        &["ledger", "path"],
    ] {
        w.read(args);
    }
}

fn waiter(w: &World, seconds: i64, on_task: bool) -> String {
    use horch_core::taskledger::testing::bind_live;
    use horch_core::taskledger::workers::{self, NewWorker, REVIEWS_WAIT};
    let mut store = Store::open(&w.ledger()).unwrap();
    store
        .immediate(|tx| {
            let run = runs::of_project(tx, &w.proj())?.remove(0);
            let task = tasks::of_run(tx, &run.run_id)?.remove(0).task_id;
            tx.execute(
                "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
                [&task],
            )?;
            let new = NewWorker {
                run_id: run.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            };
            let worker = bind_live(tx, &task, &new, None)?.worker_id;
            let ids = if on_task { vec![task] } else { Vec::new() };
            workers::set_wait(tx, &worker, REVIEWS_WAIT, "reviews", &ids, seconds)?;
            Ok(worker)
        })
        .unwrap()
}

fn set_clock(w: &World, clock: i64) {
    let c = rusqlite::Connection::open(w.ledger()).unwrap();
    c.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, 1, 1, 'then', ?1, 'then')
         ON CONFLICT (id) DO UPDATE SET fleet_clock_s = excluded.fleet_clock_s",
        [clock],
    )
    .unwrap();
}

fn shown_waits(w: &World, worker: &str) -> Vec<serde_json::Value> {
    let st = w.status_json();
    let line = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["worker"]["worker_id"] == worker)
        .unwrap_or_else(|| panic!("no line of {worker}: {st}"))
        .clone();
    line["waiting"].as_array().unwrap().clone()
}

#[test]
fn status_writes_nothing_for_a_wait_that_must_end() {
    let w = World::new();
    let worker = waiter(&w, -60, false);
    assert_eq!(shown_waits(&w, &worker), Vec::<serde_json::Value>::new());
}

#[test]
fn status_writes_nothing_for_a_wait_whose_cap_moves() {
    let w = World::new();
    let worker = waiter(&w, 7 * 24 * 3600, true);
    let before: i64 = rusqlite::Connection::open(w.ledger())
        .unwrap()
        .query_row(
            "SELECT due_at FROM worker_waits WHERE worker_id = ?1",
            [&worker],
            |r| r.get(0),
        )
        .unwrap();
    set_clock(&w, 1_000);
    let waits = shown_waits(&w, &worker);
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0]["due_at"], serde_json::json!(before + 1_000));
}
