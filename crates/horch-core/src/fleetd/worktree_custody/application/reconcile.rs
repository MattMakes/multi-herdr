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
//! for each `landed` tree. The jobs run outside the pass, and the guard of
//! the remove job checks each tree again at its removal.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::adapters::scans::{FolderFact, Presence};
use crate::fleetd::fleet_runtime::adapters::sqlite::{has_fact, state_of};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{
    record, transition, Cause, DoorError, Moved,
};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::git::{worktree_add_key, ListResult};
use crate::fleetd::worktree_custody::adapters::rescue_git::{rescue_key, Rescue};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::{create, remove, rescue};
use crate::fleetd::worktree_custody::domain::removal::quarantine_path;
use crate::fleetd::worktree_custody::domain::worktree::{
    self as rules, Attempt, Breach, Folder, Owner, OwnerGone, RunBranch, Seen, TreeJob,
};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerStatus, WorktreeStatus};
use crate::taskledger::{runs, tasks, util, workers};

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
    /// Moved to `created`: the task is parked by `done --unlanded`.
    pub created: Vec<String>,
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
/// landed tree. Each step reads the rows that the step before it wrote.
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
    escalate_vanished(ctx, tx, facts, clock, &mut report)?;
    act(ctx, tx, clock, &mut report)?;
    Ok(report)
}

/// The breaches of I6 that `facts` show, in `conn`. A breach is reported;
/// the rule that repairs it acts on the next pass.
pub fn check_i6(conn: &Connection, facts: &Facts) -> Result<Vec<Breach>> {
    let rows = store::in_states(conn, &WorktreeState::ALL)?;
    let live: Vec<&WorktreeRow> = rows
        .iter()
        .filter(|r| r.state != WorktreeState::Removed)
        .collect();
    let known = known_paths(live.iter().copied(), facts);
    let seen = seen(conn, &rows, facts)?;
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
        if rules::vanished(state, folder(facts, &id), false, set_aside(conn, facts, r)?) {
            out.push(Breach::NoFolder { id });
        } else if r.state == WorktreeState::Reserved && add_run(conn, r)?.is_none() {
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

fn add_run(conn: &Connection, row: &WorktreeRow) -> Result<Option<jobs_store::JobRow>> {
    let key = worktree_add_key(row);
    jobs_store::latest(conn, Kind::WorktreeAdd, &row.worktree_id, &key)
}

/// Observe the last run of the job that each row waits for, when it
/// ended `passed` or `failed`: the observations of `create`, `rescue` and
/// `remove`. A rescue run is the one of the HEAD of the row. A passed
/// rescue whose ref the row has already is the rescue of an earlier orphan
/// of a reused tree: it does not save the tree now, so it is not observed.
fn observe_runs(ctx: &JobCtx<'_>, tx: &Connection, report: &mut Report) -> Result<()> {
    for row in all_rows(tx)? {
        let id = row.worktree_id.as_str();
        let Some(awaited) = rules::awaited(row.state.as_str()) else {
            continue;
        };
        let run = match awaited {
            TreeJob::Add => add_run(tx, &row)?,
            TreeJob::Rescue => match row.head_sha.as_deref() {
                Some(head) => {
                    let key = rescue_key(&row.path, head);
                    jobs_store::latest(tx, Kind::Rescue, id, &key)?
                }
                None => None,
            },
            TreeJob::Remove => remove::latest_removal(tx, id, &row.path)?,
        };
        let Some(job) = run.filter(|j| matches!(j.state, State::Passed | State::Failed)) else {
            continue;
        };
        if awaited == TreeJob::Rescue && applied(&job, &row) {
            continue;
        }
        match awaited {
            TreeJob::Add => {
                create::observe(ctx, tx, &job)?;
            }
            TreeJob::Rescue => {
                rescue::observe(ctx, tx, &job)?;
            }
            TreeJob::Remove => {
                remove::observe(ctx, tx, &job)?;
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
fn seen(conn: &Connection, rows: &[WorktreeRow], facts: &Facts) -> Result<Vec<Seen>> {
    let mut out = Vec::new();
    for l in &facts.lists {
        for (n, e) in l.result.entries.iter().enumerate() {
            let main = n == 0 || e.bare;
            if !main && !admits(conn, rows, facts, l, &e.path)? {
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
    Ok(out)
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
fn admits(
    conn: &Connection,
    rows: &[WorktreeRow],
    facts: &Facts,
    list: &Listed,
    path: &str,
) -> Result<bool> {
    let shown = match evidence(facts, rows, list, path) {
        Evidence::RowScan(f) => f == Folder::Present,
        Evidence::WorkDirScan { listed } => listed,
        Evidence::ListOnly => true,
    };
    Ok(shown && !stale(conn, rows, list, path)?)
}

/// Whether a passed removal of the folder `path` was queued after the run
/// of `list`: the run may have read the tree before it went. The order is
/// the order of the ids of the runs, which cannot tie, as 2 times of 1 s
/// can. A list run queued after the removal but started before its end is
/// not stale by this rule: the scan of [`evidence`] shows that the folder
/// is gone.
fn stale(conn: &Connection, rows: &[WorktreeRow], list: &Listed, path: &str) -> Result<bool> {
    for row in rows.iter().filter(|r| r.path == path) {
        let Some(job) = jobs_store::last(conn, Kind::WorktreeRemove, &row.worktree_id)? else {
            continue;
        };
        if job.state == State::Passed && job.id > list.job {
            return Ok(true);
        }
    }
    Ok(false)
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
    let rows = all_rows(tx)?;
    let live = rows.iter().filter(|r| r.state != WorktreeState::Removed);
    let known = known_paths(live, facts);
    report.not_trees = plain_folders(&known, facts);
    let seen = seen(tx, &rows, facts)?;
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

/// The owner of the `active` row `row`, as the ledger gives it.
fn owner(tx: &Connection, row: &WorktreeRow) -> Result<Owner> {
    let attempt = match row.owner_attempt.as_deref() {
        Some(a) => Some(match state_of(tx, Entity::Attempt, a)?.as_deref() {
            None => Attempt::Missing,
            Some("ended") => Attempt::Ended,
            Some(_) => Attempt::Holds,
        }),
        None => None,
    };
    let worker_live = match row.worker_id.as_deref() {
        Some(w) => workers::find(tx, w)?
            .is_some_and(|w| WorkerStatus::parse(&w.status).is_ok_and(WorkerStatus::is_live)),
        None => false,
    };
    let task = match row.task_id.as_deref() {
        Some(t) => Some(tasks::get(tx, t)?),
        None => None,
    };
    Ok(Owner {
        attempt,
        worker_live,
        task_parked: task.as_ref().is_some_and(|t| t.is_parked()),
        task_cancelled: task
            .as_ref()
            .is_some_and(|t| t.status == TaskStatus::Cancelled.as_str()),
    })
}

/// Move each `active` row whose owner is gone: `created` for a parked
/// task, else `orphaned` in this pass (FM-25, Q10, Q23).
fn owners(ctx: &JobCtx<'_>, tx: &Connection, report: &mut Report) -> Result<()> {
    for row in store::in_states(tx, &[WorktreeState::Active])? {
        let id = row.worktree_id.as_str();
        let (to, cause, list) = match rules::owner_gone(&owner(tx, &row)?) {
            None => continue,
            Some(OwnerGone::Created) => (
                "created",
                "its task is parked by done --unlanded",
                &mut report.created,
            ),
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
fn set_aside(conn: &Connection, facts: &Facts, row: &WorktreeRow) -> Result<bool> {
    let quarantine = quarantine_path(Path::new(&row.path), &row.worktree_id);
    let quarantine = quarantine.display().to_string();
    let in_work_dir = facts
        .work_dirs
        .iter()
        .filter(|d| d.repo == row.repo_root)
        .any(|d| d.folders.contains(&quarantine));
    Ok(in_work_dir || jobs_store::active(conn, Kind::WorktreeRemove, &row.worktree_id)?.is_some())
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
    for row in all_rows(tx)? {
        let id = row.worktree_id.as_str();
        let gone = rules::vanished(
            row.state.as_str(),
            folder(facts, id),
            in_git(facts, &row),
            set_aside(tx, facts, &row)?,
        );
        if !gone {
            continue;
        }
        report.vanished.push(id.to_string());
        if has_fact(
            tx,
            Entity::Worktree,
            id,
            EventKind::Note,
            rules::VANISHED_KIND,
        )? {
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

/// Queue 1 `rescue` run of each `orphaned` row with no rescue ref, and 1
/// `worktree_remove` run of each `landed` row (FM-25, FM-27, Q8, Q9). A row
/// whose folder vanished is escalated, not queued. A row with an active run
/// of that kind is not queued again.
fn act(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds, report: &mut Report) -> Result<()> {
    for row in all_rows(tx)? {
        let id = row.worktree_id.as_str();
        if report.vanished.iter().any(|v| v == id) {
            continue;
        }
        match rules::to_queue(row.state.as_str()) {
            Some(TreeJob::Rescue) => {
                if queue_rescue(ctx, tx, &row, clock)? {
                    report.rescues.push(id.to_string());
                }
            }
            Some(TreeJob::Remove) => {
                if remove::queue_auto_remove(ctx, tx, &row, "its tree is landed", clock)? {
                    report.removals.push(id.to_string());
                }
            }
            Some(TreeJob::Add) | None => {}
        }
    }
    Ok(())
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
    let last = jobs_store::latest(tx, Kind::Rescue, &row.worktree_id, &input_key)?;
    if last.is_some_and(|j| j.state == State::Failed) {
        return Ok(false);
    }
    let queued = jobs::queue(ctx, tx, &key, Some(&row.repo_root), clock)?;
    Ok(matches!(queued, Queued::New(_)))
}
