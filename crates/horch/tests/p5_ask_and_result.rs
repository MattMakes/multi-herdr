//! `horch ask`, the QUESTION line of `horch tell`, `horch result` and the
//! top-level `horch done` of a worker write through the outbox writer of
//! the own attempt (FR-7, RR-5; ticket 28 of fleetd-p5,
//! `docs/tickets/fleetd-p5/MAP.md` Q3 and Q5).
//!
//! Each test runs the development `horch` with temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and fleet root.
//! The stand-in wrapper is a real `sh` process: the test records it as the
//! wrapper of a live attempt, then the `sh` runs `horch` as its child. No
//! test sets `HORCH_WORKER_ID` or `HORCH_TASK_YAML`: the command finds its
//! attempt by its parent chain only.

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
const NO_LIVE_ATTEMPT: &str = "no live attempt of this process";

/// A temp ledger with the schema of today, a fleet root and a fake herdr
/// with no pane.
struct World {
    temp: tempfile::TempDir,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let w = World { temp };
    std::fs::create_dir_all(w.fleet()).unwrap();
    std::fs::create_dir_all(w.ledger().parent().unwrap()).unwrap();
    let mut conn = Connection::open(w.ledger()).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let herdr = w.root().join("herdr");
    std::fs::write(
        &herdr,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[],\"panes\":[]}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
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

    fn outbox(&self) -> PathBuf {
        self.fleet()
            .join("tasks")
            .join(TASK)
            .join("attempts")
            .join("A1")
            .join("outbox")
    }

    fn questions(&self) -> PathBuf {
        self.outbox().join("questions")
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
            .env("HORCH_HERDR_BIN", root.join("herdr"))
            .env("HORCH_WORKSPACE_ID", "w1")
            .env("HORCH_ROLE", "dev1")
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_NOW", AT)
            .env("HORCH_NO_WATCHDOG", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }

    /// Run `horch args` as the child of a stand-in wrapper of `T-1/A1`. The
    /// wrapper `sh` waits for 1 line on its stdin, so the test records it
    /// before `horch` starts.
    fn in_attempt(&self, args: &[&str]) -> Output {
        let horch = self.horch(&[]);
        let mut sh = Command::new("sh");
        sh.args(["-c", r#"read go; "$HORCH_BIN" "$@"; exit $?"#, "sh"])
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
        go(wrapper)
    }
}

/// Send the line that starts the wrapper, and wait for its end.
fn go(mut wrapper: Child) -> Output {
    let mut stdin = wrapper.stdin.take().unwrap();
    stdin.write_all(b"go\n").unwrap();
    drop(stdin);
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

fn read_json(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap()
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The names of the files in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn result(w: &World) -> serde_json::Value {
    let path = w.outbox().join("result.json");
    assert_eq!(mode_of(&path), 0o600, "the mode of result.json");
    read_json(&path)
}

#[test]
fn ask_writes_question_1_then_2() {
    let w = world();
    assert_ok(&w.in_attempt(&["ask", "Which port does the server use?"]));
    assert_ok(&w.in_attempt(&["ask", "Is 8080 free?"]));
    assert_eq!(names(&w.questions()), ["1.json", "2.json"]);
    assert_eq!(mode_of(&w.questions()), 0o700, "the mode of questions/");
    for (n, asked) in [(1, "Which port does the server use?"), (2, "Is 8080 free?")] {
        let path = w.questions().join(format!("{n}.json"));
        assert_eq!(mode_of(&path), 0o600, "the mode of {n}.json");
        let q = read_json(&path);
        assert_eq!(q["task"], TASK);
        assert_eq!(q["attempt"], "A1");
        assert_eq!(q["text"], asked);
    }
}

#[test]
fn question_line_of_tell_writes_the_same_question_file() {
    let w = world();
    let told = w.in_attempt(&["tell", "orchestrator", "QUESTION: which port? I recommend 8080."]);
    let q = w.questions().join("1.json");
    assert!(
        q.is_file(),
        "no question file; tell: exit {:?}\nstdout: {}\nstderr: {}",
        told.status.code(),
        text(&told.stdout),
        text(&told.stderr)
    );
    let q = read_json(&q);
    assert_eq!(q["task"], TASK);
    assert_eq!(q["attempt"], "A1");
    assert!(
        q["text"]
            .as_str()
            .unwrap()
            .contains("QUESTION: which port? I recommend 8080."),
        "{q}"
    );
    // The same writer numbers the files: the next ask is file 2.
    assert_ok(&w.in_attempt(&["ask", "And the host?"]));
    assert_eq!(names(&w.questions()), ["1.json", "2.json"]);
    // A NOTE line asks nothing.
    w.in_attempt(&["tell", "orchestrator", "NOTE: the build passes."]);
    assert_eq!(names(&w.questions()), ["1.json", "2.json"]);
}

#[test]
fn result_writes_through_the_outbox_writer() {
    let w = world();
    let o = w.in_attempt(&[
        "result",
        "--status",
        "completed",
        "--summary",
        "I added the parser.",
        "--commit",
        "0123456789abcdef0123456789abcdef01234567",
        "--file",
        "src/parse.rs",
    ]);
    assert_ok(&o);
    assert_eq!(mode_of(&w.outbox()), 0o700, "the mode of the outbox");
    let r = result(&w);
    assert_eq!(r["task"], TASK);
    assert_eq!(r["attempt"], "A1");
    assert_eq!(r["status"], "completed");
    assert_eq!(r["summary"], "I added the parser.");
    assert_eq!(
        r["commits"],
        serde_json::json!(["0123456789abcdef0123456789abcdef01234567"])
    );
    assert_eq!(r["files_changed"], serde_json::json!(["src/parse.rs"]));
    // result.json is written 1 time.
    let again = w.in_attempt(&["result", "--status", "failed", "--summary", "x", "--commit", "abc"]);
    assert_eq!(again.status.code(), Some(1), "{}", text(&again.stderr));
    assert_eq!(result(&w)["status"], "completed");
    // --check sees the result of the own attempt.
    assert_ok(&w.in_attempt(&["result", "--check"]));
}

#[test]
fn horch_done_of_a_worker_writes_result_completed() {
    let w = world();
    assert_ok(&w.in_attempt(&["done", "I", "added", "the", "parser."]));
    let r = result(&w);
    assert_eq!(r["task"], TASK);
    assert_eq!(r["attempt"], "A1");
    assert_eq!(r["status"], "completed");
    assert_eq!(r["summary"], "I added the parser.");
}

#[test]
fn ask_outside_an_attempt_is_refused() {
    let w = world();
    // The attempt is live, but its wrapper is not a parent of this process.
    let other = Command::new("sh")
        .args(["-c", "read go"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    w.live(other.id());
    let o = w.horch(&["ask", "Which port?"]).output().unwrap();
    go(other);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stdout));
    assert!(
        text(&o.stderr).contains(NO_LIVE_ATTEMPT),
        "stderr: {}",
        text(&o.stderr)
    );
    assert!(!w.questions().exists(), "a refused ask writes no file");
}
