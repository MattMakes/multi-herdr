//! The wrapper of a worker reads no operator token and sends no request of
//! class operator (T-899, SR-8; ticket 61 of fleetd-build).
//!
//! - The wrapper path never reads `<fleet root>/run/operator.token`: in a
//!   process that is the wrapper, the client of the channel refuses each
//!   operator kind before the token read, and a write of a session record
//!   stays in the process and sends nothing.
//! - A request of class operator from a process of a live attempt, which
//!   the client does not stop (a raw line with a copy of the token), is
//!   refused by fleetd, and its `requests` row records the refusal.
//!
//! Each test marks this test process as the wrapper, as `horch worker`
//! does. The client refusal is not the boundary: the peer process check of
//! fleetd and the worker sandbox are.
//!
//! Unix only: the channel is a Unix socket, and the tests use unix modes.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::ExecutionStore;
use horch_core::fleetd::fleet_runtime::adapters::channel_client;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Request;
use horch_core::fleetd::fleet_runtime::adapters::requests_store::{self, Stored};
use horch_core::fleetd::fleet_runtime::adapters::{test_fleetd, token_file};
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::runs;
use horch_core::taskledger::store::Store;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_platform::channel::Stream;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

/// The start of the refusal of the client in the wrapper.
const WRAPPER_REFUSED: &str = "the wrapper of a worker sends no operator request";

/// The context of the token read of the client: a read error names it.
const TOKEN_READ: &str = "reading the operator token";

/// A fleet in a short temporary folder: its ledger and its fleet root.
struct Fleet {
    dir: TempDir,
}

impl Fleet {
    fn new() -> Fleet {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let dir = tempfile::Builder::new()
            .prefix("ob")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let fleet = Fleet { dir };
        drop(Store::open(&fleet.db()).unwrap());
        fleet
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("l.db")
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("f")
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn ctx(&self) -> RuntimeContext {
        let dir = self.dir.path();
        let text = |p: PathBuf| p.to_string_lossy().into_owned();
        let env = MapEnv::new(dir)
            .with("HOME", &text(dir.to_path_buf()))
            .with("PATH", "/nonexistent")
            .with("HORCH_LEDGER_DB", &text(self.db()))
            .with("FLEET_ROOT", &text(self.root()))
            .with("HORCH_STATE_DIR", &text(self.state()))
            .with("HORCH_DATA_DIR", &text(dir.join("data")));
        RuntimeContext::from_env(&env).unwrap()
    }

    /// Each entry of `run/` of the fleet root but the token file: a
    /// channel or a request file of the client.
    fn run_entries(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut todo = vec![self.root().join("run")];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    todo.push(p.clone());
                }
                if p != token_file::path(&self.root()) {
                    found.push(p);
                }
            }
        }
        found
    }
}

/// Set the mode of `path` to `mode`.
fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// The payload of `session_records` that the wrapper sent before T-899.
fn session_payload(record_id: &str) -> Map<String, Value> {
    json!({
        "project_root": "/p",
        "rows": [{ "record_id": record_id, "record": null }],
    })
    .as_object()
    .unwrap()
    .clone()
}

/// The wrapper path never reads the operator token: with a token file that
/// no process can read, each operator kind that the wrapper sent before
/// T-899 gets the refusal of the wrapper, not a read error of the token.
/// The client makes no channel and no request file.
#[test]
fn wrapper_refuses_each_operator_kind_before_the_token_read() {
    channel_client::mark_wrapper_process();
    let fleet = Fleet::new();
    let token = token_file::ensure(&fleet.root()).unwrap();
    drop(token);
    let path = token_file::path(&fleet.root());
    chmod(&path, 0o000);
    let kinds = [
        Kind::SessionRecords,
        Kind::WorkerLaunched,
        Kind::WorkerLaunchFailed,
        Kind::WorkerProgress,
        Kind::WorkerDone,
        Kind::SpawnGateNote,
        Kind::LedgerGc,
    ];
    for kind in kinds {
        let err = channel_client::send(&fleet.ctx(), kind, session_payload("r1")).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains(WRAPPER_REFUSED), "{}: {text}", kind.as_str());
        assert!(!text.contains(TOKEN_READ), "{}: {text}", kind.as_str());
    }
    chmod(&path, 0o600);
    assert_eq!(fleet.run_entries(), Vec::<PathBuf>::new());
}

/// A session record that the wrapper writes (the session id that the
/// launch finds, the window decision) stays in the wrapper process: a
/// later read of the process sees it, the ledger has no row of it, and the
/// client sends no request and reads no token. fleetd gets the session id
/// from `session.json` of the attempt.
#[test]
fn wrapper_session_record_stays_in_its_process() {
    channel_client::mark_wrapper_process();
    let fleet = Fleet::new();
    token_file::ensure(&fleet.root()).unwrap();
    let path = token_file::path(&fleet.root());
    chmod(&path, 0o000);
    let ctx = fleet.ctx();
    let store = ExecutionStore::open(&ctx.paths, Path::new("/project/ob"));
    store
        .insert(LedgerRecordV1 {
            record_id: "rec-ob-1".into(),
            role: "sonnet-1".into(),
            ..Default::default()
        })
        .unwrap();
    // What the session discovery of the launch writes.
    store
        .update(|records| {
            for r in records.iter_mut().filter(|r| r.record_id == "rec-ob-1") {
                r.session_id = Some("sess-ob-1".into());
            }
            Ok(())
        })
        .unwrap();
    chmod(&path, 0o600);

    let again = ExecutionStore::open(&ctx.paths, Path::new("/project/ob"));
    let record = again.get("rec-ob-1").unwrap();
    assert_eq!(record.session_id.as_deref(), Some("sess-ob-1"));
    let rows: i64 = Store::open_read_only(&fleet.db())
        .unwrap()
        .conn()
        .query_row("SELECT COUNT(*) FROM session_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "the wrapper writes no row of the ledger");
    assert_eq!(fleet.run_entries(), Vec::<PathBuf>::new());
}

/// Record this test process as the wrapper of a live attempt with no task
/// in the ledger `db`, and give the attempt id.
fn record_this_process_as_wrapper(db: &Path) -> String {
    let pid = std::process::id();
    let start = horch_platform::process::start_time(pid).unwrap();
    let mut store = Store::open(db).unwrap();
    store
        .immediate(|tx| {
            let run = runs::create(tx, Path::new("/project/ob"), &runs::solo_new_run("w1"))?;
            let worker = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: "opus".into(),
                    ..Default::default()
                },
            )?;
            let id = format!("A-{worker}");
            tx.execute(
                "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
                 VALUES (?1, NULL, ?2, ?3, ?4, 'live', '2026-11-20T12:00:00Z')",
                rusqlite::params![id, worker, pid, i64::try_from(start).unwrap()],
            )?;
            Ok(id)
        })
        .unwrap()
}

/// Send `line` on the channel `channel` and give the reply line.
fn exchange(channel: &Path, line: &str) -> Value {
    let stream = Stream::connect(channel, Instant::now() + Duration::from_secs(5)).unwrap();
    stream.set_read_timeout(Duration::from_secs(30)).unwrap();
    let mut writer = &stream;
    writer.write_all(line.as_bytes()).unwrap();
    writer.flush().unwrap();
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).unwrap();
    serde_json::from_str(&reply).unwrap()
}

/// A wrapper request of class operator that does not go through the
/// client (a raw `session_records` line with a copy of the valid token,
/// from the recorded wrapper process) is refused by fleetd. Its `requests`
/// row records the refusal and the attempt.
#[test]
fn raw_wrapper_request_of_class_operator_is_refused_and_recorded() {
    let fleet = Fleet::new();
    let server = test_fleetd::serve_session_records_at(&fleet.db(), &fleet.root()).unwrap();
    let attempt = record_this_process_as_wrapper(&fleet.db());
    let token = token_file::read(&fleet.root()).unwrap();
    let id = RequestId::parse("ob-raw-session-records").unwrap();
    let request = Request {
        request_id: id.clone(),
        kind: Kind::SessionRecords,
        payload: session_payload("rec-ob-raw"),
        token: Some(token.text().to_string()),
    };
    let reply = exchange(
        &fleet.root().join("run").join("fleetd.sock"),
        &request.to_line(),
    );
    assert_eq!(reply["state"], "refused", "{reply}");

    let conn = Store::open_read_only(&fleet.db()).unwrap();
    let row = requests_store::get(conn.conn(), id.as_str())
        .unwrap()
        .expect("fleetd records the refused request");
    let Stored::Refused(reason) = row.stored else {
        panic!("the request is refused: {:?}", row.stored);
    };
    let peer = format!("the peer process is a fleet process of attempt {attempt}");
    assert!(reason.contains(&peer), "{reason}");
    let auth: Option<String> = conn
        .conn()
        .query_row(
            "SELECT auth FROM requests WHERE request_id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(auth, None, "a refused request has no auth");
    let records: i64 = conn
        .conn()
        .query_row("SELECT COUNT(*) FROM session_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(records, 0, "fleetd applies nothing of the refused request");
    drop(server);
}
