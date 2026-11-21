//! The ingest of the outbox files of each live attempt: `progress.jsonl` to
//! its last complete line, and the handoff result, each 1 time.
//!
//! Requirements: SR-6, RR-8, FM-23. `docs/specs/fleetd/design.md`,
//! "Workspace layout and file protocol" (protocol rules 1, 2 and 7) and
//! "Ledger" (`ingested`); `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q4.
//!
//! - Part 1 of a pass calls [`read`] for the outboxes of its outbox scan.
//!   Each file passes the owner and mode checks of the protected file
//!   reader. A path with a `rejected` row is not read again.
//! - Part 2 calls [`ingest()`] in its transaction. `result.json` and the
//!   question files go to [`super::ingest`]. Each file is ingested 1 time:
//!   a message by its `ingested` row, the log by its offset, which moves
//!   only from the offset that part 1 read.
//! - A progress line becomes the rows of `horch ledger progress`: the
//!   progress of the worker, its event, and the activity of a live attempt
//!   on the fleet clock. A line that fails is skipped with 1 event.
//! - A `context-warned` line becomes 1 `context-warned` event of the session
//!   record of the worker (G5): a process of the attempt cannot write it.
//! - The handoff result (Q4) ends its attempt with the outcome
//!   `handed_off`, burns no attempt, and moves the task to `ready`. The
//!   handoff path is kept only when its real path is inside the worktree or
//!   the task folder of the attempt; `START.md` of the next attempt names
//!   it ([`handoff_of`]).

use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_platform::channel::User;
use rusqlite::{params, Connection, OptionalExtension};

use crate::compaction::policy::EVENT_WARNED;
use crate::execution::store::record_history_in;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::task_execution::adapters::ingested_store as store;
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::outbox_reader::{
    handoff_inside, progress_path, read_progress, Line, ProgressFile, ProgressLimits, ProgressRead,
};
use crate::fleetd::task_execution::adapters::result_reader::{
    read_outbox, AttemptRef, FileRead, Payload, Read,
};
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::ingest::{self, key, moved, newest};
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::domain::outbox::LineKind;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fleetd::worktree_custody::adapters::sqlite as trees;
use crate::taskledger::model::{
    Actor, EndReason, EventKind, ProgressSource, TaskStatus, WorkerStatus,
};
use crate::taskledger::util::{self, About};
use crate::taskledger::{tasks, workers};

/// The size and line limits of the outbox files (`fleet.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub result_max_bytes: u64,
    pub progress_max_bytes: u64,
    pub progress_max_lines: u64,
    pub progress_line_max_bytes: u64,
}

impl Limits {
    pub fn of(config: &FleetConfig) -> Limits {
        Limits {
            result_max_bytes: config.result_max_bytes,
            progress_max_bytes: config.progress_max_bytes,
            progress_max_lines: config.progress_max_lines,
            progress_line_max_bytes: config.progress_line_max_bytes,
        }
    }

    fn progress(&self) -> ProgressLimits {
        ProgressLimits {
            max_bytes: self.progress_max_bytes,
            max_lines: self.progress_max_lines,
            line_max_bytes: self.progress_line_max_bytes,
        }
    }
}

/// A valid handoff `result.json` (Q4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    /// The path of `result.json`.
    pub path: PathBuf,
    pub sha256: String,
    /// The real path of the handoff file, or why fleetd does not use it.
    pub file: Result<String, String>,
}

/// What part 1 read from the outbox of 1 attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxRead {
    pub attempt: AttemptRef,
    /// `result.json` that is not a handoff, and the question files.
    pub messages: Vec<FileRead>,
    pub handoff: Option<Handoff>,
    pub progress: Option<ProgressFile>,
}

/// What 1 ingest did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The tasks that a result moved to `verifying`.
    pub to_verifying: Vec<String>,
    /// The tasks that a question moved to `blocked_on_user`.
    pub to_blocked: Vec<String>,
    /// The tasks whose attempt handed off.
    pub handed_off: Vec<String>,
    /// The files that this ingest recorded as rejected.
    pub rejected: Vec<PathBuf>,
    /// The progress lines that this ingest recorded.
    pub progress_lines: u64,
    /// The progress lines that this ingest skipped, each with 1 event.
    pub skipped_lines: u64,
}

/// Part 1: read the new outbox files of each of `attempts` under
/// `fleet_root`. `owner` is the user that owns the ledger. An outbox with
/// nothing new is left out.
pub fn read(
    conn: &Connection,
    fleet_root: &Path,
    attempts: &[AttemptRef],
    owner: &User,
    limits: &Limits,
) -> Result<Vec<OutboxRead>> {
    let mut out = Vec::new();
    for attempt in attempts {
        let mut done = |path: &Path| store::has(conn, &key(path));
        let files = read_outbox(
            fleet_root,
            attempt,
            owner,
            limits.result_max_bytes,
            &mut done,
        )?;
        let mut messages = Vec::new();
        let mut handoff = None;
        for file in files {
            match handoff_path(&file) {
                Some((sha256, path)) => {
                    let roots = handoff_roots(conn, fleet_root, attempt)?;
                    handoff = Some(Handoff {
                        sha256: sha256.to_string(),
                        file: handoff_inside(path, &roots),
                        path: file.path,
                    });
                }
                None => messages.push(file),
            }
        }
        let progress = match log_offset(conn, &progress_path(fleet_root, attempt))? {
            Some(offset) => read_progress(fleet_root, attempt, owner, &limits.progress(), offset)?,
            None => None,
        };
        if !messages.is_empty() || handoff.is_some() || progress.is_some() {
            out.push(OutboxRead {
                attempt: attempt.clone(),
                messages,
                handoff,
                progress,
            });
        }
    }
    Ok(out)
}

/// The sha256 and the handoff path of a valid handoff `result.json`.
fn handoff_path(file: &FileRead) -> Option<(&str, &str)> {
    match &file.read {
        Read::Valid {
            sha256,
            payload: Payload::Result(result),
        } => result
            .handoff
            .as_deref()
            .map(|path| (sha256.as_str(), path)),
        _ => None,
    }
}

/// The folders that may hold the handoff file of `attempt`: its task
/// folder, and the worktree of its task when it has one.
fn handoff_roots(conn: &Connection, fleet_root: &Path, a: &AttemptRef) -> Result<Vec<PathBuf>> {
    let mut roots = vec![fleet_root.join("tasks").join(&a.task_id)];
    if let Some(tree) = trees::of_task(conn, &a.task_id)? {
        roots.push(PathBuf::from(tree.path));
    }
    Ok(roots)
}

/// The offset to read the log `path` from: 0 with no row, the recorded
/// offset of an accepted log. `None`: the log is rejected and is not read
/// again.
fn log_offset(conn: &Connection, path: &Path) -> Result<Option<u64>> {
    let row: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT outcome, log_offset FROM ingested WHERE path = ?1",
            [key(path)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        None => Some(0),
        Some((outcome, offset)) if outcome == store::ACCEPTED => {
            Some(offset.and_then(|o| u64::try_from(o).ok()).unwrap_or(0))
        }
        Some(_) => None,
    })
}

/// Part 2: record each file of `reads` 1 time and act on it, in the open
/// transaction `tx`. The fleetd of generation `guard` writes. A question
/// gets the time limit `question_limit_s`.
pub fn ingest(
    tx: &Connection,
    guard: Option<Generation>,
    reads: &[OutboxRead],
    question_limit_s: u64,
) -> Result<Report> {
    // The message ingest checks the transaction and the generation first.
    let messages: Vec<ingest::OutboxRead> = reads
        .iter()
        .map(|r| ingest::OutboxRead {
            attempt: r.attempt.clone(),
            files: r.messages.clone(),
        })
        .collect();
    let done = ingest::ingest(tx, guard, &messages, question_limit_s)?;
    let mut report = Report {
        to_verifying: done.to_verifying,
        to_blocked: done.to_blocked,
        rejected: done.rejected,
        ..Report::default()
    };
    let at = util::now();
    for r in reads {
        if let Some(h) = &r.handoff {
            let path = key(&h.path);
            let new =
                store::record_message(tx, &path, Some(&h.sha256), store::ACCEPTED, None, &at)?;
            if new && hand_off(tx, guard, &r.attempt, &h.file)? {
                report.handed_off.push(r.attempt.task_id.clone());
            }
        }
        if let Some(p) = &r.progress {
            ingest_progress(tx, &r.attempt, p, &at, &mut report)?;
        }
    }
    Ok(report)
}

/// The outcome of a handed-off attempt: the word of today.
fn handed_off_word() -> &'static str {
    EndReason::HandedOff.as_str()
}

/// A valid handoff result of `a`: the attempt ends `handed_off`, its worker
/// ends `handed_off` with the handoff file when fleetd keeps it, and its
/// `running` task moves to `ready` with its attempt given back.
fn hand_off(
    tx: &Connection,
    guard: Option<Generation>,
    a: &AttemptRef,
    file: &Result<String, String>,
) -> Result<bool> {
    let Some((worker, state)) = newest(tx, a)? else {
        return Ok(false);
    };
    let status = tasks::get(tx, &a.task_id)?.status;
    if status != TaskStatus::Running.as_str() || state == AttemptState::Ended.as_str() {
        return Ok(false);
    }
    let text = match file {
        Ok(path) => format!(
            "handoff: attempt {} handed off; handoff file {path}",
            a.attempt_id
        ),
        Err(why) => format!(
            "handoff: attempt {} handed off; the handoff file is not used: {why}",
            a.attempt_id
        ),
    };
    let word = handed_off_word();
    attempts_store::set_ended(tx, &a.attempt_id, word, &util::now())?;
    moved(
        tx,
        guard,
        Entity::Attempt,
        &a.attempt_id,
        AttemptState::Ended.as_str(),
        &text,
    )?;
    let live = workers::get(tx, &worker)?.status()?.is_live();
    match file {
        Ok(path) if live => workers::hand_off(tx, &worker, path)?,
        _ => {
            workers::end(tx, &worker, WorkerStatus::Incomplete, word, None)?;
        }
    }
    // No attempt is burned: the next claim adds 1 to `attempts`, and the
    // floor moves with it.
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL,
            updated_at = ?2
         WHERE task_id = ?1",
        params![a.task_id, util::now()],
    )?;
    moved(
        tx,
        guard,
        Entity::Task,
        &a.task_id,
        TaskStatus::Ready.as_str(),
        &text,
    )
}

/// The handoff file that `START.md` of the next attempt of `task_id` names:
/// the kept handoff file of its last ended attempt, when that attempt
/// ended `handed_off` or `context_limit` (fleetd-p7 ticket 18).
pub fn handoff_of(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    Ok(handoffs_of(conn, [task_id])?.remove(task_id))
}

/// [`handoff_of`] of each task of `task_ids`, by task: 1 statement for any
/// count of tasks. A task with no handoff is not in the map.
pub fn handoffs_of<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let ids: Vec<&str> = task_ids.into_iter().collect();
    let sql = format!(
        "SELECT a.task_id, a.outcome, w.handoff_path, max(a.rowid) FROM attempts a
         JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.state = ?1 AND a.task_id IN ({})
         GROUP BY a.task_id",
        crate::taskledger::sql_words::marks(2, ids.len())
    );
    let ended = AttemptState::Ended.as_str();
    let mut values = vec![ended];
    values.extend(ids);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let handed = [handed_off_word(), Outcome::ContextLimit.as_str()];
    let mut out = std::collections::BTreeMap::new();
    for row in rows {
        let (task_id, outcome, path) = row?;
        if outcome.is_some_and(|o| handed.contains(&o.as_str())) {
            if let Some(path) = path {
                out.insert(task_id, path);
            }
        }
    }
    Ok(out)
}

/// Record the log `p` of `a` from the offset that part 1 read. A second
/// ingest of the same read finds another offset and does nothing.
fn ingest_progress(
    tx: &Connection,
    a: &AttemptRef,
    p: &ProgressFile,
    at: &str,
    report: &mut Report,
) -> Result<()> {
    match &p.read {
        ProgressRead::Rejected { from, reason } => {
            if move_offset(tx, &p.path, *from, *from, store::REJECTED, Some(reason), at)? {
                report.rejected.push(p.path.clone());
            }
        }
        ProgressRead::Lines { from, to, lines } => {
            if !move_offset(tx, &p.path, *from, *to, store::ACCEPTED, None, at)? {
                return Ok(());
            }
            let Some((worker, state)) = attempt_row(tx, a)? else {
                return Ok(());
            };
            let live = state != AttemptState::Ended.as_str();
            for line in lines {
                match &line.read {
                    Ok(l) if l.kind == LineKind::ContextWarned => {
                        record_warned(tx, &worker, &l.text)?;
                        report.progress_lines += 1;
                    }
                    Ok(l) => {
                        record_line(tx, a, &worker, live, l.kind, &l.text)?;
                        report.progress_lines += 1;
                    }
                    Err(reason) => {
                        skipped(tx, a, &worker, line, reason)?;
                        report.skipped_lines += 1;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Write the `ingested` row of the log `path`: offset `to` and `outcome`,
/// only when the row is new or is accepted at offset `from`. True when it
/// wrote.
fn move_offset(
    tx: &Connection,
    path: &Path,
    from: u64,
    to: u64,
    outcome: &str,
    reason: Option<&str>,
    at: &str,
) -> Result<bool> {
    let (path, from, to) = (key(path), i64::try_from(from)?, i64::try_from(to)?);
    let moved = tx.execute(
        "UPDATE ingested SET log_offset = ?3, outcome = ?4, reason = ?5, at = ?6
         WHERE path = ?1 AND outcome = ?7 AND log_offset = ?2",
        params![path, from, to, outcome, reason, at, store::ACCEPTED],
    )?;
    if moved == 1 {
        return Ok(true);
    }
    let new = tx.execute(
        "INSERT INTO ingested (path, sha256, log_offset, outcome, reason, at)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5)
         ON CONFLICT (path) DO NOTHING",
        params![path, to, outcome, reason, at],
    )?;
    Ok(new == 1)
}

/// The worker and the state of the attempt `a`.
fn attempt_row(tx: &Connection, a: &AttemptRef) -> Result<Option<(String, String)>> {
    Ok(tx
        .query_row(
            "SELECT worker_id, state FROM attempts WHERE id = ?1 AND task_id = ?2",
            [&a.attempt_id, &a.task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn event_kind(kind: LineKind) -> EventKind {
    match kind {
        LineKind::Progress => EventKind::Progress,
        LineKind::Note | LineKind::ContextWarned => EventKind::Note,
    }
}

fn record_warned(tx: &Connection, worker: &str, text: &str) -> Result<()> {
    let Some(record_id) = workers::find(tx, worker)?.and_then(|w| w.legacy_record_id) else {
        return Ok(());
    };
    record_history_in(tx, &record_id, EVENT_WARNED, text, &util::now())
}

/// The rows of `horch ledger progress` for 1 line: the progress of the
/// worker and its event, and the activity of a live attempt on the fleet
/// clock (`docs/tickets/fleetd-p2/MAP.md`, Q7). A line of an ended attempt
/// gets its event only.
fn record_line(
    tx: &Connection,
    a: &AttemptRef,
    worker: &str,
    live: bool,
    kind: LineKind,
    text: &str,
) -> Result<()> {
    if live {
        workers::progress(tx, worker, ProgressSource::Agent, event_kind(kind), text)?;
        leases_store::set_active_at(tx, &a.attempt_id, lease::fleet_clock(tx)?.0)?;
        return Ok(());
    }
    let run = tasks::get(tx, &a.task_id)?.run_id;
    let about = About {
        run: Some(&run),
        task: Some(&a.task_id),
        worker: Some(worker),
    };
    util::event(tx, &about, Actor::Worker, event_kind(kind), text)
}

/// The 1 event of a skipped line.
fn skipped(tx: &Connection, a: &AttemptRef, worker: &str, line: &Line, reason: &str) -> Result<()> {
    let run = tasks::get(tx, &a.task_id)?.run_id;
    let about = About {
        run: Some(&run),
        task: Some(&a.task_id),
        worker: Some(worker),
    };
    let text = format!(
        "progress.jsonl line {} of attempt {} is skipped: {reason}",
        line.number, a.attempt_id
    );
    util::event(tx, &about, Actor::Horch, EventKind::Note, &text)
}
