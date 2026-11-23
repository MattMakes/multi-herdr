use anyhow::{bail, ensure, Context, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::fleetd::ideas::application::questions;
use crate::taskledger::model::{EventKind, ProgressSource, RunStatus, WorkerStatus};
use crate::taskledger::{runs, workers};

pub const TEXT_MAX_BYTES: usize = 16 * 1024;

const ID_MAX_BYTES: usize = 128;

pub fn register(handlers: &mut Handlers) {
    handlers.insert(Kind::WorkerProgress, |tx, p| {
        worker_progress(tx, &parse(Kind::WorkerProgress, p)?)
    });
    handlers.insert(Kind::WorkerDone, |tx, p| {
        worker_done(tx, &parse(Kind::WorkerDone, p)?)
    });
    handlers.insert(Kind::RunAdopt, |tx, p| {
        run_adopt(tx, &parse(Kind::RunAdopt, p)?)
    });
    handlers.insert(Kind::QuestionAnswer, |tx, p| {
        question_answer(tx, &parse(Kind::QuestionAnswer, p)?)
    });
}

fn parse<T: DeserializeOwned>(kind: Kind, payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone())
        .with_context(|| format!("a bad {} payload", kind.as_str()))
}

fn check_text(what: &str, text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "the {what} is empty");
    ensure!(
        text.len() <= TEXT_MAX_BYTES,
        "the {what} has {} bytes; the limit is {TEXT_MAX_BYTES}",
        text.len()
    );
    Ok(())
}

fn check_id(what: &str, id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= ID_MAX_BYTES
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "the {what} {id:?} is not valid: it is not 1 to {ID_MAX_BYTES} characters of [A-Za-z0-9._-]"
    );
    Ok(())
}

fn worker(tx: &Connection, id: &str) -> Result<workers::Worker> {
    workers::find(tx, id)?.with_context(|| format!("the ledger has no worker {id}"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProgress {
    pub worker_id: String,
    pub kind: String,
    pub source: String,
    pub text: String,
}

fn worker_progress(tx: &Connection, p: &WorkerProgress) -> Result<Value> {
    let kind = EventKind::parse(&p.kind)?;
    if !matches!(kind, EventKind::Progress | EventKind::Note) {
        bail!("the kind {} is not progress or note", p.kind);
    }
    let source = ProgressSource::parse(&p.source)?;
    check_text("progress text", &p.text)?;
    worker(tx, &p.worker_id)?;
    workers::progress(tx, &p.worker_id, source, kind, &p.text)?;
    Ok(json!({ "worker_id": p.worker_id }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDone {
    pub worker_id: String,
}

fn worker_done(tx: &Connection, p: &WorkerDone) -> Result<Value> {
    let w = worker(tx, &p.worker_id)?;
    let ended =
        w.task_id.is_none() && workers::end(tx, &w.worker_id, WorkerStatus::Done, "done", None)?;
    Ok(json!({ "worker_id": w.worker_id, "ended": ended }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAdopt {
    pub run_id: String,
    pub workspace: String,
}

fn run_adopt(tx: &Connection, p: &RunAdopt) -> Result<Value> {
    check_id("workspace", &p.workspace)?;
    let run = runs::get(tx, &p.run_id)?;
    ensure!(
        run.status == RunStatus::Active.as_str(),
        "run {} is {}; only an active run adopts a workspace",
        run.slug,
        run.status
    );
    let adopted = runs::adopt_workspace(tx, &run.run_id, &p.workspace)?;
    Ok(json!({ "run_id": run.run_id, "slug": run.slug, "adopted": adopted }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswer {
    pub workspace: String,
    pub mail_id: String,
    pub text: String,
}

fn question_answer(tx: &Connection, p: &QuestionAnswer) -> Result<Value> {
    check_id("workspace", &p.workspace)?;
    check_id("mail id", &p.mail_id)?;
    check_text("answer", &p.text)?;
    let Some(id) = questions::answer_mail(tx, &p.mail_id, &p.text)? else {
        bail!(
            "no question row waits for an answer to mail {}: fleetd did not ingest its question \
             file yet, the question has its answer, or the sender is in no attempt",
            p.mail_id
        );
    };
    Ok(json!({ "question_id": id }))
}
