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
//!   a task to `done`.
//! - A valid question file of the newest attempt of a `running` task ends
//!   the attempt with the outcome `asked`, which burns no attempt, and
//!   moves the task to `blocked_on_user`.
//! - A rejected file gets its row with the reason, and changes nothing
//!   else. fleetd stays healthy.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::ingested_store as store;
use crate::fleetd::task_execution::adapters::result_reader::{
    read_outbox, AttemptRef, FileRead, Payload, Read,
};
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
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
    let mut out = Vec::new();
    for attempt in attempts {
        let mut done = |path: &Path| store::has(conn, &key(path));
        let files = read_outbox(fleet_root, attempt, max_bytes, &mut done)?;
        if !files.is_empty() {
            out.push(OutboxRead {
                attempt: attempt.clone(),
                files,
            });
        }
    }
    Ok(out)
}

/// Part 2: record each file of `reads` 1 time and act on each valid file,
/// in the open transaction `tx`. The fleetd of generation `guard` writes.
pub fn ingest(tx: &Connection, guard: Option<Generation>, reads: &[OutboxRead]) -> Result<Report> {
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
                Read::Rejected { .. } => report.rejected.push(file.path.clone()),
                Read::Valid {
                    payload: Payload::Result(_),
                    ..
                } => {
                    if to_verifying(tx, guard, a)? {
                        report.to_verifying.push(a.task_id.clone());
                    }
                }
                Read::Valid {
                    payload: Payload::Question(_),
                    ..
                } => {
                    if asked(tx, guard, a)? {
                        report.to_blocked.push(a.task_id.clone());
                    }
                }
            }
        }
    }
    Ok(report)
}

/// The `ingested.path` of a file.
fn key(path: &Path) -> String {
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
fn newest(tx: &Connection, a: &AttemptRef) -> Result<Option<(String, String)>> {
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
fn moved(
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

/// A valid question file of `a`: the attempt ends with the outcome
/// `asked`, which burns no attempt, its worker row ends, and its `running`
/// task moves to `blocked_on_user`.
fn asked(tx: &Connection, guard: Option<Generation>, a: &AttemptRef) -> Result<bool> {
    let Some((worker, state)) = newest(tx, a)? else {
        return Ok(false);
    };
    let status = tasks::get(tx, &a.task_id)?.status;
    if status != TaskStatus::Running.as_str() || state == AttemptState::Ended.as_str() {
        return Ok(false);
    }
    let outcome = Outcome::Asked.as_str();
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
    moved(
        tx,
        guard,
        Entity::Task,
        &a.task_id,
        TaskStatus::BlockedOnUser.as_str(),
        &text,
    )
}
