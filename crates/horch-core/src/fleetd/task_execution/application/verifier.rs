//! The verifier of an attempt, with its review and structural steps
//! (tickets 19, 20 and 21 of fleetd-p4) and the judge pair (ticket 41 of
//! fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "Architecture and roles" (Verifier; Done
//! for each kind of task; The judge pair), "The task state machine"
//! (`verifying`), RR-3, FM-9, FM-33 and "The kinds of job" (`task_check`).
//!
//! - Part 2 of a pass calls [`verify`]. For each task in `verifying` whose
//!   newest attempt is a push attempt, it queues 1 `task_check` job keyed
//!   by the tip of the task branch and the hash of the check, and never
//!   waits for it. A claim attempt is not touched: its old finish runs.
//! - A later pass reads the ended job. A verdict that did not fail gives a
//!   [`Verified`] for the review steps of a code kind or the structural
//!   check of a judgment kind or a judge task. A failed verdict ends the
//!   attempt with the outcome `rejected` and burns 1 attempt (rung 6).
//! - A verdict stands only for the tip that it judged (F5). New commits
//!   queue a new check job.
//! - Part 3 calls [`write_verdicts`] after the commit: each verdict is a
//!   row first, then its file `verdict-<n>.json`.
//! - [`done`] is the only move of a task from `verifying` to `done`, and it
//!   needs a [`Verified`] that still stands. A code kind also needs each of
//!   its review steps passed or not applicable. A judgment kind also needs
//!   its judge task `done` (`application::judge_pair`).
//! - Part 3 calls [`review`] after the commit: the review steps of each
//!   code kind whose check passed (`application::review`, ticket 20).
//! - Part 3 calls [`structural`] after the commit: the structural check of
//!   each judgment kind and judge task whose check passed
//!   (`domain::structural`). A judgment kind then waits in `verifying` for
//!   its judge task. The verdict file of a judge task goes to the judge
//!   pair (`application::judge_pair::settle`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::adapters::verdict_file;
use crate::fleetd::task_execution::application::judge_pair::{self, Judging, Settled};
use crate::fleetd::task_execution::application::retry::{self, Policy};
use crate::fleetd::task_execution::application::review;
use crate::fleetd::task_execution::domain::judge_pair::Verdict as JudgeVerdict;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::structural::{self, Checked, Output};
use crate::fleetd::task_execution::domain::task::{judged_task, AttemptState};
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
    /// The review tasks that their findings file moved to `done`.
    pub reviews_done: Vec<String>,
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
        let task = tasks::get(tx, &a.task_id)?;
        if task.is_review() {
            review_task(v, tx, &task, &a, &mut report)?;
            continue;
        }
        let Some(tip) = v.tips.get(&a.task_id) else {
            continue;
        };
        verify_one(v, tx, &a, tip, &mut report)?;
    }
    Ok(report)
}

/// A code-sweep review task of a push attempt has no check, no review steps
/// and no worktree: its findings file is its result (`finish::complete`
/// needs the same file). With the file, the task moves to `done` and its
/// attempt ends [`VERIFIED`]. Then the code-sweep of its parent task sees
/// 1 open review less. With no file, the attempt ends `rejected` and burns
/// 1 attempt.
fn review_task(
    v: &Verifier<'_>,
    tx: &Connection,
    task: &Task,
    a: &Attempt,
    report: &mut Report,
) -> Result<()> {
    let guard = v.jobs.guard;
    let findings = task.spec_ref.as_deref().unwrap_or_default();
    if findings.is_empty() || !Path::new(findings).is_file() {
        let why = format!("the review task wrote no findings file {findings}");
        end_with(tx, guard, a, Outcome::Rejected, &why)?;
        retry::burn_attempt(
            tx,
            &task.task_id,
            Outcome::Rejected,
            &(v.policy)(task),
            &why,
        )?;
        report.rejected.push(task.task_id.clone());
        return Ok(());
    }
    let text = format!("the verifier: the findings file {findings} is in");
    match transition(
        tx,
        guard,
        Entity::Task,
        &task.task_id,
        TaskStatus::Done.as_str(),
        cause(&text),
        &NeedsYouList,
    ) {
        Ok(_) => {
            end_as(tx, guard, a, VERIFIED, WorkerStatus::Done, &text)?;
            report.reviews_done.push(task.task_id.clone());
            Ok(())
        }
        Err(DoorError::Refused { .. }) => Ok(()),
        Err(e) => Err(e.into()),
    }
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
    let work_kind = work_kind_of(tx, &task)?;
    let kind = if judged_task(&task.kind, task.parent_task_id.as_deref()).is_some() {
        Some("judge")
    } else {
        work_kind.as_deref()
    };
    let facts = Facts {
        task_id: &task.task_id,
        attempt: &a.id,
        tip,
        base: base.as_deref(),
        check: check.as_deref(),
        check_hash: &hash,
        check_none: task.check_none,
        kind,
    };
    let next = next_step(work_kind.as_deref());
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
    let text = format!("the verifier rejected it: {why}");
    end_with(tx, guard, a, Outcome::Rejected, &text)?;
    Ok(why)
}

/// The attempt `a` ends with `outcome` for the reason `text`, and its
/// worker row ends.
pub(crate) fn end_with(
    tx: &Connection,
    guard: Option<Generation>,
    a: &Attempt,
    outcome: Outcome,
    text: &str,
) -> Result<()> {
    end_as(
        tx,
        guard,
        a,
        outcome.as_str(),
        WorkerStatus::Incomplete,
        text,
    )
}

/// The outcome of an attempt whose result the verifier passed
/// (`docs/specs/fleetd/design.md`, the `attempts` table). It burns no
/// attempt, and it is not an outcome of rung 6.
pub const VERIFIED: &str = "verified";

/// The attempt `a` ends with the outcome word `outcome` for the reason
/// `text`, and its worker row ends with `status`. An attempt that ended
/// first, at the exit of its process, gets `outcome` in place of the
/// outcome of that exit.
fn end_as(
    tx: &Connection,
    guard: Option<Generation>,
    a: &Attempt,
    outcome: &str,
    status: WorkerStatus,
    text: &str,
) -> Result<()> {
    attempts_store::set_ended(tx, &a.id, outcome, &util::now())?;
    if a.state != AttemptState::Ended.as_str() {
        transition(
            tx,
            guard,
            Entity::Attempt,
            &a.id,
            AttemptState::Ended.as_str(),
            cause(text),
            &NeedsYouList,
        )?;
    }
    let reason = format!("{outcome}: attempt {}", a.id);
    workers::end(tx, &a.worker_id, status, &reason, None)?;
    Ok(())
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The step after the check of `task`, by its kind: the ticket kind of
/// `work_kind`, else the kind of its plan in `spec_json`. The ledger's
/// `work_kind` holds only the kinds of today, so a planned `judge`,
/// `research`, `design` or `creative` task has its kind in `spec_json`
/// only.
fn next_of(conn: &Connection, task: &Task) -> Result<Next> {
    Ok(next_step(work_kind_of(conn, task)?.as_deref()))
}

fn work_kind_of(conn: &Connection, task: &Task) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT COALESCE(work_kind, json_extract(spec_json, '$.kind')) FROM tasks
         WHERE task_id = ?1",
        [&task.task_id],
        |r| r.get(0),
    )?)
}

/// Whether `verified` still stands for `task` in `verifying`: its verdict
/// is the newest of the task, its attempt is the newest attempt, and `tip`
/// is the tip that the verdict judged.
fn stands(tx: &Connection, task: &Task, verified: &Verified, tip: &str) -> Result<bool> {
    if task.status != TaskStatus::Verifying.as_str() {
        return Ok(false);
    }
    let Some(newest) = verdict_file::newest(tx, &task.task_id)? else {
        return Ok(false);
    };
    let attempt: Option<String> = tx.query_row(
        "SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
        [&task.task_id],
        |r| r.get(0),
    )?;
    if !verified.stands(&newest, tip) || attempt.as_deref() != Some(verified.attempt()) {
        return Ok(false);
    }
    // A judgment kind and a judge task stand only on a passed structural
    // check (ticket 21).
    Ok(next_of(tx, task)? != Next::Structural || structural::is_pass(&newest))
}

/// Whether the newest verdict of `task_id` is a passed structural check of
/// its newest attempt: the `judges:` dependency of its judge task waits for
/// it (I2; ticket 40 of fleetd-p6).
pub fn structural_passed(conn: &Connection, task_id: &str) -> Result<bool> {
    let Some(newest) = verdict_file::newest(conn, task_id)? else {
        return Ok(false);
    };
    let attempt: Option<String> = conn
        .query_row(
            "SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [task_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(structural::is_pass(&newest) && attempt.as_deref() == Some(newest.attempt.as_str()))
}

/// The passed structural check of `task_id` that stands for `tip`, from
/// its newest verdict, in `tx`. `None` when the task is not in
/// `verifying`, or its newest verdict is not a passed structural check of
/// its newest attempt at `tip`.
pub(crate) fn standing(tx: &Connection, task_id: &str, tip: &str) -> Result<Option<Verified>> {
    let task = tasks::get(tx, task_id)?;
    let Some(verified) = verdict_file::newest(tx, task_id)?.and_then(|v| v.verified()) else {
        return Ok(None);
    };
    let structural = next_of(tx, &task)? == Next::Structural;
    Ok((structural && stands(tx, &task, &verified, tip)?).then_some(verified))
}

/// Move the task of `verified` from `verifying` to `done`, in the open
/// transaction `tx`, when `verified` still stands: its verdict is the
/// newest of the task, its attempt is the newest attempt, and `tip` is the
/// tip that the verdict judged. Its attempt ends with the outcome
/// [`VERIFIED`] in the same transaction. The fleetd of generation `guard`
/// writes. `false` when it moved nothing.
pub fn done(
    tx: &Connection,
    guard: Option<Generation>,
    verified: Verified,
    tip: &str,
) -> Result<bool> {
    let task = tasks::get(tx, verified.task_id())?;
    if !stands(tx, &task, &verified, tip)? {
        return Ok(false);
    }
    let through = match next_of(tx, &task)? {
        // A code kind moves to done only after its review steps (ticket
        // 20).
        Next::Review => review::steps_through(tx, &task, verified.attempt())?,
        // A judge task moves to done when the judge pair settles its
        // verdict with no open CRITICAL item.
        Next::Structural if judged_task(&task.kind, task.parent_task_id.as_deref()).is_some() => {
            true
        }
        // A judgment kind moves to done only after its judge task (The
        // judge pair, step 4).
        Next::Structural => judge_pair::judge_done(tx, &task.task_id)?,
    };
    if !through {
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
        Ok(_) => {
            let a = attempt(tx, verified.attempt())?;
            end_as(tx, guard, &a, VERIFIED, WorkerStatus::Done, &text)?;
            Ok(true)
        }
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
    /// The verified value no longer stands, or the task left `verifying`:
    /// nothing moved.
    Stale,
    /// The check of a judgment kind passed: the task waits in `verifying`
    /// for its judge task (The judge pair, step 1).
    WaitsForJudge,
    /// The check of the verdict file of a judge task passed: what the judge
    /// pair did.
    Judged(Settled),
}

/// Part 3 of a pass, after the commit of part 2: the structural check of
/// each task of `passed` whose next step is [`Next::Structural`], in
/// `store` with no transaction open. A check that fails records a failed
/// verdict, and the attempt ends `rejected` and burns 1 attempt. A check
/// that passes records a passed verdict. Then a judgment kind waits for its
/// judge task, and the verdict file of a judge task goes to the judge pair
/// in the same transaction. Gives what each check did, by task.
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
        return Ok(Structural::Stale);
    };
    if task.status != TaskStatus::Verifying.as_str() {
        return Ok(Structural::Stale);
    }
    let judge = judged_task(&task.kind, task.parent_task_id.as_deref()).is_some();
    // The check of a judgment kind passed in an earlier pass: it waits.
    if !judge && structural::is_pass(&newest) {
        return Ok(Structural::WaitsForJudge);
    }
    let (checked, verdict) = match outputs_of(r.tools, store.conn(), &task, verified.tip())? {
        None => (Checked::unreadable(NO_BASE), None),
        Some(outputs) if judge => judge_check(&outputs),
        Some(outputs) => (structural::check(&outputs), None),
    };
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
    let Some(verdict) = verdict else {
        if !structural::is_pass(&newest) {
            store.immediate(|tx| {
                verdict_file::record(tx, guard, of(Decision::Passed)).map(|_| ())
            })?;
        }
        return Ok(Structural::WaitsForJudge);
    };
    let held = held_tip(r.tools, store.conn(), &task, verified.tip())?;
    let settled = store.immediate(|tx| {
        let judge = if structural::is_pass(&newest) {
            verified.clone()
        } else {
            verdict_file::record(tx, guard, of(Decision::Passed))?
                .verified()
                .context("a passed structural verdict gives its verified value")?
        };
        let j = Judging {
            guard,
            clock: r.clock,
            judge: &judge,
            verdict: &verdict,
            held: held.as_deref(),
        };
        judge_pair::settle(tx, &j, &task)
    })?;
    Ok(Structural::Judged(settled))
}

/// The structural check of the outputs of a judge task, and its verdict
/// when the check passed. A check that finds no verdict file fails.
fn judge_check(outputs: &[Output]) -> (Checked, Option<JudgeVerdict>) {
    let mut checked = structural::check_judge(outputs);
    if !checked.passed() {
        return (checked, None);
    }
    let verdict = judge_pair::verdict_of(outputs);
    if verdict.is_none() {
        checked
            .failures
            .push("no output is a verdict file (`.md`)".to_string());
    }
    (checked, verdict)
}

/// The tip of the judged branch of the judge task `judge` that the judge
/// branch holds at `tip`: the tip that the newest verdict of the judged
/// task judged, when it is `tip` or an ancestor of `tip`. `None` when the
/// judge branch does not hold it, so the verdict judged another result.
fn held_tip(tools: &Tools, conn: &Connection, judge: &Task, tip: &str) -> Result<Option<String>> {
    let Some(judged) = judged_task(&judge.kind, judge.parent_task_id.as_deref()) else {
        return Ok(None);
    };
    let Some(judged_tip) = verdict_file::newest(conn, judged)?.map(|v| v.tip) else {
        return Ok(None);
    };
    let Some(dir) = finish::worktree_of(conn, &judge.task_id)?.map(|w| PathBuf::from(w.path))
    else {
        return Ok(None);
    };
    let holds = tools
        .git
        .try_run(&dir, &["merge-base", "--is-ancestor", &judged_tip, tip])?
        .ok;
    Ok(holds.then_some(judged_tip))
}

/// Why a task with no worktree and base commit fails its structural check.
const NO_BASE: &str = "the task has no worktree with a base commit, so its commits are not known";

/// The outputs of `task` at `tip`: the files of its `Own` that its commits
/// since the base of its worktree changed, with their text at `tip`.
/// `None` when the task has no worktree with a base commit.
fn outputs_of(
    tools: &Tools,
    conn: &Connection,
    task: &Task,
    tip: &str,
) -> Result<Option<Vec<Output>>> {
    let worktree = finish::worktree_of(conn, &task.task_id)?;
    let Some((dir, base)) = worktree.and_then(|w| Some((PathBuf::from(w.path), w.base_commit?)))
    else {
        return Ok(None);
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
    Ok(Some(outputs))
}

/// Part 3 of a pass, after the commit: write the file of each committed
/// verdict that has none. Gives the files.
pub fn write_verdicts(conn: &mut Connection, fleet_root: &Path) -> Result<Vec<PathBuf>> {
    verdict_file::write_unwritten(conn, fleet_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::taskledger::model::{TaskKind, WorkerKind, WorktreeMode};
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::schema;
    use crate::taskledger::tasks::NewTask;
    use crate::taskledger::workers::NewWorker;
    use rusqlite::{params, TransactionBehavior};

    struct World {
        dir: tempfile::TempDir,
        conn: Connection,
        run: Run,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let project = dir.path().join("repo");
        std::fs::create_dir_all(&project).unwrap();
        let run = runs::create(
            &conn,
            &project,
            &NewRun {
                slug: "r1".into(),
                ..Default::default()
            },
        )
        .unwrap();
        schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
        World { dir, conn, run }
    }

    impl World {
        /// A code-sweep review task of a parent task, in `verifying` after
        /// its push attempt wrote `result.json`. Gives the task and its
        /// attempt.
        fn review(&self, findings: &Path) -> (String, String) {
            let parent = tasks::add(
                &self.conn,
                &self.run,
                &NewTask::work("p", "p.md", "opus-dev"),
                Actor::Orchestrator,
            )
            .unwrap()
            .task_id;
            let mut new = NewTask::work("Review p", "review.md", "opus-reviewer");
            new.kind = TaskKind::Review;
            new.parent_task_id = Some(parent);
            new.worktree_mode = WorktreeMode::Shared;
            new.spec_ref = Some(findings.to_string_lossy().into_owned());
            let task = tasks::add(&self.conn, &self.run, &new, Actor::Horch)
                .unwrap()
                .task_id;
            let worker = workers::insert(
                &self.conn,
                &NewWorker {
                    run_id: self.run.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: "opus-reviewer".into(),
                    target_task_id: Some(task.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            let attempt = format!("{task}-a1");
            self.conn
                .execute(
                    "UPDATE tasks SET status = 'verifying', attempts = 1, current_worker_id = ?2
                     WHERE task_id = ?1",
                    params![task, worker],
                )
                .unwrap();
            self.conn
                .execute(
                    "INSERT INTO attempts (id, task_id, worker_id, state, started_at, via)
                     VALUES (?1, ?2, ?3, 'live', ?4, 'push')",
                    params![attempt, task, worker, util::now()],
                )
                .unwrap();
            (task, attempt)
        }

        /// 1 verifier part of a pass with a tip for each task in
        /// `verifying`, as the run branch gives a review task its tip.
        fn verify(&mut self, tasks: &[&str]) -> Report {
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let tips: BTreeMap<String, String> = tasks
                .iter()
                .map(|t| (t.to_string(), "1".repeat(40)))
                .collect();
            let policy = |_: &Task| Policy::default();
            let v = Verifier {
                jobs: &ctx,
                clock: FleetSeconds(100),
                tips: &tips,
                policy: &policy,
            };
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let report = verify(&v, &tx).unwrap();
            tx.commit().unwrap();
            report
        }

        fn attempt_row(&self, id: &str) -> (String, Option<String>) {
            self.conn
                .query_row(
                    "SELECT state, outcome FROM attempts WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }

        fn check_jobs(&self) -> i64 {
            self.conn
                .query_row(
                    "SELECT count(*) FROM jobs WHERE kind = 'task_check'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        }
    }

    #[test]
    fn a_push_review_task_with_its_findings_file_ends_done_with_no_check_job() {
        let mut w = world();
        let findings = w.dir.path().join("findings.md");
        std::fs::write(&findings, "No findings.\n").unwrap();
        let (task, attempt) = w.review(&findings);
        let parent = tasks::get(&w.conn, &task).unwrap().parent_task_id.unwrap();
        assert_eq!(finish::open_reviews(&w.conn, &parent).unwrap(), 1);

        let report = w.verify(&[&task]);

        assert_eq!(report.reviews_done, vec![task.clone()]);
        assert!(report.queued.is_empty() && report.passed.is_empty());
        assert_eq!(w.check_jobs(), 0, "a review task gets no task_check job");
        assert_eq!(
            tasks::get(&w.conn, &task).unwrap().status,
            TaskStatus::Done.as_str()
        );
        assert_eq!(
            w.attempt_row(&attempt),
            ("ended".into(), Some(VERIFIED.into()))
        );
        assert_eq!(finish::open_reviews(&w.conn, &parent).unwrap(), 0);
    }

    #[test]
    fn a_push_review_task_with_no_findings_file_is_rejected() {
        let mut w = world();
        let findings = w.dir.path().join("missing.md");
        let (task, attempt) = w.review(&findings);

        let report = w.verify(&[&task]);

        assert_eq!(report.rejected, vec![task.clone()]);
        assert_eq!(w.check_jobs(), 0);
        assert_ne!(
            tasks::get(&w.conn, &task).unwrap().status,
            TaskStatus::Done.as_str()
        );
        assert_eq!(
            w.attempt_row(&attempt),
            ("ended".into(), Some(Outcome::Rejected.as_str().into()))
        );
    }
}
