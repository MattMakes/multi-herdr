//! The ingest of `result.json` and of the question file (ticket 13).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol"
//! (protocol rules 1, 2 and 7), "Ledger" (`ingested`, I1), "Order inside 1
//! pass", SR-6, FM-6 and FM-23.
//!
//! - Part 1 of a pass calls [`read`] for each outbox that its outbox scan
//!   gave in its budget. A path with an `ingested` row is not read again.
//! - Part 2 calls [`ingest`] in its transaction, before
//!   `renew_or_expire_leases`, so a valid `result.json` of the pass wins
//!   over a lease expiry of the same pass (I1).
//! - Each file gets 1 `ingested` row, by its path and its sha256: 1 time.
//! - A valid `result.json` of the newest attempt of a `running` or
//!   `stalled` task moves the task to `verifying`. Only the verifier moves
//!   a task to `done`. A result with the key `unlanded` then parks the task
//!   with its tree (`verifier::park_unlanded`, RR-14).
//! - A valid question file of the newest attempt of a `running` task ends
//!   the attempt with the outcome `asked`, which burns no attempt, and
//!   moves the task to `blocked_on_user`. In the same transaction the file
//!   gives 1 `questions` row, owned by the sub-orchestrator of the feature
//!   of the task (RR-12, FM-35; [`questions::ask`]). A later question file
//!   of that attempt, while the task waits, gives 1 more row. A task with
//!   no idea has no sub-orchestrator: its question goes to you as 1
//!   needs-you item.
//! - A rejected file gets its row with the reason, and changes nothing
//!   else. A rejected question file also gives you 1 needs-you item, as
//!   its worker waits for an answer that nobody can give (OR-3). fleetd
//!   stays healthy.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use rusqlite::{Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::application::question_flow::feature_of_task;
use crate::fleetd::ideas::application::questions::{self, Ask};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::ideas::domain::questions::printable;
use crate::fleetd::task_execution::adapters::ingested_store as store;
use crate::fleetd::task_execution::adapters::result_reader::{
    read_outbox, AttemptRef, FileKind, FileRead, Payload, QuestionFile, Read,
};
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::{lease, verifier};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
use crate::taskledger::{tasks, util, workers};

/// The files that part 1 read from the outbox of 1 attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxRead {
    pub attempt: AttemptRef,
    pub files: Vec<FileRead>,
}

/// What 1 ingest did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The tasks that a result moved to `verifying`.
    pub to_verifying: Vec<String>,
    /// The tasks that a question moved to `blocked_on_user`.
    pub to_blocked: Vec<String>,
    /// The files that this ingest recorded as rejected.
    pub rejected: Vec<PathBuf>,
}

/// Part 1: read the new files of the outbox of each of `attempts` under
/// `fleet_root`, with the size limit `max_bytes`. A file with an
/// `ingested` row is not read. An outbox with no new file is left out.
pub fn read(
    conn: &Connection,
    fleet_root: &Path,
    attempts: &[AttemptRef],
    max_bytes: u64,
) -> Result<Vec<OutboxRead>> {
    // fleetd owns the ledger: each outbox file must be of its user.
    let owner = User::current().context("read the user of this process")?;
    let mut out = Vec::new();
    for attempt in attempts {
        let mut done = |path: &Path| store::has(conn, &key(path));
        let files = read_outbox(fleet_root, attempt, &owner, max_bytes, &mut done)?;
        if !files.is_empty() {
            out.push(OutboxRead {
                attempt: attempt.clone(),
                files,
            });
        }
    }
    Ok(out)
}

/// The kind of the needs-you item of a question file that has no row.
pub const QUESTION_FILE_KIND: &str = "question_file";

/// Part 2: record each file of `reads` 1 time and act on each valid file,
/// in the open transaction `tx`. The fleetd of generation `guard` writes.
/// A question gets the time limit `question_limit_s` on the fleet clock.
pub fn ingest(
    tx: &Connection,
    guard: Option<Generation>,
    reads: &[OutboxRead],
    question_limit_s: u64,
) -> Result<Report> {
    check_pass(tx, guard)?;
    let at = util::now();
    let mut report = Report::default();
    for outbox in reads {
        let a = &outbox.attempt;
        for file in &outbox.files {
            let (sha256, outcome, reason) = match &file.read {
                Read::Valid { sha256, .. } => (Some(sha256.as_str()), store::ACCEPTED, None),
                Read::Rejected { sha256, reason } => {
                    (sha256.as_deref(), store::REJECTED, Some(reason.as_str()))
                }
            };
            if !store::record_message(tx, &key(&file.path), sha256, outcome, reason, &at)? {
                continue;
            }
            match &file.read {
                Read::Rejected { reason, .. } => {
                    if file.kind == FileKind::Question {
                        rejected_question(tx, a, &file.path, reason)?;
                    }
                    report.rejected.push(file.path.clone());
                }
                Read::Valid {
                    payload: Payload::Result(r),
                    ..
                } => {
                    if to_verifying(tx, guard, a)? {
                        report.to_verifying.push(a.task_id.clone());
                        if let Some(reason) = &r.unlanded {
                            verifier::park_unlanded(tx, guard, &a.attempt_id, reason, &r.summary)?;
                        }
                    }
                }
                Read::Valid {
                    payload: Payload::Question(q),
                    ..
                } => {
                    let asking = Asking {
                        attempt: a,
                        path: &file.path,
                        file: q,
                        question_limit_s,
                    };
                    if asked(tx, guard, &asking)? {
                        report.to_blocked.push(a.task_id.clone());
                    }
                }
            }
        }
    }
    Ok(report)
}

/// The `ingested.path` of a file.
pub(super) fn key(path: &Path) -> String {
    path.display().to_string()
}

/// Refuse a connection with no open transaction, and a pass of another
/// generation, before the first write.
fn check_pass(tx: &Connection, guard: Option<Generation>) -> Result<()> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction.into());
    }
    if let Some(held) = guard {
        let found = door::generation(tx)?;
        if found != Some(held) {
            return Err(DoorError::GenerationLost { held, found }.into());
        }
    }
    Ok(())
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The worker and the state of attempt `a` when it is the newest attempt
/// of its task; `None` for an older attempt, or for no attempt row. A file
/// of an older attempt moves nothing.
pub(super) fn newest(tx: &Connection, a: &AttemptRef) -> Result<Option<(String, String)>> {
    Ok(tx
        .query_row(
            "SELECT a.worker_id, a.state FROM attempts a
             WHERE a.id = ?1 AND a.task_id = ?2
               AND a.rowid = (SELECT max(rowid) FROM attempts WHERE task_id = ?2)",
            [&a.attempt_id, &a.task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// The move of `entity` `id` to `to`. A refused move is recorded by the
/// door, and fails only its row: false.
pub(super) fn moved(
    tx: &Connection,
    guard: Option<Generation>,
    entity: Entity,
    id: &str,
    to: &str,
    text: &str,
) -> Result<bool> {
    match transition(tx, guard, entity, id, to, cause(text), &NeedsYouList) {
        Ok(m) => Ok(m == Moved::Changed),
        Err(DoorError::Refused { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// A valid `result.json` of `a`: its `running` or `stalled` task moves to
/// `verifying`.
fn to_verifying(tx: &Connection, guard: Option<Generation>, a: &AttemptRef) -> Result<bool> {
    if newest(tx, a)?.is_none() {
        return Ok(false);
    }
    let status = tasks::get(tx, &a.task_id)?.status;
    if status != TaskStatus::Running.as_str() && status != TaskStatus::Stalled.as_str() {
        return Ok(false);
    }
    let text = format!("result.json of attempt {} is valid", a.attempt_id);
    moved(
        tx,
        guard,
        Entity::Task,
        &a.task_id,
        TaskStatus::Verifying.as_str(),
        &text,
    )
}

struct Asking<'a> {
    attempt: &'a AttemptRef,
    path: &'a Path,
    file: &'a QuestionFile,
    question_limit_s: u64,
}

fn asked(tx: &Connection, guard: Option<Generation>, q: &Asking<'_>) -> Result<bool> {
    let a = q.attempt;
    let Some((worker, state)) = newest(tx, a)? else {
        return Ok(false);
    };
    let status = tasks::get(tx, &a.task_id)?.status;
    let outcome = Outcome::Asked.as_str();
    let ended = state == AttemptState::Ended.as_str();
    if status == TaskStatus::BlockedOnUser.as_str() && ended {
        if outcome_of(tx, &a.attempt_id)?.as_deref() == Some(outcome) {
            question_row(tx, q)?;
        }
        return Ok(false);
    }
    if status != TaskStatus::Running.as_str() || ended {
        return Ok(false);
    }
    let text = format!("attempt {} asked a question", a.attempt_id);
    attempts_store::set_ended(tx, &a.attempt_id, outcome, &util::now())?;
    moved(
        tx,
        guard,
        Entity::Attempt,
        &a.attempt_id,
        AttemptState::Ended.as_str(),
        &text,
    )?;
    let reason = format!("{outcome}: attempt {}", a.attempt_id);
    workers::end(tx, &worker, WorkerStatus::Incomplete, &reason, None)?;
    let blocked = moved(
        tx,
        guard,
        Entity::Task,
        &a.task_id,
        TaskStatus::BlockedOnUser.as_str(),
        &text,
    )?;
    if blocked {
        question_row(tx, q)?;
    }
    Ok(blocked)
}

fn question_row(tx: &Connection, q: &Asking<'_>) -> Result<()> {
    let a = q.attempt;
    if feature_of_task(tx, &a.task_id)?.is_none() {
        let text = format!(
            "Task {} asked a question, but it has no idea, so no sub-orchestrator owns the \
             question. The task stays in blocked_on_user until you run horch task requeue {} or \
             horch task cancel {}. The question: {}",
            a.task_id,
            a.task_id,
            a.task_id,
            printable(&q.file.text)
        );
        return question_file_item(tx, a, q.path, &text);
    }
    let asked_at = util::now();
    let ask = Ask {
        task_id: &a.task_id,
        attempt_id: &a.attempt_id,
        mail_id: q.file.mail_id.as_deref(),
        text: &q.file.text,
        asked_at: &asked_at,
    };
    let clock = lease::fleet_clock(tx)?;
    questions::ask(tx, &ask, clock, q.question_limit_s)?;
    Ok(())
}

fn outcome_of(tx: &Connection, id: &str) -> Result<Option<String>> {
    Ok(tx
        .query_row("SELECT outcome FROM attempts WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten())
}

fn rejected_question(tx: &Connection, a: &AttemptRef, path: &Path, reason: &str) -> Result<()> {
    let text = format!(
        "The question file {} of task {} is rejected, so its question has no row and no \
         owner: {}. fleetd does not read the file again.",
        path.display(),
        a.task_id,
        printable(reason)
    );
    question_file_item(tx, a, path, &text)
}

fn question_file_item(tx: &Connection, a: &AttemptRef, path: &Path, text: &str) -> Result<()> {
    needs_you::open_item(
        tx,
        &Write {
            key: &format!("{QUESTION_FILE_KIND}:{}", key(path)),
            kind: QUESTION_FILE_KIND,
            entity: Entity::Task.as_str(),
            entity_id: &a.task_id,
            text,
        },
    )
}
