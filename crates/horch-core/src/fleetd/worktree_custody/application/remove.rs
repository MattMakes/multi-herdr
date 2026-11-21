use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::sqlite::{facts_after, facts_of};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{
    record, transition, Cause, DoorError, Moved, RecordRefusal,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{backoff_s, held, reusable, Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::remove_git::{read_remove_key, remove_key, Removed};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{self, RunJob};
use crate::fleetd::worktree_custody::domain::removal;
use crate::taskledger::model::{Actor, EventKind, WorktreeStatus};
use crate::taskledger::util;

pub fn queue_remove(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: &WorktreeRow,
    cause: &str,
    keep_branch: bool,
    clock: FleetSeconds,
) -> Result<Result<Queued, String>> {
    let id = row.worktree_id.as_str();
    let Some(now) = store::get(tx, id)? else {
        return Ok(Err(format!("worktree {id} is not in the ledger ({cause})")));
    };
    if let Err(why) = removal::removable(id, now.state.as_str()) {
        return Ok(Err(format!("{why} ({cause})")));
    }
    let input_key = remove_key(&now.path, keep_branch);
    let key = JobKey {
        kind: Kind::WorktreeRemove,
        subject: id,
        input_key: &input_key,
    };
    match jobs::queue(ctx, tx, &key, Some(&now.repo_root), clock)? {
        Queued::Active(open) if open.input_key != input_key => Ok(Err(format!(
            "worktree {id} has the worktree_remove job {} in {} with another input; \
             run the command again after it ends ({cause})",
            open.id,
            open.state.as_str()
        ))),
        q => Ok(Ok(q)),
    }
}

#[derive(Debug, Clone, Default)]
pub struct TreeJobs {
    runs: BTreeMap<(&'static str, String), Vec<JobRow>>,
}

impl TreeJobs {
    pub fn read<'a>(
        conn: &Connection,
        kinds: &[Kind],
        subjects: impl IntoIterator<Item = &'a str>,
    ) -> Result<TreeJobs> {
        let mut runs: BTreeMap<_, Vec<JobRow>> = BTreeMap::new();
        for run in jobs_store::of_subjects(conn, kinds, subjects)? {
            runs.entry((run.kind.as_str(), run.subject.clone()))
                .or_default()
                .push(run);
        }
        Ok(TreeJobs { runs })
    }

    fn of(&self, kind: Kind, subject: &str) -> &[JobRow] {
        self.runs
            .get(&(kind.as_str(), subject.to_string()))
            .map_or(&[], Vec::as_slice)
    }

    pub fn latest(&self, kind: Kind, subject: &str, input_key: &str) -> Option<&JobRow> {
        self.of(kind, subject)
            .iter()
            .filter(|j| j.input_key == input_key)
            .max_by_key(|j| j.run)
    }

    pub fn last(&self, kind: Kind, subject: &str) -> Option<&JobRow> {
        self.of(kind, subject).iter().max_by_key(|j| j.id)
    }

    pub fn active(&self, kind: Kind, subject: &str) -> Option<&JobRow> {
        self.of(kind, subject)
            .iter()
            .find(|j| matches!(j.state, State::Queued | State::Running))
    }

    pub fn get(&self, kind: Kind, subject: &str, id: i64) -> Option<&JobRow> {
        self.of(kind, subject).iter().find(|j| j.id == id)
    }

    pub fn latest_removal(&self, id: &str, path: &str) -> Option<&JobRow> {
        [false, true]
            .into_iter()
            .filter_map(|keep| self.latest(Kind::WorktreeRemove, id, &remove_key(path, keep)))
            .max_by_key(|j| j.id)
    }

    pub fn queues(&self, ctx: &JobCtx<'_>, key: &JobKey<'_>) -> bool {
        if self.active(key.kind, key.subject).is_some() {
            return false;
        }
        match self.latest(key.kind, key.subject, key.input_key) {
            Some(r) if reusable(r.kind, r.state, outcome(r).is_some_and(|o| o.verdict)) => false,
            Some(r) if r.state == State::Abandoned => {
                !held(r.abandoned_runs, ctx.config.abandoned_run_limit)
            }
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Notes {
    refusals: BTreeMap<String, Vec<Refusal>>,
    pub ended: BTreeMap<String, OwnerEnded>,
    causes: BTreeSet<(String, String)>,
}

impl Notes {
    pub fn read(tx: &Connection) -> Result<Notes> {
        let mut notes = Notes::default();
        for f in facts_after(tx, Entity::Worktree, EventKind::Note, 0)? {
            if f.cause == REFUSED {
                if let Ok(r) = serde_json::from_str(&f.data_json) {
                    notes
                        .refusals
                        .entry(f.entity_id.clone())
                        .or_default()
                        .push(r);
                }
            } else if f.cause == OWNER_ENDED {
                if let Ok(ended) = serde_json::from_str(&f.data_json) {
                    notes.ended.insert(f.entity_id.clone(), ended);
                }
            }
            notes.causes.insert((f.entity_id, f.cause));
        }
        Ok(notes)
    }

    pub fn has(&self, id: &str, cause: &str) -> bool {
        self.causes.contains(&(id.to_string(), cause.to_string()))
    }

    fn refusals(&self, id: &str) -> &[Refusal] {
        self.refusals.get(id).map_or(&[], Vec::as_slice)
    }
}

pub fn queue_auto_remove(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    jobs: &TreeJobs,
    row: &WorktreeRow,
    cause: &str,
    clock: FleetSeconds,
) -> Result<bool> {
    auto_remove(ctx, tx, jobs, row, cause, false, clock)
}

pub fn auto_remove(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    jobs: &TreeJobs,
    row: &WorktreeRow,
    cause: &str,
    keep_branch: bool,
    clock: FleetSeconds,
) -> Result<bool> {
    let last = jobs.latest_removal(&row.worktree_id, &row.path);
    if last.is_some_and(|j| j.state == State::Failed) || busy(jobs, row) {
        return Ok(false);
    }
    let input_key = remove_key(&row.path, keep_branch);
    let key = JobKey {
        kind: Kind::WorktreeRemove,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    if !jobs.queues(ctx, &key) {
        return Ok(false);
    }
    let queued = queue_remove(ctx, tx, row, cause, keep_branch, clock)?;
    Ok(matches!(queued, Ok(Queued::New(_))))
}

fn busy(jobs: &TreeJobs, row: &WorktreeRow) -> bool {
    let rescue = jobs.active(Kind::Rescue, &row.worktree_id);
    let list = jobs.active(Kind::WorktreeList, &row.repo_root);
    rescue.is_some() || list.is_some_and(|j| j.state == State::Running)
}

pub fn retry(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    jobs: &TreeJobs,
    notes: &Notes,
    row: &WorktreeRow,
    clock: FleetSeconds,
) -> Result<bool> {
    let id = row.worktree_id.as_str();
    let Some(last) = jobs.latest_removal(id, &row.path) else {
        return Ok(false);
    };
    if last.state != State::Failed {
        return Ok(false);
    }
    let refusals = notes.refusals(id);
    let Some(seen) = refusals.iter().find(|r| r.job == last.id) else {
        return Ok(false);
    };
    let n = refusals.len() as u64;
    if n >= ctx.config.abandoned_run_limit || busy(jobs, row) {
        return Ok(false);
    }
    let wait = jobs_backoff(ctx, n);
    if clock.0 < seen.clock.saturating_add(wait) {
        return Ok(false);
    }
    let (_, keep_branch) = read_remove_key(&last.input_key);
    let key = JobKey {
        kind: Kind::WorktreeRemove,
        subject: id,
        input_key: &last.input_key,
    };
    if !jobs.queues(ctx, &key) {
        return Ok(false);
    }
    let cause = "its last removal was refused by a process in the tree";
    let queued = queue_remove(ctx, tx, row, cause, keep_branch, clock)?;
    Ok(matches!(queued, Ok(Queued::New(_))))
}

fn jobs_backoff(ctx: &JobCtx<'_>, refusals: u64) -> i64 {
    let s = backoff_s(&ctx.config.job_backoff_s, refusals);
    i64::try_from(s).unwrap_or(i64::MAX)
}

const REFUSED: &str = "removal_refused_for_now";

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct Refusal {
    job: i64,
    clock: i64,
}

fn refusals(tx: &Connection, id: &str) -> Result<Vec<Refusal>> {
    Ok(facts_of(tx, Entity::Worktree, id, EventKind::Note)?
        .into_iter()
        .filter(|f| f.cause == REFUSED)
        .filter_map(|f| serde_json::from_str(&f.data_json).ok())
        .collect())
}

pub const OWNER_ENDED: &str = "owner_ended";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct OwnerEnded {
    pub keep_branch: bool,
    pub rescue_job: Option<i64>,
}

pub fn record_owner_end(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    id: &str,
    actor: Actor,
    ended: &OwnerEnded,
) -> Result<()> {
    let cause = Cause {
        actor,
        text: OWNER_ENDED,
    };
    let data = serde_json::to_value(ended)?;
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::Worktree,
        id,
        cause,
        &data,
    )?;
    Ok(())
}

pub fn owners_ended(tx: &Connection) -> Result<BTreeMap<String, OwnerEnded>> {
    Ok(Notes::read(tx)?.ended)
}

pub fn removal_failed(tx: &Connection, id: &str, path: &str) -> Result<bool> {
    let key = remove_key(path, false);
    let last = jobs_store::latest(tx, Kind::WorktreeRemove, id, &key)?;
    Ok(last.is_some_and(|j| j.state == State::Failed))
}

pub fn latest_removal(tx: &Connection, id: &str, path: &str) -> Result<Option<JobRow>> {
    let mut newest: Option<JobRow> = None;
    for keep_branch in [false, true] {
        let key = remove_key(path, keep_branch);
        if let Some(run) = jobs_store::latest(tx, Kind::WorktreeRemove, id, &key)? {
            if newest.as_ref().is_none_or(|n| run.id > n.id) {
                newest = Some(run);
            }
        }
    }
    Ok(newest)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Removed,
    Failed,
    Kept,
}

pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow) -> Result<Observed> {
    if job.kind != Kind::WorktreeRemove {
        bail!(
            "job {} is a {} job, not a worktree_remove job",
            job.id,
            job.kind
        );
    }
    let id = job.subject.as_str();
    match job.state {
        State::Passed => match result_data(job).and_then(|d| serde_json::from_value(d).ok()) {
            Some(r) => removed(ctx, tx, id, &r),
            None => failed(ctx, tx, job, "the passed run gives no removal"),
        },
        State::Failed => {
            let reason = outcome(job).map(|o| o.text).unwrap_or_default();
            failed(ctx, tx, job, &reason)
        }
        State::Queued | State::Running | State::Abandoned => Ok(Observed::Kept),
    }
}

fn removed(ctx: &JobCtx<'_>, tx: &Connection, id: &str, r: &Removed) -> Result<Observed> {
    if store::get(tx, id)?.is_none() {
        return Ok(Observed::Kept);
    }
    let (files, verb) = if r.ignored_files == 1 {
        ("file", "was")
    } else {
        ("files", "were")
    };
    let text = format!(
        "its worktree_remove job removed the tree; {} ignored {files} {verb} deleted with it",
        r.ignored_files
    );
    let cause = Cause {
        actor: Actor::Horch,
        text: &text,
    };
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    match transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "removed",
        cause,
        refusals,
    ) {
        Ok(Moved::Changed) => {
            tx.execute(
                "UPDATE worktrees SET cleaned_at = ?2,
                    status = CASE WHEN status IN (?3, ?5) THEN ?4 ELSE status END
                 WHERE worktree_id = ?1",
                params![
                    id,
                    util::now(),
                    WorktreeStatus::Landed.as_str(),
                    WorktreeStatus::Cleaned.as_str(),
                    WorktreeStatus::Open.as_str()
                ],
            )?;
        }
        Ok(Moved::Unchanged) => {}
        Err(DoorError::Refused { .. }) => return Ok(Observed::Kept),
        Err(e) => return Err(e.into()),
    }
    ctx.needs_you
        .resolve_item(tx, &removal::needs_you_key(id))?;
    Ok(Observed::Removed)
}

fn failed(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow, reason: &str) -> Result<Observed> {
    let id = job.subject.as_str();
    if removal::transient(reason) {
        let mut seen = refusals(tx, id)?;
        if !seen.iter().any(|r| r.job == job.id) {
            let refusal = Refusal {
                job: job.id,
                clock: create::fleet_clock(tx)?.0,
            };
            let cause = Cause {
                actor: Actor::Horch,
                text: REFUSED,
            };
            let data = serde_json::to_value(refusal)?;
            record(
                tx,
                ctx.guard,
                EventKind::Note,
                Entity::Worktree,
                id,
                cause,
                &data,
            )?;
            seen.push(refusal);
        }
        if (seen.len() as u64) < ctx.config.abandoned_run_limit {
            return Ok(Observed::Failed);
        }
    }
    let key = removal::needs_you_key(id);
    let text = format!(
        "The removal of worktree {id} failed, so its tree stays: {reason}. \
         Correct the cause, then run horch job requeue {}, or horch worktree drop or cleanup.",
        job.id
    );
    let item = NewItem {
        key: &key,
        kind: removal::NEEDS_YOU_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)?;
    Ok(Observed::Failed)
}

pub fn remove_tree(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    id: &str,
    cause: Cause<'_>,
    keep_branch: bool,
) -> Result<Result<(), String>> {
    let row = match removable_row(ctx, conn, id, cause)? {
        Ok(row) => row,
        Err(why) => return Ok(Err(why)),
    };
    if !matches!(row.state, WorktreeState::Landed | WorktreeState::Rescued) {
        if let Err(e) = create::rescue_now(conn, ctx, jobs, &row) {
            return Ok(Err(format!("{e:#}")));
        }
    }
    remove_now(ctx, conn, jobs, id, cause.text, keep_branch)
}

fn immediate(conn: &mut Connection) -> Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

fn removable_row(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    id: &str,
    cause: Cause<'_>,
) -> Result<Result<store::WorktreeRow, String>> {
    let tx = immediate(conn)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let keep = [
        WorktreeState::Landed,
        WorktreeState::Landing,
        WorktreeState::Orphaned,
        WorktreeState::Rescued,
    ];
    if keep.contains(&row.state) {
        return Ok(Ok(row));
    }
    let moved = transition(
        &tx,
        None,
        Entity::Worktree,
        id,
        "orphaned",
        cause,
        ctx.needs_you,
    );
    if let Err(e) = moved {
        tx.commit()?;
        return Ok(Err(format!("worktree {id} is {}: {e}", row.state)));
    }
    store::set_owner(&tx, id, None)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    tx.commit()?;
    Ok(Ok(row))
}

fn remove_now(
    ctx: &JobCtx<'_>,
    conn: &mut Connection,
    jobs: &dyn RunJob,
    id: &str,
    cause: &str,
    keep_branch: bool,
) -> Result<Result<(), String>> {
    let tx = immediate(conn)?;
    let clock = create::fleet_clock(&tx)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let job = match queue_remove(ctx, &tx, &row, cause, keep_branch, clock)? {
        Ok(Queued::New(job)) => job,
        Ok(Queued::Active(r) | Queued::Reused(r)) => r.id,
        Ok(Queued::Held(r)) => {
            return Ok(Err(format!(
                "the worktree_remove job of {} ended abandoned {} times; run horch job requeue {} after you read why",
                row.path, r.abandoned_runs, r.id
            )))
        }
        Err(why) => return Ok(Err(why)),
    };
    tx.commit()?;
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = observe(ctx, &tx, &ended)?;
    tx.commit()?;
    Ok(match seen {
        Observed::Removed => Ok(()),
        _ => Err(outcome(&ended)
            .map(|o| o.text)
            .unwrap_or_else(|| format!("its worktree_remove job ended {}", ended.state.as_str()))),
    })
}
