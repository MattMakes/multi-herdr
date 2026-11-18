//! The worker commands of `horch ledger` write the outbox of their own
//! attempt and open no ledger for a write (RR-5, FR-7; ticket 27 of
//! fleetd-p5, `docs/tickets/fleetd-p5/MAP.md` Q3 and Q4).
//!
//! - `progress` and `note` append 1 line to `outbox/progress.jsonl`.
//! - `done` and `finish` write `outbox/result.json` with `status: completed`.
//! - `handoff` writes `outbox/result.json` with `status: blocked`,
//!   `reason: handoff` and the path.
//! - `claim` and `worktree new` are refused with 1 line.
//!
//! Each test runs the development `horch` with temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and fleet root.
//! The stand-in wrapper is a real `sh` process: the test records it as the
//! wrapper of a live attempt, then the `sh` runs `horch` as its child. No
//! test sets `HORCH_WORKER_ID`: the command finds its attempt by its parent
//! chain only.

#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use horch_core::taskledger::schema;
use horch_platform::process;
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const AT: &str = "2026-11-18T12:00:00Z";
const TASK: &str = "T-1";
const REFUSAL: &str =
    "fleetd binds each task and makes its worktree; this command is not for a worker";

/// A temp ledger with the schema of today, in WAL mode as the live ledger,
/// and a fleet root.
struct World {
    temp: tempfile::TempDir,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let w = World { temp };
    std::fs::create_dir_all(w.fleet()).unwrap();
    std::fs::create_dir_all(w.ledger().parent().unwrap()).unwrap();
    let mut conn = Connection::open(w.ledger()).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    w
}

impl World {
    fn root(&self) -> &Path {
        self.temp.path()
    }

    fn ledger(&self) -> PathBuf {
        self.root().join("ledger").join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root().join("fleet")
    }

    fn task_dir(&self) -> PathBuf {
        self.fleet().join("tasks").join(TASK)
    }

    fn outbox(&self) -> PathBuf {
        self.task_dir().join("attempts").join("A1").join("outbox")
    }

    /// Record the live attempt `T-1/A1` whose wrapper is `pid`, and make its
    /// attempt folder, as fleetd does. Its outbox is not made.
    fn live(&self, pid: u32) {
        let start = process::start_time(pid).expect("the start time of the wrapper");
        let conn = Connection::open(self.ledger()).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
             VALUES (?1, ?2, 'w-1', ?3, ?4, 'live', ?5)",
            params![
                format!("{TASK}/A1"),
                TASK,
                i64::from(pid),
                i64::try_from(start).unwrap(),
                AT
            ],
        )
        .unwrap();
        std::fs::create_dir_all(self.outbox().parent().unwrap()).unwrap();
    }

    /// The development `horch` with `args` and only temporary folders.
    fn horch(&self, args: &[&str]) -> Command {
        let root = self.root();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", root.join("proj"))
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_NOW", AT)
            .env("HORCH_NO_WATCHDOG", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }

    /// Start a stand-in wrapper `sh` that runs `script` with `args` after 1
    /// line on its stdin, and record it as the wrapper of the live attempt
    /// `T-1/A1`. `$HORCH_BIN` in `script` is the development `horch`.
    fn wrapper(&self, script: &str, args: &[&str]) -> Child {
        let horch = self.horch(&[]);
        let mut sh = Command::new("sh");
        sh.args(["-c", script, "sh"])
            .args(args)
            .env_clear()
            .env("HORCH_BIN", horch.get_program())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in horch.get_envs() {
            if let Some(value) = value {
                sh.env(key, value);
            }
        }
        let wrapper = sh.spawn().unwrap();
        self.live(wrapper.id());
        wrapper
    }

    /// Run `horch args` as the child of a stand-in wrapper of `T-1/A1`.
    fn in_attempt(&self, args: &[&str]) -> Output {
        go(self.wrapper(r#"read go; "$HORCH_BIN" "$@"; exit $?"#, args))
    }
}

/// Send the line that starts the wrapper, and wait for its end.
fn go(mut wrapper: Child) -> Output {
    // The wrapper does not give its stdin to the child.
    wrapper.stdin.take().unwrap().write_all(b"go\n").unwrap();
    wrapper.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        o.status.code(),
        text(&o.stdout),
        text(&o.stderr)
    );
}

fn assert_refused(o: &Output, line: &str) {
    assert_eq!(
        o.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        text(&o.stdout),
        text(&o.stderr)
    );
    assert!(
        text(&o.stderr).contains(line),
        "stderr has no `{line}`: {}",
        text(&o.stderr)
    );
}

fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn result(w: &World) -> serde_json::Value {
    let path = w.outbox().join("result.json");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the mode of {}", path.display());
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn progress_appends_1_line_and_opens_no_writable_ledger() {
    let w = world();
    let wrapper = w.wrapper(
        r#"read go; "$HORCH_BIN" "$@"; exit $?"#,
        &["ledger", "progress", "step 1 is done"],
    );
    // fleetd holds the write lock: a command that writes the ledger waits
    // for it and fails. A read goes on in WAL mode.
    let mut fleetd = Connection::open(w.ledger()).unwrap();
    let lock = fleetd
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let o = go(wrapper);
    lock.rollback().unwrap();
    assert_ok(&o);
    let got = lines(&w.outbox().join("progress.jsonl"));
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["kind"], "progress");
    assert_eq!(got[0]["text"], "step 1 is done");
    assert_eq!(got[0]["task"], TASK);
    assert_eq!(got[0]["attempt"], "A1");
    let events: i64 = fleetd
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(events, 0, "the command wrote a ledger event");
}

#[test]
fn note_appends_a_note_line() {
    let w = world();
    assert_ok(&w.in_attempt(&["ledger", "note", "the cache is cold"]));
    let got = lines(&w.outbox().join("progress.jsonl"));
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["kind"], "note");
    assert_eq!(got[0]["text"], "the cache is cold");
}

#[test]
fn done_writes_result_completed() {
    let w = world();
    assert_ok(&w.in_attempt(&["ledger", "done", "I added the parser."]));
    let r = result(&w);
    assert_eq!(r["status"], "completed");
    assert_eq!(r["summary"], "I added the parser.");
    assert_eq!(r["task"], TASK);
    assert_eq!(r["attempt"], "A1");
}

#[test]
fn finish_writes_result_completed() {
    let w = world();
    let o = w.in_attempt(&["ledger", "finish"]);
    assert_ok(&o);
    assert_eq!(result(&w)["status"], "completed");
    assert!(
        text(&o.stdout).contains("fleetd runs the finish steps"),
        "{}",
        text(&o.stdout)
    );
}

#[test]
fn handoff_writes_blocked_with_reason_handoff() {
    let w = world();
    std::fs::create_dir_all(w.task_dir()).unwrap();
    let file = w.task_dir().join("handoff.md");
    std::fs::write(&file, "# next step\n").unwrap();
    let file = file.canonicalize().unwrap();
    assert_ok(&w.in_attempt(&["ledger", "handoff", file.to_str().unwrap()]));
    let r = result(&w);
    assert_eq!(r["status"], "blocked");
    assert_eq!(r["reason"], "handoff");
    assert_eq!(r["handoff"], file.to_str().unwrap());
}

#[test]
fn handoff_with_a_path_outside_the_attempt_is_refused() {
    let w = world();
    let outside = w.root().join("elsewhere.md");
    std::fs::write(&outside, "# not here\n").unwrap();
    let o = w.in_attempt(&["ledger", "handoff", outside.to_str().unwrap()]);
    assert!(!o.status.success(), "stdout: {}", text(&o.stdout));
    assert!(
        text(&o.stderr).contains("outside the worktree and the task folder"),
        "{}",
        text(&o.stderr)
    );
    // A relative path is refused too: the path must be absolute.
    std::fs::create_dir_all(w.task_dir()).unwrap();
    std::fs::write(w.task_dir().join("handoff.md"), "# next\n").unwrap();
    let o = w.in_attempt(&["ledger", "handoff", "handoff.md"]);
    assert!(!o.status.success(), "stdout: {}", text(&o.stdout));
    assert!(text(&o.stderr).contains("absolute"), "{}", text(&o.stderr));
    assert!(!w.outbox().join("result.json").exists());
}

#[test]
fn claim_from_a_worker_is_refused_with_its_line() {
    let w = world();
    let o = w.in_attempt(&["ledger", "claim"]);
    assert_refused(&o, REFUSAL);
    assert!(!w.outbox().exists());
}

#[test]
fn worktree_new_from_a_worker_is_refused() {
    let w = world();
    assert_refused(&w.in_attempt(&["ledger", "worktree", "new", "x"]), REFUSAL);
    assert_refused(
        &w.in_attempt(&["ledger", "worktree", "scratch", "y"]),
        REFUSAL,
    );
}

#[test]
fn progress_and_done_work_when_the_ledger_file_is_read_only() {
    let w = world();
    // fleetd holds the ledger open, as on a live fleet, so its WAL files
    // exist. The sandbox of the worker makes each of them read-only.
    let fleetd = Connection::open(w.ledger()).unwrap();
    fleetd
        .execute_batch("CREATE TABLE IF NOT EXISTS t (x); INSERT INTO t VALUES (1);")
        .unwrap();
    // The wrapper runs progress, then done. Its row is written before the
    // ledger is made read-only.
    let wrapper = w.wrapper(
        r#"read go; "$HORCH_BIN" ledger progress "step 2" && "$HORCH_BIN" ledger done "all steps are done"; exit $?"#,
        &[],
    );
    let dir = w.ledger().parent().unwrap().to_path_buf();
    let mut files = vec![w.ledger()];
    for suffix in ["-wal", "-shm"] {
        let f = PathBuf::from(format!("{}{suffix}", w.ledger().display()));
        if f.exists() {
            files.push(f);
        }
    }
    for f in &files {
        mode(f, 0o444);
    }
    // 0444 on the folder would deny the lookup of the file, and so each
    // read. A read-only folder keeps its search bit.
    mode(&dir, 0o555);
    let o = go(wrapper);
    mode(&dir, 0o755);
    for f in &files {
        mode(f, 0o644);
    }
    drop(fleetd);
    assert_ok(&o);
    assert!(
        !text(&o.stderr).contains("readonly database"),
        "{}",
        text(&o.stderr)
    );
    assert_eq!(lines(&w.outbox().join("progress.jsonl")).len(), 1);
    assert_eq!(result(&w)["status"], "completed");
}
