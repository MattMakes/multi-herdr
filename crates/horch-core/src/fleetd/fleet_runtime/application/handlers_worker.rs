//! The handlers of the worker request kinds: `worker_stop`, and the
//! records of 1 spawn of fleetd: `spawn_gate_note`, `worker_launched`
//! and `worker_launch_failed`.
//!
//! Requirements: RR-5, RR-8. Ticket 19 of fleetd-p5. Ticket 48 of
//! fleetd-p6 retires `spawn`, `worker_reap`, `worker_new`, `worker_pid`,
//! `orchestrator_claim` and `orchestrator_stop`: fleetd dispatches,
//! launches and records each attempt itself.
//!
//! Each handler writes the rows that its `horch` command writes today,
//! through the same functions of `taskledger/` and `fleetd/`, in the
//! transaction of its request. It does no effect:
//!
//! - `spawn_gate_note`, `worker_launched` and `worker_launch_failed` are
//!   the records of 1 spawn of fleetd (`horch spawn --attempt` and the
//!   restart on a fallback) that `horch spawn` wrote itself before (ticket
//!   23 of fleetd-p5). Each handler checks that the worker row of its
//!   payload exists and is a row of its run.
//! - `session_records` writes the session records that 1 process changed
//!   ([`crate::execution::store::apply_session_records`], T-884).
//! - `worker_stop` ends the worker row. The pass stops each process of an
//!   ended row by its PID and start time, and closes its pane only with a
//!   fact that proves it (`commands::mark_orphans`).
//!
//! The payload holds the arguments of the command; the run is its id, as
//! the command resolves it. The result holds what the command prints.

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, Transaction};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::requests::{check_spawn_record, Kind};
use crate::fleetd::task_execution::application::commands as task_commands;
use crate::fleetd::task_execution::application::launch as push_launch;
use crate::taskledger::model::{Actor, EventKind, StopRequester, WorkerStatus};
use crate::taskledger::stop;
use crate::taskledger::util::{self, About};
use crate::taskledger::workers;

/// What the handlers of this module read outside their payload.
#[derive(Clone)]
pub struct WorkerDeps {
    /// The generation of this fleetd.
    pub guard: Generation,
}

/// Register the handler of each kind of this module in `handlers`.
pub fn register(handlers: &mut Handlers, _deps: &WorkerDeps) {
    handlers.insert(Kind::SpawnGateNote, |tx, p| {
        spawn_gate_note(tx, &record(Kind::SpawnGateNote, p)?)
    });
    handlers.insert(Kind::WorkerLaunched, |tx, p| {
        worker_launched(tx, &record(Kind::WorkerLaunched, p)?)
    });
    let d = deps.clone();
    handlers.insert(Kind::WorkerLaunchFailed, move |tx, p| {
        worker_launch_failed(tx, &d, &record(Kind::WorkerLaunchFailed, p)?)
    });
    handlers.insert(Kind::SessionRecords, |tx, p| {
        crate::execution::store::apply_session_records(tx, p)
    });
    handlers.insert(Kind::WorkerStop, |tx, p| worker_stop(tx, &parse(p)?));
}

fn parse<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).context("the payload does not fit its kind")
}

// ─── the records of 1 spawn ──────────────────────────────────────────────

/// The payload of a kind of the records of 1 spawn: an object of texts that
/// passes [`check_spawn_record`].
fn record<T: DeserializeOwned>(kind: Kind, payload: &Value) -> Result<T> {
    let Value::Object(map) = payload else {
        bail!("the payload of {} is not an object", kind.as_str());
    };
    let mut fields = Vec::with_capacity(map.len());
    for (key, value) in map {
        let Value::String(text) = value else {
            bail!(
                "the payload field `{key}` of {} is not a text",
                kind.as_str()
            );
        };
        fields.push((key.as_str(), text.as_str()));
    }
    check_spawn_record(kind, &fields)?;
    parse(payload)
}

/// The worker row `worker_id` of the run `run_id`. The payload names both,
/// so the row must exist and be a row of that run.
fn worker_of_run(tx: &Connection, run_id: &str, worker_id: &str) -> Result<workers::Worker> {
    let Some(w) = workers::find(tx, worker_id)? else {
        bail!("no worker row has the id {worker_id}");
    };
    ensure!(
        w.run_id.as_deref() == Some(run_id),
        "worker {worker_id} is not a worker of run {run_id}"
    );
    Ok(w)
}

/// The arguments of the record of 1 spawn-gate decision (TRF-41).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnGateNotePayload {
    /// `spawn gate: teammate=<t> outcome=<o> pool=<p> reason=<r>`.
    pub text: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
}

/// The prefix of the text of each spawn-gate note.
const GATE_NOTE: &str = "spawn gate: ";

/// 1 `note` event of a spawn-gate decision, about the run and the worker of
/// the spawn when it joins a run, as `horch spawn` wrote it.
fn spawn_gate_note(tx: &Transaction<'_>, p: &SpawnGateNotePayload) -> Result<Value> {
    ensure!(
        p.text.starts_with(GATE_NOTE),
        "the text of spawn_gate_note does not start with `{GATE_NOTE}`"
    );
    if let (Some(run), Some(worker)) = (&p.run_id, &p.worker_id) {
        worker_of_run(tx, run, worker)?;
    }
    util::event(
        tx,
        &About {
            run: p.run_id.as_deref(),
            task: None,
            worker: p.worker_id.as_deref(),
        },
        Actor::Horch,
        EventKind::Note,
        &p.text,
    )?;
    Ok(json!({ "run_id": p.run_id, "worker_id": p.worker_id }))
}

/// The arguments of the record of a pane that a spawn opened.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaunchedPayload {
    pub run_id: String,
    pub worker_id: String,
    pub pane: String,
    pub role: String,
    pub harness: String,
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
}

/// The pane of the worker is open and runs `horch worker <role>`: its pane,
/// its role and its launch, as `horch spawn` wrote them.
fn worker_launched(tx: &Transaction<'_>, p: &WorkerLaunchedPayload) -> Result<Value> {
    worker_of_run(tx, &p.run_id, &p.worker_id)?;
    workers::set_pane(tx, &p.worker_id, &p.pane)?;
    workers::set_launch(
        tx,
        &p.worker_id,
        &p.role,
        &p.harness,
        &p.model,
        p.effort.as_deref(),
    )?;
    Ok(json!({ "worker_id": p.worker_id, "pane": p.pane }))
}

/// The arguments of the record of a spawn that failed before its worker ran.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaunchFailedPayload {
    pub run_id: String,
    pub worker_id: String,
    pub reason: String,
}

/// The spawn failed before the worker ran: the row ends `launch_failed`, as
/// `horch spawn` wrote it. Its push attempt in `launching` ends `crashed`,
/// so rung 6 queues the task again at once.
fn worker_launch_failed(
    tx: &Transaction<'_>,
    deps: &WorkerDeps,
    p: &WorkerLaunchFailedPayload,
) -> Result<Value> {
    worker_of_run(tx, &p.run_id, &p.worker_id)?;
    let why = format!("its spawn failed: {}", p.reason);
    let ended = push_launch::end_failed_spawn(tx, Some(deps.guard), &p.worker_id, &why)?;
    if ended.is_empty() {
        workers::launch_failed(tx, &p.worker_id, &p.reason)?;
    }
    Ok(json!({ "worker_id": p.worker_id, "ended": ended }))
}

// ─── worker_stop ────────────────────────────────────────────────────────

/// The arguments of `horch ledger stop`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopPayload {
    pub run_id: String,
    /// A worker id or role.
    pub worker: String,
    pub reason: String,
}

fn worker_stop(tx: &Transaction<'_>, p: &StopPayload) -> Result<Value> {
    let w = workers::resolve(tx, &p.run_id, &p.worker)?;
    let by = StopRequester::Orchestrator;
    stop::check_stoppable(tx, &w, by, &p.reason)?;
    task_commands::end_stopped_attempt(tx, &w.worker_id, &p.reason)?;
    let requeued = stop::end_row_in(tx, &w, by, &p.reason, WorkerStatus::Stopped)?;
    Ok(json!({
        "worker_id": w.worker_id,
        "worker": w.label(),
        "requeued": requeued,
        "steps": [],
    }))
}
