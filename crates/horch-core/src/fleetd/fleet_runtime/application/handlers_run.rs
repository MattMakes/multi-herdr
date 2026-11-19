//! The handlers of the run and plan request kinds that stay after Phase 6:
//! `ledger_import` and `ledger_gc`. Ticket 47 of fleetd-p6 retires
//! `run_new`, `run_close`, `run_gate`, `task_add`, `map_add`, `task_solo`
//! and `plan_workers` (`domain::requests::RETIRED`).
//!
//! Requirements: RR-5, RR-8. Ticket 17 of fleetd-p5 fills it.
//!
//! Each handler writes the rows that its `horch` command writes today,
//! through the functions of `taskledger`, and returns the values that the
//! command prints. It runs no process, no herdr command and no git. The
//! command gathers the facts of git and the disk before it sends the
//! request, and the payload carries them. fleetd trusts such a fact only in
//! a request of class `operator` ([`operator_only`]).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rusqlite::Transaction;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::requests::{Class, Kind};
use crate::taskledger::import::{self, ImportFacts};

/// The size limit of a ticket that a request names, in bytes.
pub const INPUT_MAX_BYTES: u64 = 1 << 20;

/// The kinds of this file.
pub const KINDS: [Kind; 2] = [Kind::LedgerImport, Kind::LedgerGc];

/// 1 handler of this file.
type HandlerFn = fn(&Transaction<'_>, &Value) -> Result<Value>;

/// Put the handler of each kind of this file in `handlers`.
pub fn register(handlers: &mut Handlers) {
    let all: [(Kind, HandlerFn); 2] = [
        (Kind::LedgerImport, ledger_import),
        (Kind::LedgerGc, ledger_gc),
    ];
    for (kind, handler) in all {
        handlers.insert(kind, operator_only(kind, handler));
    }
}

/// `handler` for `kind`, which refuses each request when `kind` is not of
/// class `operator`: the payload of these kinds carries facts that the
/// command gathered, and only the operator is trusted to give them.
pub fn operator_only(
    kind: Kind,
    handler: impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static,
) -> impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static {
    move |tx, payload| {
        if kind.class() != Class::Operator {
            bail!(
                "`{}` is not an operator kind, so fleetd does not trust the facts of its payload",
                kind.as_str()
            );
        }
        handler(tx, payload)
    }
}

/// The payload as `T`. An unknown or missing field refuses the request.
fn args<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).context("the payload does not fit its kind")
}

// ─── ledger_import ──────────────────────────────────────────────────────

/// The savepoint of `horch ledger import --dry-run`: the import runs in it,
/// and its rows roll back.
const DRY_RUN_SAVEPOINT: &str = "ledger_import_dry_run";

/// `horch ledger import`: the old files under `state_root`, and the facts
/// of git and the disk that the command read. With `dry_run` (T-885) the
/// handler gives the report and keeps no row of the import.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportArgs {
    state_root: PathBuf,
    facts: ImportFacts,
    #[serde(default)]
    dry_run: bool,
}

fn ledger_import(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: ImportArgs = args(payload)?;
    if !a.dry_run {
        let report = import::import_from(tx, &a.state_root, &a.facts, false)?;
        return Ok(json!({ "report": report }));
    }
    tx.execute_batch(&format!("SAVEPOINT {DRY_RUN_SAVEPOINT}"))?;
    let report = import::import_from(tx, &a.state_root, &a.facts, true);
    tx.execute_batch(&format!(
        "ROLLBACK TO {DRY_RUN_SAVEPOINT}; RELEASE {DRY_RUN_SAVEPOINT}"
    ))?;
    Ok(json!({ "report": report? }))
}

// ─── ledger_gc ──────────────────────────────────────────────────────────

/// `horch ledger gc`: the command deleted the trash refs, which are git
/// refs and not ledger rows. The request records what it deleted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GcArgs {
    deleted: Vec<String>,
}

fn ledger_gc(_tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: GcArgs = args(payload)?;
    Ok(json!({ "deleted": a.deleted }))
}
