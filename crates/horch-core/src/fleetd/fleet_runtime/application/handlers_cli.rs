//! The handlers of the 4 kinds of the last direct writers of the CLI
//! (T-885): `worker_progress`, `worker_done`, `run_adopt` and
//! `question_answer`. Before, each command wrote these rows itself.
//!
//! Requirements: RR-5, RR-8, FM-24.
//!
//! Each handler checks its payload as untrusted input: the form and the
//! size of each field, and that each id exists. Then it writes the rows
//! that its command wrote, through the same functions of `taskledger/` and
//! `fleetd/`, in the transaction of its request. It does no effect.

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

/// The most bytes of the text of 1 progress line or 1 answer.
pub const TEXT_MAX_BYTES: usize = 16 * 1024;

/// The most bytes of a workspace id or a mail id.
const ID_MAX_BYTES: usize = 128;

/// Register the handler of each kind of this module in `handlers`.
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

/// The payload of `kind`, with no unknown field.
fn parse<T: DeserializeOwned>(kind: Kind, payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone())
        .with_context(|| format!("a bad {} payload", kind.as_str()))
}

/// Refuse an empty `text`, or a `text` over [`TEXT_MAX_BYTES`].
fn check_text(what: &str, text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "the {what} is empty");
    ensure!(
        text.len() <= TEXT_MAX_BYTES,
        "the {what} has {} bytes; the limit is {TEXT_MAX_BYTES}",
        text.len()
    );
    Ok(())
}

/// Refuse a workspace id or a mail id `id` that is not 1 to
/// [`ID_MAX_BYTES`] characters of `[A-Za-z0-9._-]`.
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

/// The worker row `id`, which must exist.
fn worker(tx: &Connection, id: &str) -> Result<workers::Worker> {
    workers::find(tx, id)?.with_context(|| format!("the ledger has no worker {id}"))
}

// ─── worker_progress ────────────────────────────────────────────────────

/// `horch note` of a worker with no live attempt, and the compact job of a
/// compacted worker: 1 progress line on the worker row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProgress {
    pub worker_id: String,
    /// `progress` or `note`.
    pub kind: String,
    /// `agent` or `transcript`.
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

// ─── worker_done ────────────────────────────────────────────────────────

/// `horch done` of a pane that holds no task, such as the report role of a
/// controller: its live row ends `done`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDone {
    pub worker_id: String,
}

/// The row ends only when it holds no task. A row that holds a task, or
/// that ended before, changes nothing: `ended` is `false`.
fn worker_done(tx: &Connection, p: &WorkerDone) -> Result<Value> {
    let w = worker(tx, &p.worker_id)?;
    let ended =
        w.task_id.is_none() && workers::end(tx, &w.worker_id, WorkerStatus::Done, "done", None)?;
    Ok(json!({ "worker_id": w.worker_id, "ended": ended }))
}

// ─── run_adopt ──────────────────────────────────────────────────────────

/// `horch ledger status` from the orchestrator pane of `workspace`: a run
/// with no workspace adopts it, so its wakes go to that orchestrator.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAdopt {
    pub run_id: String,
    pub workspace: String,
}

/// `adopted` is `false` when the run has a workspace already.
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

// ─── question_answer ────────────────────────────────────────────────────

/// `horch tell <role> --answers <id>`: the answer of the mail question
/// `mail_id` of `workspace`.
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
