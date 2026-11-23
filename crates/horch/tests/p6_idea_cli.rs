#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::thread::JoinHandle;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{parse_request, Outcome, Reply};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::taskledger::store::Store;
use horch_platform::channel::Listener;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const IDEA: &str = "I-0007";
const ASSUMPTIONS: &str = "feature: fast-search\n\n- A1: search stays under 50 ms.\n";

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    dir: TempDir,
}

impl World {
    fn new() -> World {
        let dir = tempfile::Builder::new()
            .prefix("p6i")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let w = World { dir };
        Store::open(&w.db()).unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        std::fs::create_dir_all(w.root().join("home")).unwrap();
        std::fs::create_dir_all(w.project()).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        w.git(&["commit", "-q", "--allow-empty", "-m", "first"]);
        w
    }

    fn root(&self) -> PathBuf {
        horch_platform::path::canonicalize(self.dir.path()).unwrap()
    }

    fn db(&self) -> PathBuf {
        self.root().join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root().join("fleet")
    }

    fn project(&self) -> PathBuf {
        self.root().join("proj")
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.project())
            .env("HOME", self.root().join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "n")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "n")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn horch(&self, args: &[&str]) -> Out {
        let root = self.root();
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.db())
            .env("HORCH_PROJECT_DIR", self.project())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_NO_WATCHDOG", "1")
            .output()
            .unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn sql(&self, sql: &str, params: impl rusqlite::Params) {
        Store::open(&self.db())
            .unwrap()
            .conn()
            .execute(sql, params)
            .unwrap();
    }

    fn inside_an_attempt(&self) {
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        self.sql(
            "INSERT INTO workers (worker_id, origin, status) VALUES ('idea-lead-1', 'import', 'working')",
            [],
        );
        self.sql(
            "INSERT INTO attempts (id, worker_id, pid, pid_start, state)
             VALUES ('A-1', 'idea-lead-1', ?1, ?2, 'live')",
            rusqlite::params![i64::from(pid), i64::try_from(start).unwrap()],
        );
    }

    fn idea_awaiting_approval(&self) {
        self.sql(
            "INSERT INTO features (id, name, primary_branch, state, created_at, updated_at)
             VALUES ('F-1', 'fast-search', 'main', 'open', 'now', 'now')",
            [],
        );
        self.sql(
            "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
             VALUES (?1, 'F-1', 'awaiting_approval', 'now', 'now')",
            [IDEA],
        );
        let folder = self.fleet().join("ideas").join(IDEA);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("assumptions.md"), ASSUMPTIONS).unwrap();
    }

    fn request_files(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.fleet().join("run").join("requests")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn fleetd(&self, result: Value) -> Fleetd {
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        self.sql(
            "INSERT INTO fleetd
             (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, ?2, 'now', 0, 'now')",
            rusqlite::params![i64::from(pid), i64::try_from(start).unwrap()],
        );
        let run = self.fleet().join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = Listener::bind(&run.join("fleetd.sock")).unwrap();
        let thread = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = Vec::new();
            BufReader::new(&stream)
                .read_until(b'\n', &mut line)
                .unwrap();
            let request = parse_request(&line, 64 * 1024).unwrap();
            let reply = Reply {
                request_id: request.request_id,
                outcome: Outcome::Applied(result),
            };
            let mut writer = &stream;
            writer.write_all(reply.to_line().as_bytes()).unwrap();
            writer.flush().unwrap();
            (request.kind.as_str().to_string(), request.payload)
        });
        Fleetd { thread }
    }
}

struct Fleetd {
    thread: JoinHandle<(String, Map<String, Value>)>,
}

impl Fleetd {
    fn request(self) -> (String, Map<String, Value>) {
        self.thread.join().unwrap()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn assert_refused_inside(w: &World, o: &Out) {
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(
        o.err.contains("inside a fleet attempt"),
        "stderr:\n{}",
        o.err
    );
    assert!(!o.err.contains("not built yet"), "stderr:\n{}", o.err);
    assert_eq!(w.request_files(), Vec::<String>::new());
}

#[test]
fn idea_add_prints_the_id() {
    let w = World::new();
    let fleetd = w.fleetd(json!({"idea": IDEA, "feature": "F-1", "effects": {}}));
    let o = w.horch(&["idea", "add", "make search fast"]);
    assert_eq!(o.code, 0, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert_eq!(o.out, format!("{IDEA}\n"));
    let (kind, payload) = fleetd.request();
    assert_eq!(kind, "idea_add");
    assert_eq!(
        Value::Object(payload),
        json!({
            "text": "make search fast",
            "project": w.project().to_str().unwrap(),
            "primary_branch": "main",
        })
    );
}

#[test]
fn idea_add_with_fleetd_down_writes_a_request_file() {
    let w = World::new();
    let o = w.horch(&["idea", "add", "make search fast", "--to", "F-1"]);
    assert_eq!(o.code, 0, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    let files = w.request_files();
    assert_eq!(files.len(), 1, "{files:?}");
    let id = files[0].strip_suffix(".json").unwrap();
    assert!(o.out.contains(id), "stdout:\n{}", o.out);
    assert!(o.out.contains("waits for fleetd"), "stdout:\n{}", o.out);
}

#[test]
fn idea_approve_prints_the_assumptions_and_sends_their_sha256() {
    let w = World::new();
    w.idea_awaiting_approval();
    let fleetd = w.fleetd(json!({"idea": IDEA, "state": "specifying", "effects": {}}));
    let o = w.horch(&["idea", "approve", IDEA]);
    assert_eq!(o.code, 0, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    let at = o
        .out
        .find(ASSUMPTIONS)
        .expect("the assumptions are printed");
    let approved = o.out.find("approved").expect("the result is printed");
    assert!(at < approved, "stdout:\n{}", o.out);
    let (kind, payload) = fleetd.request();
    assert_eq!(kind, "idea_approve");
    assert_eq!(
        Value::Object(payload),
        json!({"idea": IDEA, "sha256": sha256_hex(ASSUMPTIONS.as_bytes())})
    );
}

#[test]
fn idea_approve_inside_a_fleet_attempt_is_refused() {
    let w = World::new();
    w.idea_awaiting_approval();
    w.inside_an_attempt();
    let o = w.horch(&["idea", "approve", IDEA]);
    assert_refused_inside(&w, &o);
    assert!(!o.out.contains("A1:"), "stdout:\n{}", o.out);
}

#[test]
fn idea_add_direct_inside_a_fleet_attempt_is_refused() {
    let w = World::new();
    let ticket = w.project().join("ticket.md");
    std::fs::write(&ticket, "# 1: a ticket\n").unwrap();
    w.inside_an_attempt();
    let o = w.horch(&["idea", "add", "--direct", ticket.to_str().unwrap()]);
    assert_refused_inside(&w, &o);
}

#[test]
fn feature_abandon_inside_a_fleet_attempt_is_refused() {
    let w = World::new();
    w.inside_an_attempt();
    let o = w.horch(&["feature", "abandon", "F-1"]);
    assert_refused_inside(&w, &o);
}

#[test]
fn idea_cancel_needs_a_reason() {
    let w = World::new();
    let o = w.horch(&["idea", "cancel", IDEA]);
    assert_eq!(o.code, 2, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    let o = w.horch(&["idea", "cancel", IDEA, "  "]);
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(o.err.contains("reason"), "stderr:\n{}", o.err);
    assert_eq!(w.request_files(), Vec::<String>::new());
}
