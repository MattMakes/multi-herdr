//! The reconciler of the worktree rows with git.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" ("The reconciler compares three views"), "Supervision, liveness
//! and reconciliation" (`reconcile_worktrees(tx, trees)` in part 2 of a
//! pass), "Migration plan" item 5, RR-10, I6, FM-25, FM-26, FM-27;
//! `ddd/08-code/worktree-custody/design.md` §3 (`ReconcileWorktrees`).
//!
//! [`reconcile_worktrees`] compares the `worktrees` rows with the facts of
//! part 1 of a pass: the folder scan of each row, the folders under
//! `<project>/.worktrees/` of each run, and the last passed `worktree_list`
//! run of each repository. It runs no git. The 1 folder it reads is the
//! one that the observation of a `worktree_add` run checks. It is
//! level-triggered: a second call with the same facts writes nothing more.
//!
//! It acts (MAP.md of fleetd-p3, Q9): it queues 1 `rescue` job for each
//! `orphaned` tree, which has no verified rescue of its state now: a
//! verified rescue moves it to `rescued`. It queues 1 `worktree_remove` job
//! for each `landed` tree, and for each `rescued` tree whose idea is
//! `merged` ("Ideas finish clean", ticket 33 of fleetd-p6); the rescue ref
//! stays. A `rescued` tree with no task is removed only when its owner gave
//! it up: a released gate tree or a dropped tree. The jobs run
//! outside the pass, and the guard of the remove job checks each tree
//! again at its removal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::adapters::scans::{FolderFact, Presence};
use crate::fleetd::fleet_runtime::adapters::sqlite::last_moves_to;
use crate::fleetd::fleet_runtime::application::jobs::{
    self, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{
    record, transition, Cause, DoorError, Moved,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::integration::adapters::sqlite as branches_store;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::worktree_custody::adapters::git::{worktree_add_key, ListResult};
use crate::fleetd::worktree_custody::adapters::rescue_git::{rescue_key, Rescue};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::remove::{Notes, TreeJobs};
use crate::fleetd::worktree_custody::application::{create, remove, rescue};
use crate::fleetd::worktree_custody::domain::removal::quarantine_path;
use crate::fleetd::worktree_custody::domain::worktree::{
    self as rules, Attempt, Breach, Folder, LandingTree, Owner, OwnerGone, RunBranch, Seen,
    TaskNow, TreeJob,
};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerStatus, WorktreeStatus};
use crate::taskledger::store::one_snapshot;
use crate::taskledger::{runs, tasks, util};

/// The folders under `<project>/.worktrees/` of 1 repository (Q4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDir {
    pub repo: String,
    /// The full path of each folder.
    pub folders: Vec<String>,
}

/// The last passed `worktree_list` run of 1 repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub repo: String,
    /// The id of the run in `jobs`.
    pub job: i64,
    /// When the run read its facts: the `seen_at` of each row it names.
    pub seen_at: String,
    pub result: ListResult,
}

/// The worktree facts of part 1 of a pass. A path is written as git
/// writes it, so that a folder, an entry and a row compare as text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The folder scan of the rows, by worktree id. A row with no fact was
    /// not scanned in this pass.
    pub folders: Vec<FolderFact>,
    pub work_dirs: Vec<WorkDir>,
    pub lists: Vec<Listed>,
}

/// What 1 call did. Each list but `not_trees` holds worktree ids, in row
/// order; `not_trees` holds paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The ended runs of the worktree jobs that the call observed.
    pub observed: Vec<i64>,
    pub adopted: Vec<String>,
    pub orphaned: Vec<String>,
    /// Moved to `created`: the task is parked by `done --unlanded`, or it
    /// waits for its next attempt.
    pub created: Vec<String>,
    /// Moved to `landing`: the attempt ended verified.
    pub landing: Vec<String>,
    /// Moved from `landing` to `landed`: the task branch is `landed`.
    pub landed: Vec<String>,
    pub vanished: Vec<String>,
    /// The rows that got new facts from a `worktree_list` result.
    pub refreshed: Vec<String>,
    /// The trees that got a new `rescue` run.
    pub rescues: Vec<String>,
    /// The trees that got a new `worktree_remove` run.
    pub removals: Vec<String>,
    /// The path of each folder of a work directory that the last
    /// `worktree_list` result of its repository does not name: not a git
    /// worktree, so it gets no row and no job.
    pub not_trees: Vec<String>,
}

fn by_horch(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Reconcile the worktree rows with `facts`, in `tx`, at `clock` (part 2
/// of a pass). In order: observe the ended worktree job runs; adopt each
/// tree with no row as `orphaned`; write the facts of the `worktree_list`
/// results; move each `active` row whose owner is gone; escalate each
/// vanished folder; queue the rescue of each orphan and the removal of each
/// landed tree. Between the owners and the escalation, each `landing` row
/// follows its task branch. Each step reads the rows that the step before
/// it wrote.
pub fn reconcile_worktrees(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    facts: &Facts,
    clock: FleetSeconds,
) -> Result<Report> {
    let mut report = Report::default();
    observe_runs(ctx, tx, &mut report)?;
    adopt(ctx, tx, facts, &mut report)?;
    refresh(tx, facts, &mut report)?;
    owners(ctx, tx, &mut report)?;
    landings(ctx, tx, &mut report)?;
    escalate_vanished(ctx, tx, facts, clock, &mut report)?;
    act(ctx, tx, clock, &mut report)?;
    Ok(report)
}

/// The breaches of I6 that `facts` show, in `conn`. A breach is reported;
/// the rule that repairs it acts on the next pass.
pub fn check_i6(conn: &Connection, facts: &Facts) -> Result<Vec<Breach>> {
    let (rows, jobs, removing) = one_snapshot(conn, || {
        let rows = store::in_states(conn, &WorktreeState::ALL)?;
        let kinds = [Kind::WorktreeAdd, Kind::WorktreeRemove];
        let jobs = TreeJobs::read(conn, &kinds, rows.iter().map(|r| r.worktree_id.as_str()))?;
        Ok((rows, jobs, removing(conn)?))
    })?;
    let live: Vec<&WorktreeRow> = rows
        .iter()
        .filter(|r| r.state != WorktreeState::Removed)
        .collect();
    let known = known_paths(live.iter().copied(), facts);
    let seen = seen(&jobs, &rows, facts);
    let mut out: Vec<Breach> = rules::adoptions(&known, &seen, &[])
        .into_iter()
        .map(|a| Breach::NoRow {
            repo: a.repo,
            path: a.path,
        })
        .collect();
    for r in &rows {
        let id = r.worktree_id.clone();
        let state = r.state.as_str();
        if rules::vanished(
            state,
            folder(facts, &id),
            false,
            set_aside(&removing, facts, r),
        ) {
            out.push(Breach::NoFolder { id });
        } else if r.state == WorktreeState::Reserved && add_run(&jobs, r).is_none() {
            out.push(Breach::ReservedWithNoJob { id });
        } else if r.state == WorktreeState::Removed
            && !rules::removal_saved(r.status.as_str(), r.rescue_ref.as_deref())
        {
            out.push(Breach::RemovedUnsaved { id });
        }
    }
    Ok(out)
}

// ─── the steps ──────────────────────────────────────────────────────────

/// Each row, oldest first.
fn all_rows(tx: &Connection) -> Result<Vec<WorktreeRow>> {
    store::in_states(tx, &WorktreeState::ALL)
}

fn add_run<'a>(jobs: &'a TreeJobs, row: &WorktreeRow) -> Option<&'a jobs_store::JobRow> {
    jobs.latest(Kind::WorktreeAdd, &row.worktree_id, &worktree_add_key(row))
}

/// Observe the last run of the job that each row waits for, when it
/// ended `passed` or `failed`: the observations of `create`, `rescue` and
/// `remove`. A rescue run is the newest of the tree, of any HEAD: a drop
/// queues it at the HEAD that the command read, which the row may not
/// have yet, and a run that is not observed gets a second rescue. A passed
/// rescue whose ref the row has already is the rescue of an earlier orphan
/// of a reused tree: it does not save the tree now, so it is not observed.
///
/// 1 snapshot reads the rows and the runs of their jobs: 2 statements for
/// any count of rows.
fn observe_runs(ctx: &JobCtx<'_>, tx: &Connection, report: &mut Report) -> Result<()> {
    let (rows, jobs) = one_snapshot(tx, || {
        let rows = all_rows(tx)?;
        let kinds = [Kind::WorktreeAdd, Kind::Rescue, Kind::WorktreeRemove];
        let waiting = rows
            .iter()
            .filter(|r| rules::awaited(r.state.as_str()).is_some())
            .map(|r| r.worktree_id.as_str());
        let jobs = TreeJobs::read(tx, &kinds, waiting)?;
        Ok((rows, jobs))
    })?;
    for row in rows {
        let id = row.worktree_id.as_str();
        let Some(awaited) = rules::awaited(row.state.as_str()) else {
            continue;
        };
        let run = match awaited {
            TreeJob::Add => add_run(&jobs, &row),
            TreeJob::Rescue => jobs.last(Kind::Rescue, id),
            TreeJob::Remove => jobs.latest_removal(id, &row.path),
        };
        let Some(job) = run.filter(|j| matches!(j.state, State::Passed | State::Failed)) else {
            continue;
        };
        if awaited == TreeJob::Rescue && applied(job, &row) {
            continue;
        }
        match awaited {
            TreeJob::Add => {
                create::observe(ctx, tx, job)?;
            }
            TreeJob::Rescue => {
                rescue::observe(ctx, tx, job)?;
            }
            TreeJob::Remove => {
                remove::observe(ctx, tx, job)?;
            }
        }
        report.observed.push(job.id);
    }
    Ok(())
}

/// Whether the passed rescue run `job` names the rescue ref that `row` has.
fn applied(job: &jobs_store::JobRow, row: &WorktreeRow) -> bool {
    job.state == State::Passed
        && result_data(job)
            .and_then(|d| serde_json::from_value::<Rescue>(d).ok())
            .is_some_and(|r| row.rescue_ref.as_deref() == Some(r.rescue_ref.as_str()))
}

/// The path of each row in `rows`, and of each tree that a `worktree_list`
/// run matched to a row.
fn known_paths<'a>(
    rows: impl Iterator<Item = &'a WorktreeRow>,
    facts: &'a Facts,
) -> BTreeSet<&'a str> {
    let listed = facts
        .lists
        .iter()
        .flat_map(|l| &l.result.trees)
        .filter(|t| t.worktree_id.is_some())
        .map(|t| t.path.as_str());
    rows.map(|r| r.path.as_str()).chain(listed).collect()
}

/// The trees that the views show: each entry of `git worktree list` that
/// [`admits`] gives as a tree now. A folder of a work directory is a tree
/// only when the last `worktree_list` result of its repository names it:
/// with no result yet, its adoption waits for one.
fn seen(jobs: &TreeJobs, rows: &[WorktreeRow], facts: &Facts) -> Vec<Seen> {
    let mut out = Vec::new();
    for l in &facts.lists {
        for (n, e) in l.result.entries.iter().enumerate() {
            let main = n == 0 || e.bare;
            if !main && !admits(jobs, rows, facts, l, &e.path) {
                continue;
            }
            out.push(Seen {
                repo: l.repo.clone(),
                path: e.path.clone(),
                branch: e.branch.as_deref().map(short_branch),
                main,
                gone: e.prunable.is_some(),
            });
        }
    }
    out
}

/// What shows that the folder of an entry of a `worktree_list` result is
/// there now, besides the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Evidence {
    /// The scan of this pass read the folder of a row at the path.
    RowScan(Folder),
    /// The path is in the work directory `<repo>/.worktrees/`, which the
    /// scan of this pass lists: whether the listing holds it.
    WorkDirScan { listed: bool },
    /// No scan of this pass reads the path: a folder that no row names,
    /// outside the work directory. The list is the only evidence.
    ListOnly,
}

/// The evidence for the folder `path` of an entry of `list`.
fn evidence(facts: &Facts, rows: &[WorktreeRow], list: &Listed, path: &str) -> Evidence {
    let read = rows
        .iter()
        .filter(|r| r.path == path)
        .map(|r| folder(facts, &r.worktree_id))
        .filter(|f| *f != Folder::Unknown)
        .max_by_key(|f| *f == Folder::Present);
    if let Some(f) = read {
        return Evidence::RowScan(f);
    }
    let work_dir = Path::new(&list.repo).join(".worktrees");
    if Path::new(path).parent() == Some(work_dir.as_path()) {
        let listed = facts
            .work_dirs
            .iter()
            .any(|d| d.repo == list.repo && d.folders.iter().any(|f| f == path));
        return Evidence::WorkDirScan { listed };
    }
    Evidence::ListOnly
}

/// Whether the entry `path` of `list` is a tree now: its folder is not
/// shown gone by a scan of this pass, and no passed removal of the path
/// made the list result stale.
fn admits(jobs: &TreeJobs, rows: &[WorktreeRow], facts: &Facts, list: &Listed, path: &str) -> bool {
    let shown = match evidence(facts, rows, list, path) {
        Evidence::RowScan(f) => f == Folder::Present,
        Evidence::WorkDirScan { listed } => listed,
        Evidence::ListOnly => true,
    };
    shown && !stale(jobs, rows, list, path)
}

/// Whether a passed removal of the folder `path` was queued after the run
/// of `list`: the run may have read the tree before it went. The order is
/// the order of the ids of the runs, which cannot tie, as 2 times of 1 s
/// can. A list run queued after the removal but started before its end is
/// not stale by this rule: the scan of [`evidence`] shows that the folder
/// is gone.
fn stale(jobs: &TreeJobs, rows: &[WorktreeRow], list: &Listed, path: &str) -> bool {
    rows.iter()
        .filter(|r| r.path == path)
        .filter_map(|r| jobs.last(Kind::WorktreeRemove, &r.worktree_id))
        .any(|job| job.state == State::Passed && job.id > list.job)
}

/// The folders of the work directories that no row owns and that the
/// `worktree_list` result of their repository does not name.
fn plain_folders(known: &BTreeSet<&str>, facts: &Facts) -> Vec<String> {
    let mut out = Vec::new();
    for d in &facts.work_dirs {
        let lists: Vec<&Listed> = facts.lists.iter().filter(|l| l.repo == d.repo).collect();
        if lists.is_empty() {
            continue;
        }
        let named = |path: &str| {
            lists
                .iter()
                .any(|l| l.result.entries.iter().any(|e| e.path == path))
        };
        out.extend(
            d.folders
                .iter()
                .filter(|path| !named(path) && rules::unowned(known, path))
                .cloned(),
        );
    }
    out
}

fn short_branch(full: &str) -> String {
    full.strip_prefix("refs/heads/").unwrap_or(full).to_string()
}

/// Adopt each tree that no row owns as `orphaned`, with the task that its
/// branch names (RR-10, FM-26, Q15). A `removed` row owns no tree: a
/// folder at its path gets a new row, as [`check_i6`] reads it. A folder
/// that git does not name is reported in `not_trees` and gets no row.
fn adopt(ctx: &JobCtx<'_>, tx: &Connection, facts: &Facts, report: &mut Report) -> Result<()> {
    let (rows, jobs) = one_snapshot(tx, || {
        let rows = all_rows(tx)?;
        let ids = rows.iter().map(|r| r.worktree_id.as_str());
        let jobs = TreeJobs::read(tx, &[Kind::WorktreeRemove], ids)?;
        Ok((rows, jobs))
    })?;
    let live = rows.iter().filter(|r| r.state != WorktreeState::Removed);
    let known = known_paths(live, facts);
    report.not_trees = plain_folders(&known, facts);
    let seen = seen(&jobs, &rows, facts);
    // The runs are read only for the repositories of a tree to adopt.
    let repos: BTreeSet<String> = rules::adoptions(&known, &seen, &[])
        .into_iter()
        .map(|a| a.repo)
        .collect();
    let mut branches = Vec::new();
    for repo in repos {
        for run in runs::of_project(tx, Path::new(&repo))? {
            if let Some(branch) = run.run_branch {
                branches.push(RunBranch {
                    repo: repo.clone(),
                    run_id: run.run_id,
                    branch,
                });
            }
        }
    }
    for a in rules::adoptions(&known, &seen, &branches) {
        let task_id = match &a.task {
            Some(t) => tasks::of_run(tx, &t.run_id)?
                .into_iter()
                .find(|task| task.seq == t.seq)
                .map(|task| task.task_id),
            None => None,
        };
        let new = NewWorktree {
            worktree_id: util::new_id(),
            task_id,
            worker_id: None,
            repo_root: a.repo,
            path: a.path,
            branch: a.branch,
            base: String::new(),
            base_commit: None,
            status: WorktreeStatus::Open,
            mode: None,
        };
        let cause = by_horch("the reconciler adopted a tree that no row owns");
        store::insert_orphan(tx, ctx.guard, &new, cause)?;
        report.adopted.push(new.worktree_id);
    }
    Ok(())
}

/// Write the HEAD, the dirty flag and the size of each tree of a row that
/// a `worktree_list` result read with no error, when they changed (Q11).
fn refresh(tx: &Connection, facts: &Facts, report: &mut Report) -> Result<()> {
    let rows = all_rows(tx)?;
    for l in &facts.lists {
        for t in &l.result.trees {
            let (Some(head), Some(dirty), None) = (&t.head, t.dirty, &t.error) else {
                continue;
            };
            // A `removed` row of the same path is history.
            let row = rows
                .iter()
                .filter(|r| r.state != WorktreeState::Removed)
                .find(|r| match &t.worktree_id {
                    Some(id) => &r.worktree_id == id,
                    None => r.path == t.path,
                });
            let Some(row) = row else {
                continue;
            };
            let bytes = t.bytes.and_then(|b| i64::try_from(b).ok());
            let now = (Some(head), dirty, bytes, Some(&l.seen_at));
            let was = (
                row.head_sha.as_ref(),
                row.dirty,
                row.bytes_on_disk,
                row.seen_at.as_ref(),
            );
            if now != was {
                store::set_facts(tx, &row.worktree_id, head, dirty, bytes, &l.seen_at)?;
                report.refreshed.push(row.worktree_id.clone());
            }
        }
    }
    Ok(())
}

/// Move `id` to `to` through the door. `false` when it was there already,
/// or when the door refused the move and recorded the refusal.
fn moved(ctx: &JobCtx<'_>, tx: &Connection, id: &str, to: &str, cause: &str) -> Result<bool> {
    match transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        to,
        by_horch(cause),
        ctx.needs_you,
    ) {
        Ok(Moved::Changed) => Ok(true),
        Ok(Moved::Unchanged) | Err(DoorError::Refused { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The owner of the `active` row `row`, as the ledger gives it in `facts`
/// and `outcomes`. A task id with no task row is an error.
fn owner(
    row: &WorktreeRow,
    outcomes: &BTreeMap<String, Option<String>>,
    facts: &attempts_store::OwnerFacts,
) -> Result<Owner> {
    let attempt = row.owner_attempt.as_deref().map(|a| {
        match facts.attempt_states.get(a).map(String::as_str) {
            None => Attempt::Missing,
            Some("ended") => Attempt::Ended,
            Some(_) => Attempt::Holds,
        }
    });
    let worker_live = row
        .worker_id
        .as_deref()
        .and_then(|w| facts.worker_statuses.get(w))
        .is_some_and(|s| WorkerStatus::parse(s).is_ok_and(WorkerStatus::is_live));
    let task = match row.task_id.as_deref() {
        Some(t) => {
            let (status, held) = facts.tasks.get(t).with_context(|| format!("no task {t}"))?;
            task_now(status, held.as_deref())
        }
        None => TaskNow::Other,
    };
    Ok(Owner {
        attempt,
        worker_live,
        task,
        ended_clean: rules::clean_end(outcome_of(outcomes, row)),
    })
}

fn outcome_of<'a>(
    outcomes: &'a BTreeMap<String, Option<String>>,
    row: &WorktreeRow,
) -> Option<&'a str> {
    let attempt = row.owner_attempt.as_deref()?;
    outcomes.get(attempt)?.as_deref()
}

/// The outcome of the owner attempt of each row of `rows`, in 1 statement
/// for any count of rows.
fn owner_outcomes(
    tx: &Connection,
    rows: &[WorktreeRow],
) -> Result<BTreeMap<String, Option<String>>> {
    attempts_store::outcomes_of(tx, rows.iter().filter_map(|r| r.owner_attempt.as_deref()))
}

fn task_now(status: &str, held_reason: Option<&str>) -> TaskNow {
    if tasks::parked(status, held_reason) {
        return TaskNow::Parked;
    }
    match TaskStatus::parse(status) {
        Ok(TaskStatus::Verifying) => TaskNow::Verifying,
        Ok(TaskStatus::Done) => TaskNow::Done,
        Ok(TaskStatus::Ready | TaskStatus::BlockedOnUser | TaskStatus::WaitingQuota) => {
            TaskNow::Retried
        }
        Ok(TaskStatus::Cancelled) => TaskNow::Cancelled,
        _ => TaskNow::Other,
    }
}

/// Move each `active` row whose owner is gone: `landing` for a verified
/// task, `created` for a parked or retried task, else `orphaned` in this
/// pass (FM-25, Q10, Q23).
fn owners(ctx: &JobCtx<'_>, tx: &Connection, report: &mut Report) -> Result<()> {
    let (rows, outcomes, facts) = one_snapshot(tx, || {
        let rows = store::in_states(tx, &[WorktreeState::Active])?;
        let outcomes = owner_outcomes(tx, &rows)?;
        let facts = attempts_store::owner_facts(
            tx,
            rows.iter().filter_map(|r| r.owner_attempt.as_deref()),
            rows.iter().filter_map(|r| r.worker_id.as_deref()),
            rows.iter().filter_map(|r| r.task_id.as_deref()),
        )?;
        Ok((rows, outcomes, facts))
    })?;
    for row in rows {
        let id = row.worktree_id.as_str();
        let (to, cause, list) = match rules::owner_gone(&owner(&row, &outcomes, &facts)?) {
            None => continue,
            Some(OwnerGone::Created(cause)) => ("created", cause, &mut report.created),
            Some(OwnerGone::Landing) => ("landing", rules::VERIFIED, &mut report.landing),
            Some(OwnerGone::Orphaned(cause)) => ("orphaned", cause, &mut report.orphaned),
        };
        if moved(ctx, tx, id, to, cause)? {
            // A tree in `created` has no owner (Q10); an orphan keeps the
            // attempt that died, for its rescue commit.
            if to == "created" {
                store::set_owner(tx, id, None)?;
            }
            list.push(id.to_string());
        }
    }
    Ok(())
}

/// The landing state of the task branch of each `landing` row, by branch
/// name, and the stamp of the last event that moved that branch to
/// `landed`. Each is 1 set read of the context that owns its table, on 1
/// snapshot: 2 statements for any count of rows.
struct Branches {
    states: BTreeMap<String, Option<String>>,
    landed_at: BTreeMap<String, String>,
}

impl Branches {
    fn read(tx: &Connection, rows: &[WorktreeRow], with_landed_at: bool) -> Result<Branches> {
        let names = || rows.iter().map(|r| r.branch.as_str());
        one_snapshot(tx, || {
            let states = branches_store::task_branch_landing_states(tx, names())?;
            let landed_at = if with_landed_at {
                last_moves_to(tx, Entity::TaskBranch, names(), "landed")?
            } else {
                BTreeMap::new()
            };
            Ok(Branches { states, landed_at })
        })
    }

    fn state(&self, row: &WorktreeRow) -> Option<&str> {
        self.states.get(&row.branch)?.as_deref()
    }
}

/// Move each `landing` row whose task branch is `landed` to `landed`, at
/// the HEAD that a `worktree_list` run read after the landing: its landed
/// commit, the 1 record of the landing for the removal guard and for
/// `horch ledger unfinished`. A row with no such read waits for it. A row
/// whose branch is `abandoned` gets its rescue in [`act`].
fn landings(ctx: &JobCtx<'_>, tx: &Connection, report: &mut Report) -> Result<()> {
    let (rows, branches) = one_snapshot(tx, || {
        let rows = store::in_states(tx, &[WorktreeState::Landing])?;
        let branches = Branches::read(tx, &rows, true)?;
        Ok((rows, branches))
    })?;
    for row in rows {
        let landed_at = branches.landed_at.get(&row.branch);
        if rules::landing_next(branches.state(&row)) != Some(LandingTree::Landed) {
            continue;
        }
        let Some(head) = row.head_sha.as_deref() else {
            continue;
        };
        if !rules::read_after_landing(row.seen_at.as_deref(), landed_at.map(String::as_str)) {
            continue;
        }
        let id = row.worktree_id.as_str();
        store::set_landed(tx, id, head)?;
        if moved(ctx, tx, id, "landed", "its task branch is landed")? {
            report.landed.push(id.to_string());
        }
    }
    Ok(())
}

/// What the folder scan saw at the row `id`.
fn folder(facts: &Facts, id: &str) -> Folder {
    match facts
        .folders
        .iter()
        .find(|f| f.key == id)
        .map(|f| &f.presence)
    {
        Some(Presence::Present) => Folder::Present,
        Some(Presence::Missing) => Folder::Missing,
        Some(Presence::Unreadable(_)) | None => Folder::Unknown,
    }
}

/// Whether `git worktree list` of the repository of `row` shows its tree,
/// with a folder.
fn in_git(facts: &Facts, row: &WorktreeRow) -> bool {
    facts
        .lists
        .iter()
        .filter(|l| l.repo == row.repo_root)
        .any(|l| {
            l.result
                .entries
                .iter()
                .any(|e| e.prunable.is_none() && names_row(&l.result, &e.path, row))
        })
}

/// Whether the entry at `path` of `result` is the tree of `row`: by its
/// path as text, or by the row that the `worktree_list` run matched to it.
fn names_row(result: &ListResult, path: &str, row: &WorktreeRow) -> bool {
    let same_path = path == row.path;
    let same_row = result
        .trees
        .iter()
        .any(|t| t.path == path && t.worktree_id.as_ref() == Some(&row.worktree_id));
    same_path || same_row
}

/// Whether a removal set the tree of `row` aside: its remove run is
/// active, or its quarantine folder is in the work directory.
fn set_aside(removing: &BTreeSet<String>, facts: &Facts, row: &WorktreeRow) -> bool {
    let quarantine = quarantine_path(Path::new(&row.path), &row.worktree_id);
    let quarantine = quarantine.display().to_string();
    let in_work_dir = facts
        .work_dirs
        .iter()
        .filter(|d| d.repo == row.repo_root)
        .any(|d| d.folders.contains(&quarantine));
    in_work_dir || removing.contains(&row.worktree_id)
}

/// 1 statement for any count of trees: check_i6 and escalate_vanished judge
/// each tree, so this read stays whole.
fn removing(conn: &Connection) -> Result<BTreeSet<String>> {
    jobs_store::active_subjects(conn, Kind::WorktreeRemove)
}

/// Escalate each folder that vanished without a removal by fleetd: 1
/// event and 1 needs-you item, 1 time for each row. The row stays (FM-27).
fn escalate_vanished(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    facts: &Facts,
    clock: FleetSeconds,
    report: &mut Report,
) -> Result<()> {
    let (rows, removing, notes) =
        one_snapshot(tx, || Ok((all_rows(tx)?, removing(tx)?, Notes::read(tx)?)))?;
    for row in rows {
        let id = row.worktree_id.as_str();
        let gone = rules::vanished(
            row.state.as_str(),
            folder(facts, id),
            in_git(facts, &row),
            set_aside(&removing, facts, &row),
        );
        if !gone {
            continue;
        }
        report.vanished.push(id.to_string());
        if notes.has(id, rules::VANISHED_KIND) {
            continue;
        }
        let data = json!({ "path": row.path, "fleet_clock": clock.0 });
        let cause = by_horch(rules::VANISHED_KIND);
        record(
            tx,
            ctx.guard,
            EventKind::Note,
            Entity::Worktree,
            id,
            cause,
            &data,
        )?;
        let key = rules::vanished_key(id);
        let text = format!(
            "The folder {} of worktree {id} is gone, and fleetd did not remove it. \
             Find what deleted it; the row stays.",
            row.path
        );
        let item = NewItem {
            key: &key,
            kind: rules::VANISHED_KIND,
            entity: Entity::Worktree,
            entity_id: id,
            text: &text,
        };
        ctx.needs_you.open_item(tx, &item)?;
    }
    Ok(())
}

/// Queue 1 `rescue` run of each `orphaned` row and of each `landing` row
/// whose task branch is `abandoned`, and 1
/// `worktree_remove` run of each `landed` row (FM-25, FM-27, Q8, Q9), of
/// each `rescued` row whose idea is `merged` ("Ideas finish clean"), and of
/// each `rescued` row with no task that its owner gave up ([`given_up`]).
/// A removal refused by a process in the tree runs again
/// ([`remove::retry`]). A row whose folder vanished is escalated, not
/// queued. A row with an active run of that kind is not queued again.
///
/// 1 snapshot reads the rows, the task branches of the `landing` rows, the
/// notes and the job runs of the trees and of their repositories, and the
/// idea states of the tasks of the `rescued` rows: 7 statements for any
/// count of rows.
fn act(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds, report: &mut Report) -> Result<()> {
    let (rows, branches, notes, jobs, ideas) = one_snapshot(tx, || {
        let rows = all_rows(tx)?;
        let landing: Vec<WorktreeRow> = rows
            .iter()
            .filter(|r| r.state == WorktreeState::Landing)
            .cloned()
            .collect();
        let branches = Branches::read(tx, &landing, false)?;
        let kinds = [Kind::Rescue, Kind::WorktreeRemove, Kind::WorktreeList];
        let ids = rows.iter().map(|r| r.worktree_id.as_str());
        let repos = rows.iter().map(|r| r.repo_root.as_str());
        let jobs = TreeJobs::read(tx, &kinds, ids.chain(repos))?;
        let ideas = IdeaStates::read(tx, &rows)?;
        Ok((rows, branches, Notes::read(tx)?, jobs, ideas))
    })?;
    for row in rows {
        let id = row.worktree_id.as_str();
        if report.vanished.iter().any(|v| v == id) {
            continue;
        }
        let removable = matches!(row.state, WorktreeState::Landed | WorktreeState::Rescued);
        if removable && remove::retry(ctx, tx, &jobs, &notes, &row, clock)? {
            report.removals.push(id.to_string());
            continue;
        }
        match rules::to_queue(row.state.as_str()) {
            Some(TreeJob::Rescue) => {
                if queue_rescue(ctx, tx, &jobs, &row, clock)? {
                    report.rescues.push(id.to_string());
                }
            }
            Some(TreeJob::Remove) => {
                let cause = "its tree is landed";
                if remove::queue_auto_remove(ctx, tx, &jobs, &row, cause, clock)? {
                    report.removals.push(id.to_string());
                }
            }
            Some(TreeJob::Add) | None if row.state == WorktreeState::Landing => {
                if rules::landing_next(branches.state(&row)) == Some(LandingTree::Rescue)
                    && queue_rescue(ctx, tx, &jobs, &row, clock)?
                {
                    report.rescues.push(id.to_string());
                }
            }
            Some(TreeJob::Add) | None => {
                if row.state != WorktreeState::Rescued {
                    continue;
                }
                let owner = notes.ended.get(id);
                let keep_branch = owner.is_some_and(|o| o.keep_branch);
                let cause = if ideas.merged(&row) {
                    "its idea is merged"
                } else if given_up(&jobs, &row, owner) {
                    "its owner gave it up, and its rescue ref is verified"
                } else {
                    continue;
                };
                if remove::auto_remove(ctx, tx, &jobs, &row, cause, keep_branch, clock)? {
                    report.removals.push(id.to_string());
                }
            }
        }
    }
    Ok(())
}

/// Whether the `rescued` row `row` has no task and its owner gave it up
/// (`owner`): fleetd released its gate tree ("removed at close", row W11
/// of preservation-04), or a worker dropped it (RR-13). A drop counts only
/// with the rescue ref of the rescue run that it queued; a later rescue
/// holds work that came after the drop. A tree with no task that nobody
/// gave up stays.
/// The rescue run of a drop is a run of the tree, so `jobs` holds it.
fn given_up(jobs: &TreeJobs, row: &WorktreeRow, owner: Option<&remove::OwnerEnded>) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    if row.task_id.is_some() {
        return false;
    }
    let Some(job) = owner.rescue_job else {
        return true;
    };
    jobs.get(Kind::Rescue, &row.worktree_id, job)
        .is_some_and(|j| applied(j, row))
}

/// The idea state of the task of each `rescued` row, by task id: 2 set
/// reads of the contexts that own the tables.
struct IdeaStates(BTreeMap<String, String>);

impl IdeaStates {
    fn read(tx: &Connection, rows: &[WorktreeRow]) -> Result<IdeaStates> {
        let tasks = rows
            .iter()
            .filter(|r| r.state == WorktreeState::Rescued)
            .filter_map(|r| r.task_id.as_deref());
        let ideas = attempts_store::ideas_of_tasks(tx, tasks)?;
        let states = ideas_store::states_of(tx, ideas.values().map(String::as_str))?;
        let by_task = ideas
            .into_iter()
            .filter_map(|(task, idea)| Some((task, states.get(&idea)?.clone())))
            .collect();
        Ok(IdeaStates(by_task))
    }

    /// Whether the idea of the task of `row` is `merged`. A row with no
    /// task has no idea.
    fn merged(&self, row: &WorktreeRow) -> bool {
        row.task_id
            .as_deref()
            .and_then(|t| self.0.get(t))
            .is_some_and(|s| s == "merged")
    }
}

/// Queue the `rescue` run of the orphan `row` at the HEAD that the last
/// `worktree_list` result gave; `true` for a new run. A row with no HEAD
/// waits for that result. The run of the same path and HEAD that ended
/// `failed` is not run again: a failed rescue keeps the tree and its
/// needs-you item, and a new HEAD is a new input. A passed run of the same
/// input saved an earlier orphan of a reused tree, so a new run saves the
/// tree now. A run that ended `abandoned` runs again after its backoff, up
/// to `abandoned_run_limit`.
fn queue_rescue(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    jobs: &TreeJobs,
    row: &WorktreeRow,
    clock: FleetSeconds,
) -> Result<bool> {
    let Some(head) = row.head_sha.as_deref() else {
        return Ok(false);
    };
    let input_key = rescue_key(&row.path, head);
    let key = JobKey {
        kind: Kind::Rescue,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    let last = jobs.latest(Kind::Rescue, &row.worktree_id, &input_key);
    if last.is_some_and(|j| j.state == State::Failed) || !jobs.queues(ctx, &key) {
        return Ok(false);
    }
    let queued = jobs::queue(ctx, tx, &key, Some(&row.repo_root), clock)?;
    Ok(matches!(queued, Queued::New(_)))
}

#[cfg(test)]
mod removing_tests {
    use super::*;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;
    use rusqlite::params;

    fn ledger(n: usize) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                for i in 0..n {
                    tx.execute(
                        "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base,
                             status, created_at)
                         VALUES (?1, '/p', ?2, ?1, 'main', 'open', 'now')",
                        params![format!("WT{i:04}"), format!("/p/.worktrees/{i}")],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    fn job(conn: &Connection, kind: &str, id: &str, run: i64, state: &str) {
        conn.execute(
            "INSERT INTO jobs (kind, subject, input_key, run, state, time_limit_s, created_at)
             VALUES (?1, ?2, 'k', ?3, ?4, 60, 'now')",
            params![kind, id, run, state],
        )
        .unwrap();
    }

    #[test]
    fn removing_names_the_trees_of_the_read_of_each_tree() {
        let (_tmp, store) = ledger(6);
        let c = store.conn();
        job(c, "worktree_remove", "WT0000", 1, "queued");
        job(c, "worktree_remove", "WT0001", 1, "running");
        job(c, "worktree_remove", "WT0002", 1, "passed");
        job(c, "worktree_remove", "WT0003", 1, "failed");
        job(c, "worktree_remove", "WT0003", 2, "queued");
        job(c, "worktree_add", "WT0004", 1, "queued");
        let want: BTreeSet<String> = all_rows(c)
            .unwrap()
            .into_iter()
            .filter(|r| {
                jobs_store::active(c, Kind::WorktreeRemove, &r.worktree_id)
                    .unwrap()
                    .is_some()
            })
            .map(|r| r.worktree_id)
            .collect();
        assert_eq!(want.len(), 3);
        assert_eq!(removing(c).unwrap(), want);
    }

    #[test]
    fn check_i6_runs_the_same_statements_for_3_and_300_trees() {
        let count = |n: usize| {
            let (_tmp, store) = ledger(n);
            let (breaches, statements) = count_statements(store.conn(), || {
                check_i6(store.conn(), &Facts::default()).unwrap()
            });
            assert_eq!(breaches, vec![]);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}

#[cfg(test)]
mod landing_reads_tests {
    use super::*;
    use crate::fleetd::fleet_runtime::adapters::sqlite::{append_event, Event};
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;
    use rusqlite::{params, OptionalExtension};

    struct TreeWorld {
        _tmp: tempfile::TempDir,
        store: Store,
    }

    fn ledger(trees: &[(&str, &str, &str, Option<&str>)]) -> TreeWorld {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now');",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                tx.execute(
                    "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                         claimable_by, status, created_at, updated_at, idea_id)
                     VALUES ('t1', 'r1', 1, 't1', 'a.md', 'opus', 'opus', 'running', 'now', 'now',
                         ?1)",
                    [idea],
                )?;
                tx.execute_batch(
                    "INSERT INTO workers (worker_id, run_id, persona, host, status, task_id)
                     VALUES ('w1', 'r1', 'opus', 'h', 'working', 't1');
                     INSERT INTO features (id, name, branch, primary_branch, state, created_at,
                         updated_at)
                     VALUES ('F1', 'f', 'feat', 'main', 'open', 'now', 'now');",
                )?;
                for (id, state, branch, owner) in trees {
                    tx.execute(
                        "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch,
                             base, status, state, created_at, owner_attempt)
                         VALUES (?1, 't1', '/p', '/p/.worktrees/' || ?1, ?2, 'main', 'open', ?3,
                             'now', ?4)",
                        params![id, branch, state, owner],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        TreeWorld { _tmp: tmp, store }
    }

    impl TreeWorld {
        fn branch(&mut self, name: &str, state: &str, landed_at: Option<&str>) {
            self.store
                .immediate(|tx| {
                    tx.execute(
                        "INSERT INTO branches (name, feature_id, task_id, landing_state, owner,
                             due_at)
                         VALUES (?1, 'F1', 't1', ?2, 'fleetd', 0)",
                        params![name, state],
                    )?;
                    if let Some(at) = landed_at {
                        let e = Event {
                            entity: Entity::TaskBranch,
                            id: name,
                            from: Some("in_bundle"),
                            to: "landed",
                            cause: "test",
                            actor: Actor::Horch,
                        };
                        append_event(tx, &e)?;
                        tx.execute(
                            "UPDATE events SET at = ?1 WHERE event_id = last_insert_rowid()",
                            [at],
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
        }

        fn attempt(&mut self, id: &str, outcome: Option<&str>) {
            self.store
                .immediate(|tx| {
                    tx.execute(
                        "INSERT INTO attempts (id, task_id, worker_id, state, outcome)
                         VALUES (?1, 't1', 'w1', 'ended', ?2)",
                        params![id, outcome],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
    }

    fn branch_of_one_by_one(
        tx: &Connection,
        row: &WorktreeRow,
    ) -> (Option<String>, Option<String>) {
        let state: Option<Option<String>> = tx
            .query_row(
                "SELECT landing_state FROM branches WHERE name = ?1 AND task_id IS NOT NULL",
                [&row.branch],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        let landed_at: Option<String> = tx
            .query_row(
                "SELECT max(at) FROM events
                 WHERE entity = ?1 AND entity_id = ?2 AND to_state = 'landed'",
                [Entity::TaskBranch.as_str(), row.branch.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        (state.flatten(), landed_at)
    }

    fn outcome_of_one_by_one(tx: &Connection, row: &WorktreeRow) -> Option<String> {
        let attempt = row.owner_attempt.as_deref()?;
        tx.query_row(
            "SELECT outcome FROM attempts WHERE id = ?1",
            [attempt],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .unwrap()
        .flatten()
    }

    #[test]
    fn the_set_reads_give_the_facts_of_the_read_of_each_row() {
        let outcomes = [
            Some("done"),
            Some("crashed"),
            Some("asked"),
            Some("idle"),
            None,
        ];
        let mut l = ledger(&[
            ("L1", "landing", "b-landed", None),
            ("L2", "landing", "b-abandoned", None),
            ("L3", "landing", "b-no-event", None),
            ("L4", "landing", "b-no-row", None),
            ("L5", "landing", "b-landed-twice", None),
            ("A0", "active", "b-a0", None),
        ]);
        let ids: Vec<String> = (0..outcomes.len()).map(|i| format!("t1/A{i}")).collect();
        for (attempt, outcome) in ids.iter().zip(outcomes) {
            l.attempt(attempt, outcome);
        }
        l.store
            .immediate(|tx| {
                for (i, attempt) in ids.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch,
                             base, status, state, created_at, owner_attempt)
                         VALUES (?1, 't1', '/p', '/p/.worktrees/' || ?1, ?1, 'main', 'open',
                             'active', 'now', ?2)",
                        params![format!("A{}", i + 1), attempt],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        l.branch("b-landed", "landed", Some("2026-11-21T01:00:00Z"));
        l.branch("b-abandoned", "abandoned", None);
        l.branch("b-no-event", "landed", None);
        l.branch("b-landed-twice", "landed", Some("2026-11-21T01:00:00Z"));
        l.store
            .immediate(|tx| {
                let e = Event {
                    entity: Entity::TaskBranch,
                    id: "b-landed-twice",
                    from: Some("in_bundle"),
                    to: "landed",
                    cause: "test",
                    actor: Actor::Horch,
                };
                append_event(tx, &e)?;
                tx.execute(
                    "UPDATE events SET at = '2026-11-21T02:00:00Z'
                     WHERE event_id = last_insert_rowid()",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let c = l.store.conn();

        let landing = store::in_states(c, &[WorktreeState::Landing]).unwrap();
        assert_eq!(landing.len(), 5);
        let branches = Branches::read(c, &landing, true).unwrap();
        for row in &landing {
            let set = (
                branches.state(row).map(str::to_string),
                branches.landed_at.get(&row.branch).cloned(),
            );
            assert_eq!(set, branch_of_one_by_one(c, row), "{}", row.worktree_id);
        }
        assert_eq!(
            branches.landed_at.get("b-landed-twice").map(String::as_str),
            Some("2026-11-21T02:00:00Z")
        );

        let active = store::in_states(c, &[WorktreeState::Active]).unwrap();
        assert_eq!(active.len(), 1 + outcomes.len());
        let set = owner_outcomes(c, &active).unwrap();
        for row in &active {
            assert_eq!(
                outcome_of(&set, row).map(str::to_string),
                outcome_of_one_by_one(c, row),
                "{}",
                row.worktree_id
            );
        }
    }

    /// T-286, by count: the steps `landings` and `act` run the same
    /// statements for 3 and for 300 `landing` rows.
    #[test]
    fn landings_and_act_run_the_same_statements_for_3_and_300_landing_rows() {
        let count = |n: usize| {
            let names: Vec<(String, String)> = (0..n)
                .map(|i| (format!("L{i:04}"), format!("b{i:04}")))
                .collect();
            let trees: Vec<(&str, &str, &str, Option<&str>)> = names
                .iter()
                .map(|(id, b)| (id.as_str(), "landing", b.as_str(), None))
                .collect();
            let mut l = ledger(&trees);
            for (_, b) in &names {
                l.branch(b, "in_bundle", None);
            }
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let tx = l.store.conn_mut().transaction().unwrap();
            let mut report = Report::default();
            let ((), statements) = count_statements(&tx, || {
                landings(&ctx, &tx, &mut report).unwrap();
                act(&ctx, &tx, FleetSeconds(0), &mut report).unwrap();
            });
            assert_eq!(report, Report::default());
            statements
        };
        assert_eq!(count(3), count(300));
    }
}

/// T-408: each step of a pass reads its rows and their facts in a fixed
/// count of statements, for any count of rows.
#[cfg(test)]
mod pass_reads_tests {
    use super::*;
    use crate::fleetd::fleet_runtime::adapters::sqlite::{append_fact, Fact};
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::fleetd::worktree_custody::adapters::remove_git::remove_key;
    use crate::fleetd::worktree_custody::application::ports::ListEntry;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;
    use rusqlite::params;

    fn ledger() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now');",
                )?;
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    /// The row `id` in `state` at `/p/.worktrees/<id>`, with HEAD `h`.
    fn tree(c: &Connection, id: &str, state: &str, task: Option<&str>) -> WorktreeRow {
        c.execute(
            "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status,
                 state, created_at, head_sha)
             VALUES (?1, ?2, '/p', '/p/.worktrees/' || ?1, ?1, 'main', 'open', ?3, 'now', 'h')",
            params![id, task, state],
        )
        .unwrap();
        store::get(c, id).unwrap().unwrap()
    }

    fn job(c: &Connection, kind: Kind, subject: &str, key: &str, state: &str) -> i64 {
        c.execute(
            "INSERT INTO jobs (kind, subject, input_key, run, state, time_limit_s, created_at)
             VALUES (?1, ?2, ?3, 1, ?4, 60, 'now')",
            params![kind.as_str(), subject, key, state],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn note(c: &Connection, id: &str, cause: &str, data: &str) {
        let f = Fact {
            kind: EventKind::Note,
            entity: Entity::Worktree,
            id,
            cause,
            actor: Actor::Horch,
            data_json: data,
        };
        append_fact(c, &f).unwrap();
    }

    fn task(c: &Connection, id: &str, seq: usize) {
        let idea = crate::taskledger::schema::run_idea(c, "r1", "now", true).unwrap();
        c.execute(
            "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                 claimable_by, status, created_at, updated_at, idea_id)
             VALUES (?1, 'r1', ?2, ?1, 'a.md', 'opus', 'opus', 'running', 'now', 'now', ?3)",
            params![id, seq as i64, idea],
        )
        .unwrap();
    }

    /// check_i6 with a `reserved` row and its `worktree_add` run, and a
    /// `worktree_list` result that names each row: the read of the add run
    /// of each row and of the last removal of each listed path.
    #[test]
    fn check_i6_reads_the_add_runs_and_removals_of_3_and_300_trees_in_the_same_statements() {
        let count = |n: usize| {
            let (_tmp, mut store) = ledger();
            let tx = store.conn_mut().transaction().unwrap();
            let mut facts = Facts::default();
            let mut entries = vec![ListEntry {
                path: "/p".into(),
                head: Some("h".into()),
                branch: None,
                detached: true,
                bare: false,
                locked: None,
                prunable: None,
            }];
            for i in 0..n {
                let id = format!("R{i:04}");
                let row = tree(&tx, &id, "reserved", None);
                job(
                    &tx,
                    Kind::WorktreeAdd,
                    &id,
                    &worktree_add_key(&row),
                    "queued",
                );
                job(&tx, Kind::WorktreeRemove, &id, "k", "passed");
                facts.folders.push(FolderFact {
                    key: id.clone(),
                    presence: Presence::Present,
                });
                entries.push(ListEntry {
                    path: row.path.clone(),
                    ..entries[0].clone()
                });
            }
            facts.lists.push(Listed {
                repo: "/p".into(),
                job: i64::MAX,
                seen_at: "now".into(),
                result: ListResult {
                    entries,
                    trees: Vec::new(),
                },
            });
            let (breaches, statements) = count_statements(&tx, || check_i6(&tx, &facts).unwrap());
            assert_eq!(breaches, vec![]);
            statements
        };
        assert_eq!(count(3), count(300));
    }

    /// observe_runs, escalate_vanished and act over rows of each kind that
    /// they read and do not act on:
    /// - an `orphaned` row with an active rescue run;
    /// - a `landed` row with an active removal;
    /// - a `rescued` row whose idea is not merged;
    /// - a `rescued` row with no task, given up by a drop whose rescue run
    ///   did not save it;
    /// - a `created` row whose folder vanished, escalated already.
    #[test]
    fn observe_escalate_and_act_read_3_and_300_trees_of_each_kind_in_the_same_statements() {
        let count = |n: usize| {
            let (_tmp, mut store) = ledger();
            let tx = store.conn_mut().transaction().unwrap();
            let mut facts = Facts::default();
            let mut vanished = Vec::new();
            for i in 0..n {
                let o = tree(&tx, &format!("O{i:04}"), "orphaned", None);
                job(
                    &tx,
                    Kind::Rescue,
                    &o.worktree_id,
                    &rescue_key(&o.path, "h"),
                    "queued",
                );
                let l = tree(&tx, &format!("L{i:04}"), "landed", None);
                let key = remove_key(&l.path, false);
                job(&tx, Kind::WorktreeRemove, &l.worktree_id, &key, "queued");
                let t = format!("T{i:04}");
                task(&tx, &t, i + 1);
                tree(&tx, &format!("M{i:04}"), "rescued", Some(&t));
                let d = tree(&tx, &format!("D{i:04}"), "rescued", None);
                let rescue = job(&tx, Kind::Rescue, &d.worktree_id, "k", "passed");
                let ended = json!({ "keep_branch": false, "rescue_job": rescue }).to_string();
                note(&tx, &d.worktree_id, remove::OWNER_ENDED, &ended);
                let v = tree(&tx, &format!("V{i:04}"), "created", None);
                note(&tx, &v.worktree_id, rules::VANISHED_KIND, "{}");
                facts.folders.push(FolderFact {
                    key: v.worktree_id.clone(),
                    presence: Presence::Missing,
                });
                vanished.push(v.worktree_id);
            }
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let mut report = Report::default();
            let ((), statements) = count_statements(&tx, || {
                observe_runs(&ctx, &tx, &mut report).unwrap();
                escalate_vanished(&ctx, &tx, &facts, FleetSeconds(0), &mut report).unwrap();
                act(&ctx, &tx, FleetSeconds(0), &mut report).unwrap();
            });
            assert_eq!(
                report,
                Report {
                    vanished,
                    ..Report::default()
                }
            );
            statements
        };
        assert_eq!(count(3), count(300));
    }
}
