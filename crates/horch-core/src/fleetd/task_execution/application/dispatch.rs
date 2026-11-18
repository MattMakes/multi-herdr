//! The use case of a push dispatch and its bind (ticket 08).
//!
//! `docs/specs/fleetd/design.md`, "The task state machine" (`ready`,
//! `leased`), FR-5, FM-2, FM-14, "Holds on dispatch are visible" and I4.
//!
//! - [`bind`]: the 1 function that binds a ready task to a new attempt, in
//!   1 transaction, before any pane or process exists. The task goes
//!   `ready -> leased`, 1 new worker row of the persona and 1 attempt in
//!   `launching` with `via = push` are written, and the tree of the task
//!   is named: the tree of the last attempt, or a new `reserved` row with
//!   its `worktree_add` job. No lease and no start timeout start here:
//!   both start at the launch.
//! - [`choices`]: the choice of the rules of `domain/dispatch.rs` on the
//!   rows, with the holds of the caller and the check sandbox of each
//!   persona.
//! - [`dispatch_due`]: fleetd binds the chosen tasks of 1 pass, at most
//!   its work budget. A bind that fails rolls back only its own rows.
//! - [`write_task_files`] and [`write_missing_task_files`]: after the
//!   commit, `task.yaml` and `START.md` of an attempt. A crash between the
//!   commit and the files leaves an attempt in `launching` with no files,
//!   and the next pass writes them. The writer never rewrites a file.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::adapters::task_files::{
    attempt_dir, write_attempt_files, AttemptFiles,
};
use crate::fleetd::task_execution::domain::dispatch::{
    choose, Choice, Hold, Holds, ReadyTask, RunningTask,
};
use crate::fleetd::task_execution::domain::task::{attempt_id, AttemptRow, AttemptState};
use crate::fleetd::task_execution::domain::task_files::{Secret, TaskFacts};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{create_worktree, CreateOutcome, Lease};
use crate::taskledger::git::Git;
use crate::taskledger::model::{Actor, FileMode, TaskStatus, WorkerKind, WorktreeStatus};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::sql_words::{word, WORKTREES_GONE};
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::util;
use crate::taskledger::workers::{self, NewWorker};

/// The check sandbox of each persona (ticket 18). A persona whose check
/// sandbox is NotSupported on this host gets no dispatch (SR-4).
pub trait CheckSandboxes {
    fn supported(&self, persona: &str) -> bool;
}

/// The cause of the hold of a persona with no check sandbox.
pub const NO_CHECK_SANDBOX: &str =
    "no check sandbox on this host: its check cannot run in the worker sandbox (SR-4)";

/// The task states whose task holds its paths: a task bound or at work.
const HOLDING: [TaskStatus; 6] = [
    TaskStatus::Leased,
    TaskStatus::Running,
    TaskStatus::Stalled,
    TaskStatus::Verifying,
    TaskStatus::BlockedOnUser,
    TaskStatus::WaitingQuota,
];

/// What a bind and a dispatch read and write with.
pub struct DispatchCtx<'a> {
    /// The job context of the `worktree_add` job of a new tree.
    pub jobs: JobCtx<'a>,
    /// The fleet clock now.
    pub clock: FleetSeconds,
    /// The git of the base tip of a new tree.
    pub git: &'a Git,
}

/// The tree that a bind names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundTree {
    pub worktree_id: String,
    pub path: String,
    pub branch: String,
    /// The `worktree_add` job of a `reserved` row; `None` when the tree
    /// exists.
    pub add_job: Option<i64>,
}

/// 1 task bound to 1 new attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub task_id: String,
    pub attempt_id: String,
    /// The number of the attempt, `n` of `A<n>`.
    pub attempt: i64,
    pub worker_id: String,
    pub persona: String,
    /// `None` for a task of the shared checkout.
    pub worktree: Option<BoundTree>,
}

/// What 1 dispatch pass did.
#[derive(Debug, Default)]
pub struct Dispatched {
    pub bound: Vec<Bound>,
    /// Each ready task that a hold keeps `ready`, with its hold.
    pub held: Vec<(String, Hold)>,
    /// Each chosen task whose bind failed, with the error. Its rows did
    /// not change.
    pub failed: Vec<(String, String)>,
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Bind the ready task `task_id` to a new attempt of `persona`, in the open
/// transaction `tx`. `worker` gives the fields of the new worker row that
/// the caller chooses (workspace, predecessor, resumed session); the bind
/// sets its run, kind, persona and target task.
///
/// FM-14: the bind is a compare-and-set on `ready`, so 2 binds of 1 task
/// give 1 attempt. It refuses a task with an attempt that is not ended.
pub fn bind(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    task_id: &str,
    persona: &str,
    worker: &NewWorker,
) -> Result<Bound> {
    if tx.is_autocommit() {
        bail!("the bind writes its rows in 1 open transaction of the ledger");
    }
    let task = tasks::get(tx, task_id)?;
    if task.status != TaskStatus::Ready.as_str() {
        bail!(
            "{} is {}, not ready: 1 task binds to 1 attempt (FM-14)",
            task.short(),
            task.status
        );
    }
    if let Some(why) = tasks::cancel_requested(tx, task_id)? {
        bail!("{} waits for its cancel: {why}", task.short());
    }
    if let Some(open) = open_attempt(tx, task_id)? {
        bail!(
            "{} has the attempt {open} that is not ended: 1 task has 1 live attempt (I1)",
            task.short()
        );
    }
    let n = task.attempts + 1;
    let attempt = attempt_id(task_id, n);
    let text = format!("dispatch binds {} to attempt {attempt}", task.short());
    let leased = TaskStatus::Leased.as_str();
    let guard = ctx.jobs.guard;
    transition(
        tx,
        guard,
        Entity::Task,
        task_id,
        leased,
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    let worker_id = workers::insert(
        tx,
        &NewWorker {
            run_id: task.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: persona.to_string(),
            target_task_id: Some(task_id.to_string()),
            ..worker.clone()
        },
    )?;
    tx.execute(
        "UPDATE workers SET task_id = ?2, attempt = ?3 WHERE worker_id = ?1",
        params![worker_id, task_id, n],
    )?;
    tx.execute(
        &format!(
            "UPDATE tasks SET attempts = ?2, current_worker_id = ?3, updated_at = ?4
             WHERE task_id = ?1 AND status = {}",
            word(TaskStatus::Leased)
        ),
        params![task_id, n, worker_id, util::now()],
    )?;
    let row = AttemptRow {
        id: attempt.clone(),
        task_id: Some(task_id.to_string()),
        worker_id: worker_id.clone(),
        state: AttemptState::Launching,
        process: None,
        pane: None,
    };
    store::insert_attempt(tx, &row, &util::now())?;
    tx.execute("UPDATE attempts SET via = 'push' WHERE id = ?1", [&attempt])?;
    created(tx, guard, Entity::Attempt, &attempt, by_fleetd(&text))?;
    let worktree = if task.uses_worktree() {
        Some(bind_tree(tx, ctx, &task, &worker_id, &attempt)?)
    } else {
        None
    };
    Ok(Bound {
        task_id: task_id.to_string(),
        attempt_id: attempt,
        attempt: n,
        worker_id,
        persona: persona.to_string(),
        worktree,
    })
}

/// The id of an attempt of `task_id` that is not ended, if any.
fn open_attempt(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM attempts WHERE task_id = ?1 AND state <> 'ended' ORDER BY rowid LIMIT 1",
    )?;
    let mut rows = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(rows.next().transpose()?)
}

/// The tree of `task` that a new attempt takes: the row of an earlier
/// attempt that no drop or cleanup ended.
fn task_tree(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    Ok(trees::of_task(conn, task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status)))
}

/// Name the tree of the attempt `attempt` of `task`: the tree of the last
/// attempt moves to it, or a new `reserved` row with its `worktree_add`
/// job.
fn bind_tree(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    task: &Task,
    worker_id: &str,
    attempt: &str,
) -> Result<BoundTree> {
    let new = match task_tree(tx, &task.task_id)? {
        Some(_) => None,
        None => Some(new_tree(tx, ctx.git, task, worker_id)?),
    };
    let lease = Lease {
        task_id: Some(&task.task_id),
        worker_id,
        attempt: Some(attempt),
        from: None,
        new: new.as_ref(),
    };
    let (row, add_job) = match create_worktree(&ctx.jobs, tx, &lease, ctx.clock)? {
        CreateOutcome::Attached(row) => (row, None),
        CreateOutcome::Reserved { row, job } => (row, Some(job)),
        CreateOutcome::NeedsRescue(row) => bail!(
            "the worktree {} of {} is orphaned: its rescue comes first",
            row.worktree_id,
            task.short()
        ),
    };
    Ok(BoundTree {
        worktree_id: row.worktree_id,
        path: row.path,
        branch: row.branch,
        add_job,
    })
}

/// The base branch of the trees of `run`: the run branch, else the base.
fn base_branch(run: &Run) -> Result<String> {
    run.run_branch
        .clone()
        .or(run.base_branch.clone())
        .context("the run has no run branch: create runs in a git repository")
}

/// The row of a new tree of `task`, from the tip of the run branch.
fn new_tree(conn: &Connection, git: &Git, task: &Task, worker_id: &str) -> Result<NewWorktree> {
    let run = runs::get(conn, &task.run_id)?;
    let root = Path::new(&run.project_root);
    let base = base_branch(&run)?;
    let name = format!("T{}-{}", task.seq, slug(&task.title));
    // Beside the run branch, not under it: git cannot hold both `fleet/x`
    // and `fleet/x/T1-y`.
    let branch = format!("{}-{name}", run.run_branch.as_deref().unwrap_or("fleet"));
    let base_commit = git.rev(root, &base)?;
    Ok(NewWorktree {
        worktree_id: util::new_id(),
        task_id: Some(task.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        repo_root: run.project_root.clone(),
        path: root
            .join(".worktrees")
            .join(&name)
            .to_string_lossy()
            .into_owned(),
        branch,
        base,
        base_commit: Some(base_commit),
        status: WorktreeStatus::Open,
        mode: None,
    })
}

/// A branch-safe name of at most 40 characters.
fn slug(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(40).collect()
}

// ─── the choice ─────────────────────────────────────────────────────────

/// The ids of the tasks in the states `states`, in seq order of each run.
fn task_ids(conn: &Connection, states: &[TaskStatus]) -> Result<Vec<String>> {
    let words: Vec<String> = states.iter().map(|s| word(*s)).collect();
    let sql = format!(
        "SELECT task_id FROM tasks WHERE status IN ({}) ORDER BY run_id, seq",
        words.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// The project of each run, read once.
struct Projects<'c> {
    conn: &'c Connection,
    by_run: BTreeMap<String, String>,
}

impl Projects<'_> {
    fn of(&mut self, run_id: &str) -> Result<String> {
        if let Some(p) = self.by_run.get(run_id) {
            return Ok(p.clone());
        }
        let project = runs::get(self.conn, run_id)?.project_id;
        self.by_run.insert(run_id.to_string(), project.clone());
        Ok(project)
    }
}

/// The persona of the next attempt of `t`: `claimable_by`, else its
/// primary.
fn persona_of(t: &Task) -> String {
    t.claimable_by
        .clone()
        .or(t.primary_persona.clone())
        .unwrap_or_default()
}

fn ready_task(
    conn: &Connection,
    projects: &mut Projects<'_>,
    t: Task,
    target: Option<&str>,
) -> Result<ReadyTask> {
    let tree_orphaned =
        task_tree(conn, &t.task_id)?.is_some_and(|r| r.state == WorktreeState::Orphaned);
    Ok(ReadyTask {
        short: t.short(),
        project: projects.of(&t.run_id)?,
        persona: persona_of(&t),
        target: target == Some(t.task_id.as_str()),
        shared_checkout: !t.uses_worktree(),
        review: t.is_review(),
        own: tasks::files(conn, &t.task_id, FileMode::Own)?,
        open_attempt: open_attempt(conn, &t.task_id)?,
        tree_orphaned,
        priority: t.priority,
        seq: t.seq,
        not_before: t.not_before,
        parallel: t.parallel,
        run_id: t.run_id,
        task_id: t.task_id,
    })
}

fn running_task(conn: &Connection, t: Task) -> Result<RunningTask> {
    Ok(RunningTask {
        short: t.short(),
        shared_checkout: !t.uses_worktree(),
        review: t.is_review(),
        own: tasks::files(conn, &t.task_id, FileMode::Own)?,
        parallel: t.parallel,
        run_id: t.run_id,
    })
}

/// The choice for each ready task now, in dispatch order. `holds` are the
/// holds of the caller; each persona of a ready task whose check sandbox
/// `sandboxes` does not support is held too. `target` names the task that
/// goes first: the target of a spawn, or the task of the worker that a
/// replacement replaces (L2).
pub fn choices(
    conn: &Connection,
    holds: &Holds,
    sandboxes: &dyn CheckSandboxes,
    target: Option<&str>,
) -> Result<Vec<Choice>> {
    let mut projects = Projects {
        conn,
        by_run: BTreeMap::new(),
    };
    let mut ready = Vec::new();
    for id in task_ids(conn, &[TaskStatus::Ready])? {
        // Its cancel waits for the stop of an attempt: it binds no more.
        if tasks::cancel_requested(conn, &id)?.is_some() {
            continue;
        }
        let t = tasks::get(conn, &id)?;
        ready.push(ready_task(conn, &mut projects, t, target)?);
    }
    let mut running = Vec::new();
    for id in task_ids(conn, &HOLDING)? {
        running.push(running_task(conn, tasks::get(conn, &id)?)?);
    }
    let mut holds = holds.clone();
    for t in &ready {
        if !holds.personas.contains_key(&t.persona) && !sandboxes.supported(&t.persona) {
            holds
                .personas
                .insert(t.persona.clone(), NO_CHECK_SANDBOX.to_string());
        }
    }
    Ok(choose(&ready, &running, &holds))
}

/// 1 dispatch pass of fleetd, in the open transaction `tx`: bind each
/// chosen task, at most `budget` of them. Each bind runs in its own
/// savepoint, so a bind that fails rolls back only its own rows and the
/// pass goes on. After the commit, the caller writes the task files of
/// each bound attempt ([`write_task_files`]).
pub fn dispatch_due(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    holds: &Holds,
    sandboxes: &dyn CheckSandboxes,
    budget: usize,
) -> Result<Dispatched> {
    if tx.is_autocommit() {
        bail!("the dispatch writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Dispatched::default();
    for choice in choices(tx, holds, sandboxes, None)? {
        match choice {
            Choice::Held { task_id, hold } => out.held.push((task_id, hold)),
            Choice::Bind { .. } if out.bound.len() >= budget => {}
            Choice::Bind { task_id, persona } => {
                let worker = dispatched_worker(tx, &task_id)?;
                tx.execute_batch("SAVEPOINT dispatch_bind")?;
                match bind(tx, ctx, &task_id, &persona, &worker) {
                    Ok(b) => {
                        tx.execute_batch("RELEASE dispatch_bind")?;
                        out.bound.push(b);
                    }
                    Err(e) => {
                        tx.execute_batch("ROLLBACK TO dispatch_bind; RELEASE dispatch_bind")?;
                        out.failed.push((task_id, format!("{e:#}")));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The worker row of a dispatch by fleetd: its pane goes to the workspace
/// of the run.
fn dispatched_worker(conn: &Connection, task_id: &str) -> Result<NewWorker> {
    let task = tasks::get(conn, task_id)?;
    let run = runs::get(conn, &task.run_id)?;
    Ok(NewWorker {
        workspace_id: run.workspace_id,
        ..NewWorker::default()
    })
}

// ─── the task files ─────────────────────────────────────────────────────

/// The facts of the files of the attempt `attempt_id`, from its rows.
fn facts(conn: &Connection, attempt_id: &str) -> Result<TaskFacts> {
    let (task_id, worker_id): (Option<String>, String) = conn
        .query_row(
            "SELECT task_id, worker_id FROM attempts WHERE id = ?1",
            [attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .with_context(|| format!("no attempt {attempt_id}"))?;
    let task_id = task_id.with_context(|| format!("the attempt {attempt_id} has no task"))?;
    let task = tasks::get(conn, &task_id)?;
    let run = runs::get(conn, &task.run_id)?;
    let worker = workers::get(conn, &worker_id)?;
    let n = worker
        .attempt
        .with_context(|| format!("the worker of attempt {attempt_id} has no attempt number"))?;
    let (worktree, branch) = match task_tree(conn, &task_id)?.filter(|_| task.uses_worktree()) {
        Some(row) => (row.path, row.branch),
        None => (run.project_root.clone(), base_branch(&run)?),
    };
    Ok(TaskFacts {
        task: format!("T{}", task.seq),
        task_id,
        run: run.slug,
        attempt: format!("A{n}"),
        profile: worker.persona.unwrap_or_default(),
        worktree,
        branch,
        read_first: task.task_file.unwrap_or_default(),
        goal: task.title,
        check: task.land_check,
        kind: task.work_kind,
        ticket_snapshot: task.ticket_snapshot.unwrap_or_default(),
        extra: None,
    })
}

/// Write `task.yaml` and `START.md` of the attempt `attempt_id` under
/// `fleet_root`, after the commit of its bind. `secrets` are the values
/// that the SR-2 lint refuses. A file that exists with the same bytes is
/// accepted, so a second write changes nothing.
pub fn write_task_files(
    conn: &Connection,
    fleet_root: &Path,
    attempt_id: &str,
    secrets: &[Secret],
) -> Result<AttemptFiles> {
    let facts = facts(conn, attempt_id)?;
    Ok(write_attempt_files(fleet_root, &facts, secrets)?)
}

/// Write the 2 files of each attempt of a push dispatch in `launching`
/// that has no `task.yaml`: a crash came between its commit and its files.
/// Gives each such attempt with the result of its write.
pub fn write_missing_task_files(
    conn: &Connection,
    fleet_root: &Path,
    secrets: &[Secret],
) -> Result<Vec<(String, Result<AttemptFiles>)>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM attempts WHERE via = 'push' AND state = 'launching' ORDER BY rowid",
    )?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for id in ids {
        let facts = facts(conn, &id)?;
        let dir = attempt_dir(fleet_root, &facts.task_id, &facts.attempt);
        if dir.join("task.yaml").exists() {
            continue;
        }
        let written = write_attempt_files(fleet_root, &facts, secrets).map_err(Into::into);
        out.push((id, written));
    }
    Ok(out)
}
