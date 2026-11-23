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

#[derive(Clone)]
pub struct WorkerDeps {
    pub guard: Generation,
}

pub fn register(handlers: &mut Handlers, deps: &WorkerDeps) {
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnGateNotePayload {
    pub text: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
}

const GATE_NOTE: &str = "spawn gate: ";

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaunchFailedPayload {
    pub run_id: String,
    pub worker_id: String,
    pub reason: String,
}

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopPayload {
    pub run_id: String,
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
