use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_cli;
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{self, Custody};
use horch_core::fleetd::fleet_runtime::application::handlers_run;
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::integration::application::commands::RefTips;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use rusqlite::Connection;
use serde_json::{json, Value};

const RESCUE_REF: &str = "refs/horch/rescue/branch/side/1";
const TIP: &str = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";

struct OneRef;

impl RefTips for OneRef {
    fn tip(&self, _repo: &Path, name: &str) -> anyhow::Result<Option<String>> {
        Ok((name == RESCUE_REF).then(|| TIP.to_string()))
    }
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
    store: Store,
    conn: Connection,
    run: Run,
    handlers: Handlers,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let root = top.join("repo");
    let state = top.join("state");
    std::fs::create_dir_all(&root).unwrap();
    let db = top.join("ledger.sqlite3");
    let mut store = Store::open(&db).unwrap();
    let run = store
        .immediate(|tx| {
            tx.execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, 1, 0, ?1, 0, ?1)",
                [util::now()],
            )?;
            runs::create(
                tx,
                &root,
                &NewRun {
                    slug: "r1".into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let mut handlers = Handlers::new();
    handlers_run::register(&mut handlers);
    handlers_cli::register(&mut handlers);
    handlers_custody::register(
        &mut handlers,
        Arc::new(Custody {
            guard: Generation(1),
            config: FleetConfig::default(),
            refs: Box::new(OneRef),
        }),
    );
    World {
        _tmp: tmp,
        root,
        state,
        store,
        conn,
        run,
        handlers,
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn request(kind: Kind, payload: Value) -> Request {
    let n = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Request {
        request_id: RequestId::parse(&format!("8a1c2a3e-0b9d-4c55-8e7f-{n:012x}")).unwrap(),
        kind,
        payload,
        token: Some("t".into()),
    }
}

impl World {
    fn outcome(&mut self, kind: Kind, payload: Value) -> Outcome {
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        let req = request(kind, payload);
        handle(
            &mut self.conn,
            Generation(1),
            &req,
            Some("me"),
            &decision,
            &self.handlers,
        )
        .unwrap()
        .outcome
    }

    fn applied(&mut self, kind: Kind, payload: Value) -> Value {
        match self.outcome(kind, payload) {
            Outcome::Applied(v) => v,
            Outcome::Refused(why) => panic!("refused: {why}"),
        }
    }

    fn refused(&mut self, kind: Kind, payload: Value) -> String {
        match self.outcome(kind, payload) {
            Outcome::Refused(why) => why,
            Outcome::Applied(v) => panic!("applied: {v}"),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn one(&self, sql: &str) -> Option<String> {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn worker(&mut self) -> String {
        let run_id = self.run.run_id.clone();
        self.store
            .immediate(|tx| {
                workers::insert(
                    tx,
                    &NewWorker {
                        run_id,
                        persona: "opus".into(),
                        role: Some("report".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap()
    }
}

#[test]
fn worker_progress_writes_the_event_of_the_line_and_checks_its_payload() {
    let mut w = world();
    let id = w.worker();
    let progress = |kind: &str, source: &str, text: &str| json!({ "worker_id": id, "kind": kind, "source": source, "text": text });
    let r = w.applied(
        Kind::WorkerProgress,
        progress("note", "agent", "read the ticket"),
    );
    assert_eq!(r["worker_id"], json!(id));
    assert_eq!(
        w.one(&format!(
            "SELECT text FROM events WHERE worker_id = '{id}' AND kind = 'note'"
        ))
        .as_deref(),
        Some("read the ticket")
    );
    w.applied(
        Kind::WorkerProgress,
        progress("progress", "transcript", "compaction completed: x"),
    );
    assert_eq!(
        w.one(&format!(
            "SELECT last_progress_source FROM workers WHERE worker_id = '{id}'"
        ))
        .as_deref(),
        Some("transcript")
    );
    let events = w.count("SELECT count(*) FROM events");
    for (payload, why) in [
        (progress("done", "agent", "x"), "is not progress or note"),
        (progress("note", "other", "x"), "is not a ProgressSource"),
        (progress("note", "agent", "  "), "is empty"),
        (
            progress(
                "note",
                "agent",
                &"x".repeat(handlers_cli::TEXT_MAX_BYTES + 1),
            ),
            "the limit is",
        ),
        (
            json!({ "worker_id": "w-none", "kind": "note", "source": "agent", "text": "x" }),
            "has no worker w-none",
        ),
        (json!({ "worker_id": id }), "a bad worker_progress payload"),
    ] {
        let got = w.refused(Kind::WorkerProgress, payload);
        assert!(got.contains(why), "{got} does not contain {why}");
    }
    assert_eq!(w.count("SELECT count(*) FROM events"), events);
}

#[test]
fn worker_done_ends_a_taskless_row_once() {
    let mut w = world();
    let id = w.worker();
    let r = w.applied(Kind::WorkerDone, json!({ "worker_id": id }));
    assert_eq!(r["ended"], json!(true));
    assert_eq!(
        w.one(&format!(
            "SELECT status FROM workers WHERE worker_id = '{id}'"
        ))
        .as_deref(),
        Some("done")
    );
    let r = w.applied(Kind::WorkerDone, json!({ "worker_id": id }));
    assert_eq!(r["ended"], json!(false));
    let why = w.refused(Kind::WorkerDone, json!({ "worker_id": "w-none" }));
    assert!(why.contains("has no worker w-none"), "{why}");
}

#[test]
fn run_adopt_gives_a_run_with_no_workspace_the_workspace_once() {
    let mut w = world();
    let run = w.run.run_id.clone();
    let why = w.refused(
        Kind::RunAdopt,
        json!({ "run_id": run, "workspace": "a b/c" }),
    );
    assert!(why.contains("is not valid"), "{why}");
    let r = w.applied(Kind::RunAdopt, json!({ "run_id": run, "workspace": "w1" }));
    assert_eq!(r["adopted"], json!(true));
    let r = w.applied(Kind::RunAdopt, json!({ "run_id": run, "workspace": "w2" }));
    assert_eq!(
        r["adopted"],
        json!(false),
        "a run keeps its first workspace"
    );
    assert_eq!(
        w.one(&format!(
            "SELECT workspace_id FROM runs WHERE run_id = '{run}'"
        ))
        .as_deref(),
        Some("w1")
    );
    let why = w.refused(
        Kind::RunAdopt,
        json!({ "run_id": "nope", "workspace": "w1" }),
    );
    assert!(!why.is_empty());
}

#[test]
fn question_answer_checks_its_payload_and_refuses_a_mail_with_no_row() {
    let mut w = world();
    let answer = |ws: &str, mail: &str, text: &str| json!({ "workspace": ws, "mail_id": mail, "text": text });
    for (payload, why) in [
        (
            answer("w1", "m-1", "use SQLite"),
            "no question row waits for an answer to mail m-1",
        ),
        (answer("../w1", "m-1", "x"), "is not valid"),
        (answer("w1", "../m", "x"), "is not valid"),
        (answer("w1", "m-1", ""), "is empty"),
    ] {
        let got = w.refused(Kind::QuestionAnswer, payload);
        assert!(got.contains(why), "{got} does not contain {why}");
    }
}

#[test]
fn rescue_drop_records_1_event_for_a_ref_that_git_confirms() {
    let mut w = world();
    let payload = json!({
        "repo": w.root,
        "name": RESCUE_REF,
        "tip": TIP,
        "class": "unique",
        "bytes": 10,
    });
    let r = w.applied(Kind::RescueDrop, payload.clone());
    assert_eq!(r["recorded"], json!(true));
    let facts = format!("SELECT count(*) FROM events WHERE entity_id = '{RESCUE_REF}'");
    let n = w.count(&facts);
    assert_eq!(n, 1);
    let r = w.applied(Kind::RescueDrop, payload.clone());
    assert_eq!(r["recorded"], json!(false));
    assert_eq!(w.count(&facts), n);
    let mut moved = payload;
    moved["tip"] = json!("d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2");
    let why = w.refused(Kind::RescueDrop, moved);
    assert!(why.contains("not at"), "{why}");
}

#[test]
fn a_dry_run_of_ledger_import_keeps_no_row() {
    let mut w = world();
    let runs = w.count("SELECT count(*) FROM runs");
    let state = w.state.clone();
    let facts = horch_core::taskledger::import::gather(
        w.store.conn(),
        &horch_core::taskledger::git::Git::new("git"),
        &state,
    )
    .unwrap();
    let r = w.applied(
        Kind::LedgerImport,
        json!({ "state_root": state, "facts": facts, "dry_run": true }),
    );
    assert_eq!(r["report"]["dry_run"], json!(true));
    assert_eq!(w.count("SELECT count(*) FROM runs"), runs);
}
