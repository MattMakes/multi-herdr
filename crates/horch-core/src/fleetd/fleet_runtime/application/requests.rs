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

pub const NOT_HANDLED: &str = "not handled in this version";

const HANDLER_SAVEPOINT: &str = "request_handler";

pub const ABORT_BEFORE_REQUEST_COMMIT: &str = "abort-before-request-commit";

pub type Handler = Box<dyn Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync>;

#[derive(Default)]
pub struct Handlers {
    kinds: HashMap<Kind, Handler>,
    faults: Faults,
}

impl Handlers {
    pub fn new() -> Handlers {
        Handlers::default()
    }

    pub fn with_faults(mut self, faults: Faults) -> Handlers {
        self.faults = faults;
        self
    }

    pub fn faults(&self) -> &Faults {
        &self.faults
    }

    pub fn insert(
        &mut self,
        kind: Kind,
        handler: impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static,
    ) {
        self.kinds.insert(kind, Box::new(handler));
    }
}

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

fn record(tx: &Transaction<'_>, request: &Request, caller: &str) -> Result<Option<Outcome>> {
    match store::get(tx, request.request_id.as_str())? {
        None => {
            store::insert_pending(tx, request, caller)?;
            Ok(None)
        }
        Some(row) => Ok(seen(&row, request)),
    }
}

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

fn refuse(tx: &Transaction<'_>, id: &str, reason: String) -> Result<Outcome> {
    store::set_refused(tx, id, &reason)?;
    Ok(Outcome::Refused(reason))
}

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
