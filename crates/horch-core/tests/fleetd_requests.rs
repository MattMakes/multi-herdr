//! The `requests` rows: pending, then applied or refused in 1 transaction,
//! and the stored result for a seen `request_id`
//! (docs/tickets/fleetd-p5/12-requests-store-and-apply.md; RR-8, FM-15,
//! SR-8).
//!
//! Each test uses a temp ledger at generation 1 and a test handler that
//! counts its calls and writes 1 row of the table `probe` for each call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::adapters::requests_store;
use horch_core::fleetd::fleet_runtime::application::ledger::DoorError;
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::{
    authorize, Decision, RequestFacts, Source,
};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Class, Kind, RequestId};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";
const AUTH: &str = "token; request file";
const ME: &str = "me";

/// A temp ledger at generation 1, with the table `probe` of the handler.
struct World {
    _temp: tempfile::TempDir,
    conn: Connection,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(temp.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    conn.execute_batch("CREATE TABLE probe (request TEXT NOT NULL)")
        .unwrap();
    World { _temp: temp, conn }
}

fn request(kind: Kind, payload: Value) -> Request {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Request {
        request_id: RequestId::parse(ID).unwrap(),
        kind,
        payload,
        token: Some("t".into()),
    }
}

fn authorized() -> Decision {
    Decision::Authorized { auth: AUTH.into() }
}

/// Handlers with 1 handler for `kind`: it counts its calls, writes 1 row of
/// `probe`, and returns the count of its calls.
fn counting(kind: Kind) -> (Handlers, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let mut handlers = Handlers::new();
    handlers.insert(kind, move |tx, payload| {
        let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
        tx.execute(
            "INSERT INTO probe (request) VALUES (?1)",
            [payload.to_string()],
        )?;
        Ok(json!({ "calls": n }))
    });
    (handlers, calls)
}

fn send(w: &mut World, req: &Request, decision: &Decision, handlers: &Handlers) -> Reply {
    handle(
        &mut w.conn,
        Generation(1),
        req,
        Some(ME),
        decision,
        handlers,
    )
    .unwrap()
}

fn probes(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM probe", [], |r| r.get(0))
        .unwrap()
}

fn requests(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap()
}

/// The columns of the row of [`ID`] that the tests check.
#[derive(Debug, PartialEq)]
struct Row {
    kind: String,
    caller: String,
    class: String,
    auth: Option<String>,
    state: String,
    result: Option<Value>,
    reason: Option<String>,
    decided: bool,
}

fn row(conn: &Connection) -> Option<Row> {
    conn.query_row(
        "SELECT kind, caller, class, auth, state, result_json, reason, decided_at
         FROM requests WHERE request_id = ?1",
        [ID],
        |r| {
            let result: Option<String> = r.get(5)?;
            let decided: Option<String> = r.get(7)?;
            Ok(Row {
                kind: r.get(0)?,
                caller: r.get(1)?,
                class: r.get(2)?,
                auth: r.get(3)?,
                state: r.get(4)?,
                result: result.map(|t| serde_json::from_str(&t).unwrap()),
                reason: r.get(6)?,
                decided: decided.is_some(),
            })
        },
    )
    .optional()
    .unwrap()
}

fn refused_reason(reply: &Reply) -> &str {
    match &reply.outcome {
        Outcome::Refused(reason) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn request_with_a_seen_request_id_returns_the_stored_result() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let req = request(Kind::TaskCancel, json!({ "task": "T-1" }));

    let first = send(&mut w, &req, &authorized(), &handlers);
    let second = send(&mut w, &req, &authorized(), &handlers);

    let expected = Reply {
        request_id: req.request_id.clone(),
        outcome: Outcome::Applied(json!({ "calls": 1 })),
    };
    assert_eq!(first, expected);
    assert_eq!(second, expected, "the second send gets the stored result");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "1 call of the handler");
    assert_eq!(probes(&w.conn), 1, "1 effect");
    assert_eq!(
        row(&w.conn).unwrap(),
        Row {
            kind: "task_cancel".into(),
            caller: ME.into(),
            class: "operator".into(),
            auth: Some(AUTH.into()),
            state: "applied".into(),
            result: Some(json!({ "calls": 1 })),
            reason: None,
            decided: true,
        }
    );
}

#[test]
fn known_request_id_with_another_payload_hash_is_refused() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let first = request(Kind::TaskCancel, json!({ "task": "T-1" }));
    let other = request(Kind::TaskCancel, json!({ "task": "T-2" }));

    send(&mut w, &first, &authorized(), &handlers);
    let before = row(&w.conn).unwrap();
    let reply = send(&mut w, &other, &authorized(), &handlers);

    assert!(
        refused_reason(&reply).contains("another payload"),
        "{reply:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no second call");
    assert_eq!(probes(&w.conn), 1);
    assert_eq!(
        row(&w.conn).unwrap(),
        before,
        "the stored row does not change"
    );
    let hash: String = w
        .conn
        .query_row(
            "SELECT payload_sha256 FROM requests WHERE request_id = ?1",
            [ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash, first.payload_sha256());
}

#[test]
fn refused_request_returns_its_reason() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let req = request(Kind::TaskCancel, json!({ "task": "T-1" }));
    let refused = Decision::Refused {
        reason: "the caller is unknown".into(),
    };

    let first = send(&mut w, &req, &refused, &handlers);
    // A later send of the same request gets the stored refusal, also when
    // its own decision authorizes it.
    let second = send(&mut w, &req, &authorized(), &handlers);

    assert_eq!(refused_reason(&first), "the caller is unknown");
    assert_eq!(second, first, "the second send gets the same reason");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "a refusal has no effect");
    assert_eq!(probes(&w.conn), 0);
    let stored = row(&w.conn).unwrap();
    assert_eq!(stored.state, "refused");
    assert_eq!(stored.reason.as_deref(), Some("the caller is unknown"));
    assert_eq!(stored.auth, None);
    assert_eq!(stored.result, None);
    assert!(stored.decided);
}

#[test]
fn pending_row_of_a_crash_is_applied_once_when_the_id_comes_again() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let req = request(Kind::TaskCancel, json!({ "task": "T-1" }));
    // A crash after the pending row and before the apply leaves this row.
    {
        let tx = w
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        requests_store::insert_pending(&tx, &req, ME).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(row(&w.conn).unwrap().state, "pending");

    let first = send(&mut w, &req, &authorized(), &handlers);
    let second = send(&mut w, &req, &authorized(), &handlers);

    assert_eq!(first.outcome, Outcome::Applied(json!({ "calls": 1 })));
    assert_eq!(second, first);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "applied 1 time");
    assert_eq!(probes(&w.conn), 1);
    assert_eq!(requests(&w.conn), 1);
    assert_eq!(row(&w.conn).unwrap().state, "applied");
}

#[test]
fn handler_error_refuses_the_request_and_rolls_back_its_rows() {
    let mut w = world();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let mut handlers = Handlers::new();
    handlers.insert(Kind::TaskCancel, move |tx, _payload| {
        seen.fetch_add(1, Ordering::SeqCst);
        tx.execute("INSERT INTO probe (request) VALUES ('half')", [])?;
        anyhow::bail!("task T-1 is not in the ledger")
    });
    let req = request(Kind::TaskCancel, json!({ "task": "T-1" }));

    let first = send(&mut w, &req, &authorized(), &handlers);
    let second = send(&mut w, &req, &authorized(), &handlers);

    assert!(
        refused_reason(&first).contains("task T-1 is not in the ledger"),
        "{first:?}"
    );
    assert_eq!(second, first, "the second send gets the stored refusal");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(probes(&w.conn), 0, "the rows of the handler roll back");
    let stored = row(&w.conn).unwrap();
    assert_eq!(stored.state, "refused");
    assert_eq!(stored.auth, None, "a refusal has no auth");
    assert_eq!(stored.reason.as_deref(), Some(refused_reason(&first)));
}

#[test]
fn wrong_generation_writes_nothing() {
    let mut w = world();
    w.conn
        .execute("UPDATE fleetd SET generation = 2 WHERE id = 1", [])
        .unwrap();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let req = request(Kind::TaskCancel, json!({ "task": "T-1" }));

    let err = handle(
        &mut w.conn,
        Generation(1),
        &req,
        Some(ME),
        &authorized(),
        &handlers,
    )
    .unwrap_err();

    assert!(
        matches!(
            err,
            DoorError::GenerationLost {
                held: Generation(1),
                found: Some(Generation(2))
            }
        ),
        "{err}"
    );
    assert_eq!(requests(&w.conn), 0, "no requests row");
    assert_eq!(probes(&w.conn), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn operator_request_without_a_valid_token_is_refused_and_recorded() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let mut req = request(Kind::TaskCancel, json!({ "task": "T-1" }));
    req.token = None;
    let decision = authorize(&RequestFacts {
        kind: req.kind.as_str().into(),
        source: Source::RequestFile,
        caller: Some(ME.into()),
        ledger_user: Some(ME.into()),
        token_valid: false,
    });
    let Decision::Refused { reason } = &decision else {
        panic!("no valid token is refused: {decision:?}");
    };

    let reply = send(&mut w, &req, &decision, &handlers);

    assert_eq!(refused_reason(&reply), reason);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(probes(&w.conn), 0);
    assert_eq!(
        row(&w.conn).unwrap(),
        Row {
            kind: "task_cancel".into(),
            caller: ME.into(),
            class: Class::Operator.as_str().into(),
            auth: None,
            state: "refused".into(),
            result: None,
            reason: Some(reason.clone()),
            decided: true,
        }
    );
}

#[test]
fn kind_with_no_handler_is_refused() {
    let mut w = world();
    let (handlers, calls) = counting(Kind::TaskCancel);
    let req = request(Kind::LedgerGc, json!({}));

    let reply = send(&mut w, &req, &authorized(), &handlers);

    assert_eq!(refused_reason(&reply), "not handled in this version");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let stored = row(&w.conn).unwrap();
    assert_eq!(stored.state, "refused");
    assert_eq!(stored.auth, None);
}
