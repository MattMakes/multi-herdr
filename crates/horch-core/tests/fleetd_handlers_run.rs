use std::path::PathBuf;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_run::{
    operator_only, register, KINDS,
};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers, NOT_HANDLED};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Class, Kind, RequestId};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::store::Store;
use horch_core::taskledger::{import, schema, util};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

struct World {
    temp: tempfile::TempDir,
    conn: Connection,
    handlers: Handlers,
    next_id: u64,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("proj/tickets")).unwrap();
    let mut conn = Connection::open(temp.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let mut handlers = Handlers::new();
    register(&mut handlers);
    World {
        temp,
        conn,
        handlers,
        next_id: 0,
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.temp.path().join("proj")
    }

    fn request(&mut self, kind: Kind, payload: Value) -> Request {
        self.next_id += 1;
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        Request {
            request_id: RequestId::parse(&format!("6f1c2a3e-0b9d-4c55-8e7f-{:012x}", self.next_id))
                .unwrap(),
            kind,
            payload,
            token: Some("t".into()),
        }
    }

    fn send_request(&mut self, req: &Request) -> Reply {
        handle(
            &mut self.conn,
            Generation(1),
            req,
            Some("me"),
            &Decision::Authorized {
                auth: "token; request file".into(),
            },
            &self.handlers,
        )
        .unwrap()
    }

    fn send(&mut self, kind: Kind, payload: Value) -> Outcome {
        let req = self.request(kind, payload);
        self.send_request(&req).outcome
    }

    fn applied(&mut self, kind: Kind, payload: Value) -> Value {
        match self.send(kind, payload) {
            Outcome::Applied(v) => v,
            Outcome::Refused(r) => panic!("{} refused: {r}", kind.as_str()),
        }
    }

    fn refused(&mut self, kind: Kind, payload: Value) -> String {
        match self.send(kind, payload) {
            Outcome::Refused(r) => r,
            Outcome::Applied(v) => panic!("{} applied: {v}", kind.as_str()),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn old_state(&self) -> (PathBuf, Value) {
        let state = self.temp.path().join("state");
        std::fs::create_dir_all(state.join("worktrees")).unwrap();
        let records = json!([
            {"record_id": "a", "session_id": null, "agent": "claude", "tier": "opus",
             "model": "opus", "role": "opus-1", "status": "done", "task": "t1",
             "history": [], "created_at": "2026-01-01T00:00:00Z",
             "updated_at": "2026-01-01T00:00:00Z", "project": "/repo"},
            {"record_id": "b", "session_id": "s-b", "agent": "codex", "tier": "codex-sol",
             "model": "gpt", "role": "codex-sol-1", "status": "working", "task": "t2",
             "history": [], "created_at": "2026-01-02T00:00:00Z",
             "updated_at": "2026-01-02T00:00:00Z"}
        ]);
        std::fs::write(state.join("-repo.json"), records.to_string()).unwrap();
        let claim = json!([{
            "id": "c1", "record_id": "a",
            "path": self.root().join(".worktrees/gone").to_string_lossy(),
            "branch": "fleet/gone", "base": "main", "state": "open"
        }]);
        std::fs::write(state.join("worktrees/claims.json"), claim.to_string()).unwrap();
        let facts = import::gather(&self.conn, &Git::new("git"), &state).unwrap();
        (
            state.clone(),
            json!({ "state_root": state, "facts": facts }),
        )
    }
}

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, rusqlite::types::Value>(i))
            .map(|v| {
                v.map(|v| match v {
                    rusqlite::types::Value::Null => None,
                    rusqlite::types::Value::Integer(i) => Some(i.to_string()),
                    rusqlite::types::Value::Text(t) => Some(t),
                    other => Some(format!("{other:?}")),
                })
            })
            .collect()
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

const IMPORT_TABLES: [&str; 5] = [
    "SELECT source_kind, source_key, mapped_to IS NOT NULL, map_error FROM import_raw ORDER BY raw_id",
    "SELECT record_id FROM session_records ORDER BY record_id",
    "SELECT persona, status, legacy_record_id FROM workers ORDER BY legacy_record_id",
    "SELECT title, status FROM tasks ORDER BY title",
    "SELECT kind, detail FROM import_findings ORDER BY finding_id",
];

#[test]
fn ledger_import_and_gc_write_the_rows_of_today() {
    let mut w = world();
    let (state, payload) = w.old_state();
    let v = w.applied(Kind::LedgerImport, payload);
    assert_eq!(v["report"]["sessions"], 2);
    assert_eq!(v["report"]["worktrees"], 1);

    let twin = w.temp.path().join("twin.sqlite3");
    let mut store = Store::open(&twin).unwrap();
    let today = import::import(&mut store, &Git::new("git"), &state, false).unwrap();
    assert_eq!(serde_json::to_value(&today).unwrap(), v["report"]);
    for sql in IMPORT_TABLES {
        assert_eq!(rows(&w.conn, sql), rows(store.conn(), sql), "{sql}");
    }

    let (_, payload) = w.old_state();
    std::fs::write(state.join("-repo.json"), "[]").unwrap();
    let reason = w.refused(Kind::LedgerImport, payload);
    assert!(
        reason.contains("changed after the import facts were read"),
        "{reason}"
    );

    let events = w.count("SELECT count(*) FROM events");
    let v = w.applied(
        Kind::LedgerGc,
        json!({ "deleted": ["refs/horch/trash/a/20260101T000000Z"] }),
    );
    assert_eq!(v["deleted"][0], "refs/horch/trash/a/20260101T000000Z");
    assert_eq!(w.count("SELECT count(*) FROM events"), events);
}

fn effects(conn: &Connection) -> (Vec<i64>, Option<String>) {
    let counts = [
        "runs",
        "workers",
        "tasks",
        "task_deps",
        "run_gates",
        "events",
        "import_raw",
        "session_records",
        "import_findings",
        "worktrees",
    ]
    .iter()
    .map(|t| {
        conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect();
    let workspaces = conn
        .query_row("SELECT group_concat(workspace_id) FROM runs", [], |r| {
            r.get(0)
        })
        .unwrap();
    (counts, workspaces)
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    for kind in KINDS {
        let mut w = world();
        let payload = match kind {
            Kind::LedgerImport => w.old_state().1,
            Kind::LedgerGc => json!({ "deleted": ["refs/horch/trash/a/x"] }),
            other => panic!("{} is not a kind of this file", other.as_str()),
        };
        let req = w.request(kind, payload);
        let before = effects(&w.conn);
        let first = w.send_request(&req);
        assert!(
            matches!(first.outcome, Outcome::Applied(_)),
            "{}: {first:?}",
            kind.as_str()
        );
        let once = effects(&w.conn);
        assert!(
            kind == Kind::LedgerGc || once != before,
            "{} wrote no row",
            kind.as_str()
        );
        let second = w.send_request(&req);
        assert_eq!(second, first, "{}", kind.as_str());
        assert_eq!(effects(&w.conn), once, "{} applied 2 times", kind.as_str());
        assert_eq!(
            w.count("SELECT count(*) FROM requests"),
            1,
            "{}: 1 request_id is 1 row",
            kind.as_str()
        );
    }
}

#[test]
fn each_kind_of_this_file_is_an_operator_kind_with_a_handler() {
    let mut w = world();
    for kind in KINDS {
        assert_eq!(kind.class(), Class::Operator, "{}", kind.as_str());
        let reason = w.refused(kind, json!({ "unknown": 1 }));
        assert_ne!(reason, NOT_HANDLED, "{}", kind.as_str());
        assert!(
            reason.contains("the payload does not fit its kind"),
            "{reason}"
        );
    }
}

#[test]
fn a_handler_with_client_facts_refuses_a_worker_kind() {
    let mut w = world();
    let mut handlers = Handlers::new();
    handlers.insert(
        Kind::Progress,
        operator_only(Kind::Progress, |_, _| Ok(json!({}))),
    );
    w.handlers = handlers;
    let reason = w.refused(Kind::Progress, json!({}));
    assert!(reason.contains("not an operator kind"), "{reason}");
}
