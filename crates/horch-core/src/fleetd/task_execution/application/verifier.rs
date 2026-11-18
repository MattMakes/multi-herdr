//! The verifier of an attempt, with its review and structural steps (tickets 19, 20 and 21).
//!
//! `docs/specs/fleetd/design.md`, "Architecture and roles" (Verifier; Done
//! for each kind of task), "The task state machine" (`verifying`), RR-3
//! (the Phase 4 part), FM-9 (the part test) and "The kinds of job"
//! (`task_check`).
//!
//! - Part 2 of a pass calls [`verify`]. For each task in `verifying` whose
//!   newest attempt is a push attempt, it queues 1 `task_check` job keyed
//!   by the tip of the task branch and the hash of the check, and never
//!   waits for it. A claim attempt is not touched: its old finish runs.
//! - A later pass reads the ended job. A verdict that did not fail gives a
//!   [`Verified`] for the review steps of a code kind or the structural
//!   check of a judgment kind. A failed verdict ends the attempt with the
//!   outcome `rejected` and burns 1 attempt (rung 6).
//! - A verdict stands only for the tip that it judged (F5). New commits
//!   queue a new check job.
//! - Part 3 calls [`write_verdicts`] after the commit: each verdict is a
//!   row first, then its file `verdict-<n>.json`.
//! - [`done`] is the only move of a task from `verifying` to `done`, and it
//!   needs a [`Verified`] that still stands. A code kind also needs each of
//!   its review steps passed or not applicable.
//! - Part 3 calls [`review`] after the commit: the review steps of each
//!   code kind whose check passed (`application::review`, ticket 20).
//! - Part 3 calls [`structural`] after the commit: the structural check of
//!   each judgment kind whose check passed (`domain::structural`, ticket
//!   21), then today's finish steps. With no judge before Phase 6, a
//!   judgment kind moves to `done` after them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::adapters::verdict_file;
use crate::fleetd::task_execution::application::retry::{self, Policy};
use crate::fleetd::task_execution::application::review;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::structural::{self, Checked, Output};
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fleetd::task_execution::domain::verification::{
    self, next_step, Decision, Facts, Next, Plan, Verdict, Verified,
};
use crate::taskledger::finish::Tools;
use crate::taskledger::model::{Actor, FileMode, TaskStatus, WorkerStatus};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::{finish, util, workers};

/// What [`verify`] reads.
pub struct Verifier<'a> {
    pub jobs: &'a JobCtx<'a>,
    /// The fleet clock of the pass.
    pub clock: FleetSeconds,
    /// The tip of the task branch of each task, read in part 1 of the pass.
    /// A task with no tip waits for a later pass.
    pub tips: &'a BTreeMap<String, String>,
    /// The numbers of rung 6 of a task.
    pub policy: &'a dyn Fn(&Task) -> Policy,
}

/// What 1 [`verify`] did, by task id.
#[derive(Debug, Default)]
pub struct Report {
    /// The tasks whose check job is queued or running.
    pub queued: Vec<String>,
    /// The tasks whose check job is held at `abandoned_run_limit`.
    pub held: Vec<String>,
    /// The tasks whose attempt ended `rejected`.
    pub rejected: Vec<String>,
    /// The tasks whose verdict did not fail, with their next step.
    pub passed: Vec<Passed>,
}

/// A task whose verdict did not fail.
#[derive(Debug)]
pub struct Passed {
    pub verified: Verified,
    pub next: Next,
}

/// The hash of the check command `check`: sha256, in hex. No command gives
/// `none`.
pub fn check_hash(check: Option<&str>) -> String {
    let Some(check) = check else {
        return "none".into();
    };
    Sha256::digest(check.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The newest attempt of a task in `verifying`.
pub(crate) struct Attempt {
    task_id: String,
    id: String,
    pub(crate) worker_id: String,
    state: String,
    via: String,
}

/// The attempt `id`.
pub(crate) fn attempt(conn: &Connection, id: &str) -> Result<Attempt> {
    conn.query_row(
        "SELECT task_id, id, worker_id, state, via FROM attempts WHERE id = ?1",
        [id],
        |r| {
            Ok(Attempt {
                task_id: r.get(0)?,
                id: r.get(1)?,
                worker_id: r.get(2)?,
                state: r.get(3)?,
                via: r.get(4)?,
            })
        },
    )
    .with_context(|| format!("no attempt {id}"))
}

/// The verifier part of a pass, in the open transaction `tx`.
pub fn verify(v: &Verifier<'_>, tx: &Connection) -> Result<Report> {
    if tx.is_autocommit() {
        bail!("the verifier writes its rows in 1 open transaction of the ledger");
    }
    let mut report = Report::default();
    for a in newest_attempts(tx)? {
        if a.via != "push" || tasks::cancel_requested(tx, &a.task_id)?.is_some() {
            continue;
        }
        let Some(tip) = v.tips.get(&a.task_id) else {
            continue;
        };
        verify_one(v, tx, &a, tip, &mut report)?;
    }
    Ok(report)
}

/// The newest attempt of each task in `verifying`.
fn newest_attempts(tx: &Connection) -> Result<Vec<Attempt>> {
    let mut stmt = tx.prepare(
        "SELECT t.task_id, a.id, a.worker_id, a.state, a.via
         FROM tasks t JOIN attempts a ON a.task_id = t.task_id
         WHERE t.status = ?1
           AND a.rowid = (SELECT max(rowid) FROM attempts WHERE task_id = t.task_id)
         ORDER BY t.task_id",
    )?;
    let rows = stmt
        .query_map([TaskStatus::Verifying.as_str()], |r| {
            Ok(Attempt {
                task_id: r.get(0)?,
                id: r.get(1)?,
                worker_id: r.get(2)?,
                state: r.get(3)?,
                via: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn verify_one(
    v: &Verifier<'_>,
    tx: &Connection,
    a: &Attempt,
    tip: &str,
    report: &mut Report,
) -> Result<()> {
    let task = tasks::get(tx, &a.task_id)?;
    // The check of today's finish: the task's own, else the run's default.
    let check = task
        .land_check
        .clone()
        .or(finish::default_check(tx, &task.run_id)?)
        .filter(|c| !c.trim().is_empty());
    let base = if task.uses_worktree() {
        finish::worktree_of(tx, &task.task_id)?.and_then(|w| w.base_commit)
    } else {
        None
    };
    let hash = check_hash(check.as_deref());
    let facts = Facts {
        task_id: &task.task_id,
        attempt: &a.id,
        tip,
        base: base.as_deref(),
        check: check.as_deref(),
        check_hash: &hash,
        check_none: task.check_none,
    };
    let next = next_step(task.work_kind.as_deref());
    // The verdict of this attempt, tip and check stands: decide no more.
    if let Some(last) = verdict_file::newest(tx, &task.task_id)? {
        if last.judged(&facts) {
            if let Some(verified) = last.verified() {
                report.passed.push(Passed { verified, next });
            }
            return Ok(());
        }
    }
    let verdict = match verification::plan(&facts) {
        Plan::Decided(verdict) => verdict,
        Plan::Check { input_key } => {
            let key = JobKey {
                kind: Kind::TaskCheck,
                subject: &task.task_id,
                input_key: &input_key,
            };
            match jobs::queue(v.jobs, tx, &key, None, v.clock)? {
                Queued::New(_) | Queued::Active(_) => {
                    report.queued.push(task.task_id.clone());
                    return Ok(());
                }
                Queued::Held(_) => {
                    report.held.push(task.task_id.clone());
                    return Ok(());
                }
                Queued::Reused(row) => {
                    let o = jobs::outcome(&row)
                        .with_context(|| format!("the task_check run {} has no result", row.id))?;
                    verification::from_check(&facts, row.id, o.passed, &o.text)
                }
            }
        }
    };
    let verdict = verdict_file::record(tx, v.jobs.guard, verdict)?;
    match verdict.verified() {
        Some(verified) => report.passed.push(Passed { verified, next }),
        None => {
            reject(tx, v.jobs.guard, v.policy, &task, a, &verdict)?;
            report.rejected.push(task.task_id.clone());
        }
    }
    Ok(())
}

/// A failed verdict: the attempt `a` ends with the outcome `rejected`, its
/// worker row ends, and rung 6 burns 1 attempt of `task`.
pub(crate) fn reject(
    tx: &Connection,
    guard: Option<Generation>,
    policy: &dyn Fn(&Task) -> Policy,
    task: &Task,
    a: &Attempt,
    verdict: &Verdict,
) -> Result<()> {
    let why = end_attempt(tx, guard, a, verdict)?;
    retry::burn_attempt(tx, &task.task_id, Outcome::Rejected, &policy(task), &why)?;
    Ok(())
}

/// The attempt `a` ends with the outcome `rejected` for the failed
/// `verdict`, and its worker row ends. Gives the reason.
pub(crate) fn end_attempt(
    tx: &Connection,
    guard: Option<Generation>,
    a: &Attempt,
    verdict: &Verdict,
) -> Result<String> {
    let why = format!("{}: {}", verdict_file::file_name(verdict.n), verdict.reason);
    let outcome = Outcome::Rejected.as_str();
    attempts_store::set_ended(tx, &a.id, outcome, &util::now())?;
    if a.state != AttemptState::Ended.as_str() {
        let text = format!("the verifier rejected it: {why}");
        transition(
            tx,
            guard,
            Entity::Attempt,
            &a.id,
            AttemptState::Ended.as_str(),
            cause(&text),
            &NeedsYouList,
        )?;
    }
    let reason = format!("{outcome}: attempt {}", a.id);
    workers::end(tx, &a.worker_id, WorkerStatus::Incomplete, &reason, None)?;
    Ok(why)
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Move the task of `verified` from `verifying` to `done`, in the open
/// transaction `tx`, when `verified` still stands: its verdict is the
/// newest of the task, its attempt is the newest attempt, and `tip` is the
/// tip that the verdict judged. The fleetd of generation `guard` writes.
/// `false` when it moved nothing.
pub fn done(
    tx: &Connection,
    guard: Option<Generation>,
    verified: Verified,
    tip: &str,
) -> Result<bool> {
    let task = tasks::get(tx, verified.task_id())?;
    if task.status != TaskStatus::Verifying.as_str() {
        return Ok(false);
    }
    let Some(newest) = verdict_file::newest(tx, verified.task_id())? else {
        return Ok(false);
    };
    let attempt: Option<String> = tx.query_row(
        "SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
        [verified.task_id()],
        |r| r.get(0),
    )?;
    if !verified.stands(&newest, tip) || attempt.as_deref() != Some(verified.attempt()) {
        return Ok(false);
    }
    // A judgment kind moves to done only after its structural check
    // passed (ticket 21).
    if next_step(task.work_kind.as_deref()) == Next::Structural && !structural::is_pass(&newest) {
        return Ok(false);
    }
    // Each kind moves to done only after its review steps: the review
    // steps of a code kind (ticket 20), today's finish steps of a judgment
    // kind until the judge pair of Phase 6 (ticket 21).
    if !review::steps_through(tx, &task, verified.attempt())? {
        return Ok(false);
    }
    let text = format!(
        "the verifier: {} judged {tip}",
        verdict_file::file_name(verified.n())
    );
    match transition(
        tx,
        guard,
        Entity::Task,
        verified.task_id(),
        TaskStatus::Done.as_str(),
        cause(&text),
        &NeedsYouList,
    ) {
        Ok(_) => Ok(true),
        Err(DoorError::Refused { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Part 3 of a pass, after the commit of part 2 (ticket 20): the review
/// steps of each task of `passed` whose next step is [`Next::Review`], in
/// `store` with no transaction open. The structural check of a judgment
/// kind runs in [`structural`] (ticket 21). Gives what each review did, by
/// task.
pub fn review(
    r: &review::Review<'_>,
    store: &mut Store,
    passed: Vec<Passed>,
) -> Result<Vec<(String, review::Reviewed)>> {
    let mut out = Vec::new();
    for p in passed.into_iter().filter(|p| p.next == Next::Review) {
        let task_id = p.verified.task_id().to_string();
        out.push((task_id, review::run(r, store, p.verified)?));
    }
    Ok(out)
}

/// What the structural check of 1 task did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Structural {
    /// The check failed: the attempt ended `rejected` and burned 1
    /// attempt. The reason names each failure.
    Rejected(String),
    /// The check passed, then today's finish steps did this.
    Reviewed(review::Reviewed),
}

/// Part 3 of a pass, after the commit of part 2 (ticket 21): the
/// structural check of each task of `passed` whose next step is
/// [`Next::Structural`], in `store` with no transaction open. A check that
/// passes records a passed verdict, then today's finish steps run
/// (`application::review`), and the task moves to `done` after them. With
/// no judge before Phase 6, that is its whole path. A check that fails
/// records a failed verdict, and the attempt ends `rejected` and burns 1
/// attempt. Gives what each check did, by task.
pub fn structural(
    r: &review::Review<'_>,
    store: &mut Store,
    passed: &[Passed],
) -> Result<Vec<(String, Structural)>> {
    let mut out = Vec::new();
    for p in passed.iter().filter(|p| p.next == Next::Structural) {
        let task_id = p.verified.task_id().to_string();
        out.push((task_id, structural_one(r, store, p.verified.clone())?));
    }
    Ok(out)
}

fn structural_one(
    r: &review::Review<'_>,
    store: &mut Store,
    verified: Verified,
) -> Result<Structural> {
    let task = tasks::get(store.conn(), verified.task_id())?;
    let newest = verdict_file::newest(store.conn(), &task.task_id)?;
    let Some(newest) = newest.filter(|n| verified.stands(n, verified.tip())) else {
        return Ok(Structural::Reviewed(review::Reviewed::Stale));
    };
    if task.status != TaskStatus::Verifying.as_str() {
        return Ok(Structural::Reviewed(review::Reviewed::Stale));
    }
    // The check of this verdict passed in an earlier pass: today's finish
    // steps go on.
    if structural::is_pass(&newest) {
        return Ok(Structural::Reviewed(review::run(r, store, verified)?));
    }
    let checked = check_outputs(r.tools, store.conn(), &task, verified.tip())?;
    let reason = checked.reason();
    let guard = r.jobs.guard;
    let of = |decision| Verdict {
        n: 0,
        decision,
        job: None,
        reason: reason.clone(),
        ..newest.clone()
    };
    if !checked.passed() {
        store.immediate(|tx| {
            let verdict = verdict_file::record(tx, guard, of(Decision::Failed))?;
            let a = attempt(tx, verified.attempt())?;
            reject(tx, guard, r.policy, &task, &a, &verdict)
        })?;
        return Ok(Structural::Rejected(reason));
    }
    let verdict = store.immediate(|tx| verdict_file::record(tx, guard, of(Decision::Passed)))?;
    let verified = verdict
        .verified()
        .context("a passed structural verdict gives its verified value")?;
    Ok(Structural::Reviewed(review::run(r, store, verified)?))
}

/// The structural check of the outputs of `task` at `tip`: the files of
/// its `Own` that its commits since the base of its worktree changed, with
/// their text at `tip`.
fn check_outputs(tools: &Tools, conn: &Connection, task: &Task, tip: &str) -> Result<Checked> {
    let worktree = finish::worktree_of(conn, &task.task_id)?;
    let Some((dir, base)) = worktree.and_then(|w| Some((PathBuf::from(w.path), w.base_commit?)))
    else {
        return Ok(Checked::unreadable(
            "the task has no worktree with a base commit, so its commits are not known",
        ));
    };
    let range = format!("{base}..{tip}");
    let log = tools.git.run(
        &dir,
        &["log", "--format=", "--name-only", "--no-renames", &range],
    )?;
    let changed: Vec<String> = log
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let own = tasks::files(conn, &task.task_id, FileMode::Own)?;
    let mut outputs = Vec::new();
    for path in structural::outputs_of(&own, &changed) {
        let shown = tools
            .git
            .try_run(&dir, &["show", &format!("{tip}:{path}")])?;
        outputs.push(Output {
            text: shown.ok.then_some(shown.stdout),
            path,
        });
    }
    Ok(structural::check(&outputs))
}

/// Part 3 of a pass, after the commit: write the file of each committed
/// verdict that has none. Gives the files.
pub fn write_verdicts(conn: &mut Connection, fleet_root: &Path) -> Result<Vec<PathBuf>> {
    verdict_file::write_unwritten(conn, fleet_root)
}
