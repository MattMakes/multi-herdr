use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rusqlite::Transaction;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::requests::{Class, Kind};
use crate::taskledger::import::{self, ImportFacts};

pub const INPUT_MAX_BYTES: u64 = 1 << 20;

pub const KINDS: [Kind; 2] = [Kind::LedgerImport, Kind::LedgerGc];

type HandlerFn = fn(&Transaction<'_>, &Value) -> Result<Value>;

pub fn register(handlers: &mut Handlers) {
    let all: [(Kind, HandlerFn); 2] = [
        (Kind::LedgerImport, ledger_import),
        (Kind::LedgerGc, ledger_gc),
    ];
    for (kind, handler) in all {
        handlers.insert(kind, operator_only(kind, handler));
    }
}

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

fn args<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).context("the payload does not fit its kind")
}

const DRY_RUN_SAVEPOINT: &str = "ledger_import_dry_run";

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GcArgs {
    deleted: Vec<String>,
}

fn ledger_gc(_tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: GcArgs = args(payload)?;
    Ok(json!({ "deleted": a.deleted }))
}
