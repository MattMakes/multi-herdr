//! `horch request <request_id>` (RR-8, ticket 16 of fleetd-p5): the state
//! and the result of 1 request, read from its `requests` row or from its
//! request file that waits for fleetd.
//!
//! Each test runs the real binary with its own ledger, state and fleet
//! folders, and the watchdog off.

use std::path::PathBuf;
use std::process::Command;

use horch_core::taskledger::Store;

const PATH: &str = "/usr/bin:/bin";

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("fleet")).unwrap();
        World { _tmp: tmp, root }
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    /// 1 `requests` row, as fleetd writes it.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        id: &str,
        kind: &str,
        caller: &str,
        auth: Option<&str>,
        state: &str,
        result: Option<&str>,
        reason: Option<&str>,
    ) {
        let store = Store::open(&self.ledger()).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO requests (request_id, kind, payload_json, payload_sha256, caller,
                     class, auth, state, result_json, reason, received_at, decided_at)
                 VALUES (?1, ?2, '{}', 'abc', ?3, 'operator', ?4, ?5, ?6, ?7, 't0', 't1')",
                rusqlite::params![id, kind, caller, auth, state, result, reason],
            )
            .unwrap();
    }

    fn horch(&self, args: &[&str]) -> Out {
        let root = &self.root;
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
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
            .env("HORCH_NO_WATCHDOG", "1")
            .output()
            .unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

fn assert_has(text: &str, want: &str) {
    assert!(text.contains(want), "no {want:?} in:\n{text}");
}

#[test]
fn applied_request_prints_its_result() {
    let w = World::new();
    w.row(
        "r-1",
        "task_cancel",
        "uid:501",
        Some("token"),
        "applied",
        Some(r#"{"task":"T-7","state":"cancelled"}"#),
        None,
    );
    let o = w.horch(&["request", "r-1"]);
    assert_eq!(o.code, 0, "stderr:\n{}", o.err);
    assert!(o.out.starts_with("request r-1: applied\n"), "{}", o.out);
    assert_has(&o.out, r#"result: {"task":"T-7","state":"cancelled"}"#);
    assert_has(&o.out, "kind: task_cancel");
    assert_has(&o.out, "class: operator");
    assert_has(&o.out, "caller: uid:501");
    assert_has(&o.out, "auth: token");
}

#[test]
fn refused_request_prints_its_reason() {
    let w = World::new();
    w.row(
        "r-2",
        "spawn",
        "pid:42",
        None,
        "refused",
        None,
        Some("a worker sends no operator kind"),
    );
    let o = w.horch(&["request", "r-2"]);
    assert_eq!(o.code, 0, "stderr:\n{}", o.err);
    assert!(o.out.starts_with("request r-2: refused\n"), "{}", o.out);
    assert_has(&o.out, "reason: a worker sends no operator kind");
    assert_has(&o.out, "kind: spawn");
    assert_has(&o.out, "caller: pid:42");
    assert_has(&o.out, "auth: -");
}

#[test]
fn unknown_request_exits_1() {
    let w = World::new();
    // No ledger file at all.
    let o = w.horch(&["request", "r-9"]);
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert_has(&o.err, "no request r-9");
    assert!(!w.ledger().exists(), "the read made the ledger");

    // A ledger with another request.
    w.row("r-1", "spawn", "u", None, "pending", None, None);
    let o = w.horch(&["request", "r-9"]);
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert_has(&o.err, "no request r-9");
}

#[test]
fn waiting_request_file_is_reported() {
    let w = World::new();
    let dir = w.fleet().join("run").join("requests");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("r-3.json"), "{}").unwrap();
    let o = w.horch(&["request", "r-3"]);
    assert_eq!(o.code, 0, "stderr:\n{}", o.err);
    assert_eq!(o.out, "request r-3: waits for fleetd\n");
}

#[test]
fn json_prints_the_row() {
    let w = World::new();
    w.row(
        "r-4",
        "run_new",
        "uid:501",
        Some("token"),
        "applied",
        Some(r#"{"run":"r1"}"#),
        None,
    );
    let o = w.horch(&["request", "r-4", "--json"]);
    assert_eq!(o.code, 0, "stderr:\n{}", o.err);
    let v: serde_json::Value = serde_json::from_str(o.out.trim()).unwrap();
    let want = serde_json::json!({
        "request_id": "r-4",
        "kind": "run_new",
        "payload_json": "{}",
        "payload_sha256": "abc",
        "caller": "uid:501",
        "class": "operator",
        "auth": "token",
        "state": "applied",
        "result_json": "{\"run\":\"r1\"}",
        "reason": null,
        "received_at": "t0",
        "decided_at": "t1",
    });
    assert_eq!(v, want);
}
