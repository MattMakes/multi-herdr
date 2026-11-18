//! The apply of a request: its `requests` row goes from pending to applied
//! or refused in 1 transaction, and a seen `request_id` gets its stored
//! result with no second effect.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 12 of fleetd-p5 fills it.
//!
//! [`handle`] uses 2 write transactions, and each checks the generation of
//! the `fleetd` row first:
//!
//! 1. It reads the row of the `request_id`. A row that is applied or
//!    refused gives its stored reply, and a row with another payload hash
//!    gives a refusal. Neither writes. With no row, it writes the `pending`
//!    row.
//! 2. It reads the row again. When it is still `pending`, it runs the
//!    handler of the kind and writes the decision of the row in the same
//!    transaction: `applied` with the result, or `refused` with the reason.
//!
//! A crash between the 2 transactions leaves a `pending` row. The next
//! copy of the request (a retry, or the request file, which stays until its
//! row is decided) applies it 1 time.
//!
//! A handler writes rows only. An effect is an intended row that the pass
//! carries out after the commit (A24).

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde_json::Value;

use crate::fleetd::fleet_runtime::adapters::fleetd_row;
use crate::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use crate::fleetd::fleet_runtime::adapters::requests_store::{self as store, Stored};
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::domain::authorize::Decision;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::runtime::fault::Faults;

/// The reason of the refusal of a kind with no handler.
pub const NOT_HANDLED: &str = "not handled in this version";

/// The savepoint of a handler, so that an error of the handler rolls back
/// its rows and keeps the transaction of the decision.
const HANDLER_SAVEPOINT: &str = "request_handler";

/// The fault point after the rows of the decision of a request, before
/// their commit: the `pending` row stays (`p5_requests.rs`).
pub const ABORT_BEFORE_REQUEST_COMMIT: &str = "abort-before-request-commit";

/// 1 handler: it writes the rows of 1 request in `tx` from its payload and
/// returns the result of the request.
pub type Handler = Box<dyn Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync>;

/// The handler of each kind that this version handles, and the armed
/// fault points of the request path.
#[derive(Default)]
pub struct Handlers {
    kinds: HashMap<Kind, Handler>,
    faults: Faults,
}

impl Handlers {
    /// No handler.
    pub fn new() -> Handlers {
        Handlers::default()
    }

    /// Arm the fault points `faults` of the request path (`HORCH_FAULT`).
    pub fn with_faults(mut self, faults: Faults) -> Handlers {
        self.faults = faults;
        self
    }

    /// The armed fault points of the request path.
    pub fn faults(&self) -> &Faults {
        &self.faults
    }

    /// Set the handler of `kind` to `handler`.
    pub fn insert(
        &mut self,
        kind: Kind,
        handler: impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static,
    ) {
        self.kinds.insert(kind, Box::new(handler));
    }
}

/// Handle `request` from `caller` with `decision`, while `guard` is the
/// generation of the `fleetd` row. It gives the reply of the request:
/// stored for a seen `request_id`, else new. `caller` is the peer user or
/// the owner of the request file, `None` when the fact is unknown.
/// [`DoorError::GenerationLost`] writes nothing.
pub fn handle(
    conn: &mut Connection,
    guard: Generation,
    request: &Request,
    caller: Option<&str>,
    decision: &Decision,
    handlers: &Handlers,
) -> Result<Reply, DoorError> {
    let reply = |outcome| Reply {
        request_id: request.request_id.clone(),
        outcome,
    };
    let recorded = in_transaction(conn, guard, |tx| {
        record(tx, request, caller.unwrap_or(store::UNKNOWN_CALLER))
    })?;
    if let Some(outcome) = recorded {
        return Ok(reply(outcome));
    }
    let outcome = in_transaction(conn, guard, |tx| {
        let outcome = decide(tx, request, decision, handlers)?;
        handlers.faults.abort_if(ABORT_BEFORE_REQUEST_COMMIT);
        Ok(outcome)
    })?;
    Ok(reply(outcome))
}

/// Run `f` in 1 immediate transaction that first checks `guard`. It
/// commits when `f` returns `Ok`, and rolls back otherwise.
fn in_transaction<T>(
    conn: &mut Connection,
    guard: Generation,
    f: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<T, DoorError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| DoorError::Ledger(e.into()))?;
    fleetd_row::check_generation(&tx, guard)?;
    let out = f(&tx)?;
    tx.commit().map_err(|e| DoorError::Ledger(e.into()))?;
    Ok(out)
}

/// The reply of a seen `request_id` that needs no apply, or `None` after
/// it writes the `pending` row of a new one or finds a `pending` row with
/// the same payload.
fn record(tx: &Transaction<'_>, request: &Request, caller: &str) -> Result<Option<Outcome>> {
    match store::get(tx, request.request_id.as_str())? {
        None => {
            store::insert_pending(tx, request, caller)?;
            Ok(None)
        }
        Some(row) => Ok(seen(&row, request)),
    }
}

/// The reply of the seen row `row` for `request`, or `None` when the row
/// is `pending` with the same payload.
fn seen(row: &store::Row, request: &Request) -> Option<Outcome> {
    if row.payload_sha256 != request.payload_sha256() {
        return Some(Outcome::Refused(format!(
            "request_id {} is known with another payload",
            request.request_id
        )));
    }
    match &row.stored {
        Stored::Pending => None,
        Stored::Applied(result) => Some(Outcome::Applied(result.clone())),
        Stored::Refused(reason) => Some(Outcome::Refused(reason.clone())),
    }
}

/// Apply or refuse the `pending` row of `request` and write its decision,
/// in `tx`. A row that another caller decided first gives its stored reply.
fn decide(
    tx: &Transaction<'_>,
    request: &Request,
    decision: &Decision,
    handlers: &Handlers,
) -> Result<Outcome> {
    let id = request.request_id.as_str();
    if let Some(row) = store::get(tx, id)? {
        if let Some(outcome) = seen(&row, request) {
            return Ok(outcome);
        }
    }
    match decision {
        Decision::Refused { reason } => refuse(tx, id, reason.clone()),
        Decision::Authorized { auth } => match apply(tx, request, handlers)? {
            Ok(result) => {
                store::set_applied(tx, id, auth, &result)?;
                Ok(Outcome::Applied(result))
            }
            Err(reason) => refuse(tx, id, reason),
        },
    }
}

/// Move the `pending` row of `id` to `refused` with `reason`, in `tx`.
fn refuse(tx: &Transaction<'_>, id: &str, reason: String) -> Result<Outcome> {
    store::set_refused(tx, id, &reason)?;
    Ok(Outcome::Refused(reason))
}

/// Run the handler of the kind of `request` in a savepoint of `tx`. The
/// inner `Err` is the reason of a refusal: no handler, or the error of the
/// handler, whose rows roll back. The outer `Err` is a fault of the ledger.
fn apply(
    tx: &Transaction<'_>,
    request: &Request,
    handlers: &Handlers,
) -> Result<Result<Value, String>> {
    let Some(handler) = handlers.kinds.get(&request.kind) else {
        return Ok(Err(NOT_HANDLED.to_string()));
    };
    tx.execute_batch(&format!("SAVEPOINT {HANDLER_SAVEPOINT}"))?;
    let payload = Value::Object(request.payload.clone());
    match handler(tx, &payload) {
        Ok(result) => {
            tx.execute_batch(&format!("RELEASE {HANDLER_SAVEPOINT}"))?;
            Ok(Ok(result))
        }
        Err(e) => {
            tx.execute_batch(&format!(
                "ROLLBACK TO {HANDLER_SAVEPOINT}; RELEASE {HANDLER_SAVEPOINT}"
            ))?;
            Ok(Err(format!("{e:#}")))
        }
    }
}
