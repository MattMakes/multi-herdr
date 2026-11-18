//! The reserve, the observation, and the attach of a worktree to a new owner.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" (rows `reserved` and `created`) and "Supervision, liveness and
//! reconciliation" ("the row first, then the effect, then the
//! observation"); FR-11; `ddd/08-code/worktree-custody/design.md` §3
//! (`CreateWorktree`) and §6 (`worktree-created`, `task-leased`).
//!
//! A tree is a ledger row before it exists. [`create_worktree`] writes the
//! row in `reserved` and queues its `worktree_add` job in 1 transaction.
//! The job runs `git worktree add` with no transaction open. [`observe`]
//! reads the ended run in 1 transaction: the pass and a command both call
//! it. Until Phase 4 the worker commands run the job themselves
//! ([`create_and_wait`], MAP.md of fleetd-p3, Q1 and Q17).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::config_file;
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::sqlite::{drop_reserved, state_of};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worktree_custody::adapters::executors;
use crate::fleetd::worktree_custody::adapters::git::{
    is_gate, worktree_add_key, BranchRefused, CliWorktreeGit, NoSandbox,
};
use crate::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use crate::fleetd::worktree_custody::adapters::run_now::{run_now, Starter};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::ports::WorktreeGitGateway;
use crate::fleetd::worktree_custody::application::{remove, rescue};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::paths::nonempty_path;
use crate::taskledger::model::{Actor, WorkerStatus};
use crate::taskledger::sql_words::WORKTREES_GONE;
use crate::taskledger::workers;

// ─── the request ────────────────────────────────────────────────────────

/// 1 request for the tree of a task, or for a scratch tree.
#[derive(Debug, Clone)]
pub struct Lease<'a> {
    /// The task of the tree; `None` for a scratch tree (row W4).
    pub task_id: Option<&'a str>,
    pub worker_id: &'a str,
    /// The `live` attempt of the worker; `None` when the worker has none
    /// (MAP.md of fleetd-p3, Q23).
    pub attempt: Option<&'a str>,
    /// The base that the request names (`--from`). An existing tree of
    /// the task on another base is refused.
    pub from: Option<&'a str>,
    /// The new row, when the task has no tree. `None` when the command
    /// found a tree of the task first.
    pub new: Option<&'a NewWorktree>,
}

/// What [`create_worktree`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// The tree of the task, now of the worker.
    Attached(WorktreeRow),
    /// The tree of the task is `orphaned`: rescue it, then attach it.
    NeedsRescue(WorktreeRow),
    /// The row is `reserved`; `job` is its `worktree_add` run.
    Reserved { row: WorktreeRow, job: i64 },
}

/// What [`attach`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attach {
    /// The row has the new owner.
    Attached(WorktreeRow),
    /// The row is `orphaned` and did not change: its rescue comes first.
    NeedsRescue(WorktreeRow),
}

/// The event of a made tree (`contract: worktree-created`): exactly these
/// 4 facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCreated {
    pub worktree_id: String,
    /// `None` for a scratch tree.
    pub task_id: Option<String>,
    pub path: String,
    pub branch: String,
}

/// What [`observe`] did with the row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The row moved from `reserved` to `created`, then to `active`.
    Created(WorktreeCreated),
    /// The run failed, so the `reserved` row is gone. The text is the
    /// reason of the run.
    Dropped(String),
    /// The row stays as it was, for this reason.
    Kept(String),
}

// ─── the use cases ──────────────────────────────────────────────────────

/// The cause of a write of a worker command.
fn by_worker(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Worker,
        text,
    }
}

/// The tree of `lease`, in `tx`, at `clock`:
/// - The task has a row that is not `removed` (row W2): no new row. The
///   row moves to the worker ([`attach`]). A `reserved` row gives its
///   active `worktree_add` run.
/// - Else 1 row in `reserved`, owned by the attempt of the request, and 1
///   `worktree_add` run in `queued`.
///
/// A second call with the same request finds the row and its run, so it
/// has the effect of 1 (`idempotent: task-leased`).
pub fn create_worktree(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    lease: &Lease<'_>,
    clock: FleetSeconds,
) -> Result<CreateOutcome> {
    if let Some(row) = task_tree(tx, lease.task_id)? {
        if lease.from.is_some_and(|f| f != row.base) {
            bail!(
                "existing worktree base is {}; drop it before changing base",
                row.base
            );
        }
        return match attach(tx, &row, lease.worker_id, lease.attempt)? {
            Attach::Attached(row) if row.state == WorktreeState::Reserved => {
                reserved(ctx, tx, row, clock)
            }
            Attach::Attached(row) => Ok(CreateOutcome::Attached(row)),
            Attach::NeedsRescue(row) => Ok(CreateOutcome::NeedsRescue(row)),
        };
    }
    let new = lease
        .new
        .context("the task has no worktree now; run the command again")?;
    let cause = by_worker("the worker asked for a worktree");
    store::insert_reserved(tx, ctx.guard, new, cause)?;
    store::set_owner(tx, &new.worktree_id, lease.attempt)?;
    let row = store::get(tx, &new.worktree_id)?.context("the reserved row is gone")?;
    reserved(ctx, tx, row, clock)
}

/// The row of `task_id` that is not `removed` and that no drop or cleanup
/// ended; `None` for a scratch request.
fn task_tree(tx: &Connection, task_id: Option<&str>) -> Result<Option<WorktreeRow>> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    Ok(store::of_task(tx, task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status)))
}

/// The `worktree_add` run of the `reserved` row `row`: its active run, or
/// a new one.
fn reserved(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: WorktreeRow,
    clock: FleetSeconds,
) -> Result<CreateOutcome> {
    let job = add_run(ctx, tx, &row, clock)?;
    Ok(CreateOutcome::Reserved { row, job })
}

/// The id of the `worktree_add` run of the `reserved` row `row` to wait
/// for ([`queued_run`]).
fn add_run(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: &WorktreeRow,
    clock: FleetSeconds,
) -> Result<i64> {
    let input_key = worktree_add_key(row);
    let key = JobKey {
        kind: Kind::WorktreeAdd,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    queued_run(ctx, tx, &key, &row.repo_root, clock)
}

/// The id of the run of `key` to wait for: a new run, the active run, or
/// the run whose result is used again. A held key is an error.
fn queued_run(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    key: &JobKey<'_>,
    repo: &str,
    clock: FleetSeconds,
) -> Result<i64> {
    match jobs::queue(ctx, tx, key, Some(repo), clock)? {
        Queued::New(id) => Ok(id),
        Queued::Active(r) | Queued::Reused(r) => Ok(r.id),
        Queued::Held(r) => bail!(
            "the {} job of {} ended abandoned {} times; run horch job requeue {} after you read why",
            key.kind,
            key.subject,
            r.abandoned_runs,
            r.id
        ),
    }
}

/// Move the row `row` to the worker `worker` and its attempt `attempt`, in
/// `tx`:
/// - `created` or `rescued`: the row moves to `active`.
/// - `reserved`, `active`, `landing` or `landed`: only the owner changes.
/// - `orphaned`: no change; [`Attach::NeedsRescue`].
///
/// It refuses a row that another live attempt owns (FM-14), and a row
/// whose removal can run ([`refuse_while_removal_active`]).
///
/// The claim calls it too (ticket 11 of fleetd-p3).
pub fn attach(
    tx: &Connection,
    row: &WorktreeRow,
    worker: &str,
    attempt: Option<&str>,
) -> Result<Attach> {
    let id = row.worktree_id.as_str();
    match row.state {
        WorktreeState::Orphaned => return Ok(Attach::NeedsRescue(row.clone())),
        WorktreeState::Removed => bail!("worktree {id} is removed"),
        _ => {}
    }
    refuse_live_owner(tx, row, attempt)?;
    refuse_while_removal_active(tx, row)?;
    tx.execute(
        "UPDATE worktrees SET worker_id = ?2 WHERE worktree_id = ?1",
        params![id, worker],
    )?;
    store::set_owner(tx, id, attempt)?;
    if matches!(row.state, WorktreeState::Created | WorktreeState::Rescued) {
        let refusals: &dyn RecordRefusal = &NeedsYouList;
        let cause = by_worker("a new attempt of its task takes it");
        transition(tx, None, Entity::Worktree, id, "active", cause, refusals)?;
    }
    let row = store::get(tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    Ok(Attach::Attached(row))
}

/// Refuse a new owner of `row` while its owner attempt holds it: the
/// attempt is not `attempt`, is not ended, and the worker of the row is
/// live (FM-14). An attempt whose worker row ended (a handoff, a reap) is
/// ending: the pass reaps it, and it holds no tree.
fn refuse_live_owner(tx: &Connection, row: &WorktreeRow, attempt: Option<&str>) -> Result<()> {
    let Some(owner) = row.owner_attempt.as_deref().filter(|o| Some(*o) != attempt) else {
        return Ok(());
    };
    let not_ended =
        state_of(tx, Entity::Attempt, owner)?.is_some_and(|s| s != AttemptState::Ended.as_str());
    let worker_live = match row.worker_id.as_deref() {
        Some(w) => workers::find(tx, w)?
            .is_some_and(|w| WorkerStatus::parse(&w.status).is_ok_and(WorkerStatus::is_live)),
        None => false,
    };
    if not_ended && worker_live {
        bail!(
            "worktree {} is held by the live attempt {owner}; a new attempt takes it only after that attempt ends (FM-14)",
            row.worktree_id
        );
    }
    Ok(())
}

/// Refuse a new owner of `row` while its `worktree_remove` run is `queued`
/// or `running`, or `passed` before a pass observed it. A claim and a
/// removal never both pass: `queue_remove` refuses a row that is not
/// `landed` or `rescued`, and the writer lock of SQLite orders the 2
/// transactions.
fn refuse_while_removal_active(tx: &Connection, row: &WorktreeRow) -> Result<()> {
    let id = row.worktree_id.as_str();
    let Some(run) = remove::latest_removal(tx, id, &row.path)? else {
        return Ok(());
    };
    if matches!(run.state, State::Queued | State::Running | State::Passed) {
        bail!(
            "worktree {id} has the worktree_remove job {} in {}; it takes no new owner until a pass ends that job. Run the command again after that",
            run.id,
            run.state.as_str()
        );
    }
    Ok(())
}

/// The owner of `row` gives it up, in `tx`: the row has no worker and no
/// owner attempt. An `active` row moves to `created` (the design row
/// `active`: retried, back to `created`), so the next attempt of its task
/// takes it. Each other state stays. Handoff calls it (ticket 11 of
/// fleetd-p3).
pub fn release(tx: &Connection, row: &WorktreeRow, why: &str) -> Result<()> {
    let id = row.worktree_id.as_str();
    tx.execute(
        "UPDATE worktrees SET worker_id = NULL WHERE worktree_id = ?1",
        [id],
    )?;
    store::set_owner(tx, id, None)?;
    if row.state == WorktreeState::Active {
        let refusals: &dyn RecordRefusal = &NeedsYouList;
        transition(
            tx,
            None,
            Entity::Worktree,
            id,
            "created",
            by_worker(why),
            refusals,
        )?;
    }
    Ok(())
}

/// Observe the `worktree_add` run `job`, in `tx`:
/// - `passed`, and the folder of the `reserved` row exists: the row moves
///   to `created`, then to `active` with the owner of the request. The
///   gate tree of a feature has no owner: it stays `created`.
/// - `failed`, or `passed` with no folder: the `reserved` row is
///   dropped, with the cause `worktree_add_failed`. A run that refused a
///   branch that another tree holds also opens 1 needs-you item.
/// - `abandoned`, `queued` or `running`: the row stays `reserved`. The
///   rule of abandoned runs gives the next run.
///
/// A row that is no longer `reserved` stays as it is.
pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow) -> Result<Observed> {
    if job.kind != Kind::WorktreeAdd {
        bail!(
            "job {} is a {} job, not a worktree_add job",
            job.id,
            job.kind
        );
    }
    let id = job.subject.as_str();
    let Some(row) = store::get(tx, id)? else {
        return Ok(Observed::Kept(format!("no worktree {id} in the ledger")));
    };
    if row.state != WorktreeState::Reserved {
        return Ok(Observed::Kept(format!("worktree {id} is {}", row.state)));
    }
    let reason = outcome(job).map(|o| o.text).unwrap_or_default();
    match job.state {
        State::Passed if Path::new(&row.path).is_dir() => created(ctx, tx, row),
        // The effect did not hold: a kept row would wait for a folder that
        // no run makes again, as the next run keeps the branch of this run
        // and fails (review F1). The branch stays for the reconciler.
        State::Passed => {
            drop_reserved(tx, ctx.guard, id, "worktree_add_failed", Actor::Horch)?;
            Ok(Observed::Dropped(format!(
                "the worktree_add job {} passed, but {} does not exist",
                job.id, row.path
            )))
        }
        State::Failed => {
            if let Some(refused) = branch_refused(job) {
                open_branch_refused(ctx, tx, id, &refused, &reason)?;
            }
            if no_sandbox(job).is_some() {
                open_no_sandbox(ctx, tx, Kind::WorktreeAdd, id, &reason)?;
            }
            drop_reserved(tx, ctx.guard, id, "worktree_add_failed", Actor::Horch)?;
            Ok(Observed::Dropped(reason))
        }
        State::Abandoned => Ok(Observed::Kept(format!(
            "the worktree_add job {} ended abandoned: its process is gone with no result",
            job.id
        ))),
        State::Queued | State::Running => Ok(Observed::Kept(format!(
            "the worktree_add job {} is {}",
            job.id,
            job.state.as_str()
        ))),
    }
}

/// The kind of the needs-you item of a job kind that ran no git because the
/// host has no sandbox.
pub const GIT_SANDBOX_KIND: &str = "git_sandbox";

/// Open the needs-you item of the job kind `kind` that ran no git because
/// the host has no sandbox: 1 item for each kind, not 1 for each job. The
/// entity is `id`, the subject of the first such job.
pub fn open_no_sandbox(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    kind: Kind,
    id: &str,
    reason: &str,
) -> Result<()> {
    let key = format!("{GIT_SANDBOX_KIND}:{kind}");
    let text = format!(
        "{reason}. Run fleetd on a host with a sandbox (macOS, or Linux with \
         bubblewrap on the PATH), then ask again"
    );
    let item = NewItem {
        key: &key,
        kind: GIT_SANDBOX_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

/// The kind of the needs-you item of a branch that a `worktree_add` run
/// refused.
pub const BRANCH_REFUSED_KIND: &str = "worktree_branch_refused";

/// The branch that the failed run `job` refused.
fn branch_refused(job: &JobRow) -> Option<BranchRefused> {
    serde_json::from_value(result_data(job)?).ok()
}

/// The reason of the failed run `job` that ran no git because the host has
/// no sandbox.
fn no_sandbox(job: &JobRow) -> Option<NoSandbox> {
    serde_json::from_value(result_data(job)?).ok()
}

/// Open the needs-you item of the refused branch: the operator frees the
/// branch or moves its work, then the same request attaches or makes it.
fn open_branch_refused(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    id: &str,
    refused: &BranchRefused,
    reason: &str,
) -> Result<()> {
    let key = format!("{BRANCH_REFUSED_KIND}:{}", refused.branch);
    let fix = match &refused.held_by {
        Some(tree) => {
            format!("Commit or move the work of {tree}, then run git worktree remove there")
        }
        None => format!(
            "Keep the work of {} on another branch, then delete it",
            refused.branch
        ),
    };
    let text = format!("{reason}. {fix}, and ask for the worktree again");
    let item = NewItem {
        key: &key,
        kind: BRANCH_REFUSED_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

fn created(ctx: &JobCtx<'_>, tx: &Connection, row: WorktreeRow) -> Result<Observed> {
    let id = row.worktree_id.as_str();
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    let made = Cause {
        actor: Actor::Horch,
        text: "its worktree_add job passed and its folder exists",
    };
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "created",
        made,
        refusals,
    )?;
    // The gate tree of a feature has no worker and no owner attempt: fleetd
    // owns it, and it stays `created`, which the reconciler does not orphan
    // (ticket 28 of fleetd-p4, Q18).
    if is_gate(&row) && row.worker_id.is_none() && row.owner_attempt.is_none() {
        return Ok(Observed::Created(WorktreeCreated {
            worktree_id: row.worktree_id,
            task_id: row.task_id,
            path: row.path,
            branch: row.branch,
        }));
    }
    let owned = Cause {
        actor: Actor::Horch,
        text: "the owner of the request takes it",
    };
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "active",
        owned,
        refusals,
    )?;
    Ok(Observed::Created(WorktreeCreated {
        worktree_id: row.worktree_id,
        task_id: row.task_id,
        path: row.path,
        branch: row.branch,
    }))
}

// ─── the command ────────────────────────────────────────────────────────

/// What runs a job to its end for a command, with no transaction open.
pub trait RunJob {
    /// Run the job `job_id` to its end, and give its ended row. A run that
    /// ended already is given as it is.
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow>;
}

/// The job process `horch job run <id>`, started through the launcher by
/// [`run_now`].
#[derive(Debug, Clone)]
pub struct JobProcess {
    horch: PathBuf,
}

impl JobProcess {
    /// The job process of the `horch` binary `horch`.
    pub fn new(horch: impl Into<PathBuf>) -> JobProcess {
        JobProcess {
            horch: horch.into(),
        }
    }

    /// The job process of this binary. This process must be `horch`: a
    /// `horch` from PATH is never used.
    pub fn of_this_process() -> Result<JobProcess> {
        let exe = ProcessEnv
            .current_exe()
            .context("cannot read the path of this process")?;
        if exe.file_stem().and_then(|s| s.to_str()) != Some("horch") {
            bail!(
                "{} is not horch, so it cannot start `horch job run`: run the horch command, \
                 or pass a RunJob to the _with function",
                exe.display()
            );
        }
        Ok(JobProcess::new(exe))
    }
}

impl RunJob for JobProcess {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow> {
        let starter = Starter {
            ctx,
            host: &Launcher,
            horch: &self.horch,
            output: None,
        };
        run_now(conn, job_id, &starter)
    }
}

/// The config of the fleet of the ledger `ledger`: `fleet.toml` in
/// `$FLEET_ROOT`, else in the folder `fleet` beside the ledger file (the
/// rule of `Paths`).
pub fn fleet_config(ledger: &Path) -> Result<FleetConfig> {
    let root = nonempty_path(&ProcessEnv, "FLEET_ROOT")
        .unwrap_or_else(|| ledger.parent().unwrap_or(Path::new("")).join("fleet"));
    config_file::load(&root)
}

/// The tree of `lease`, made or taken, from a worker command. It gives
/// the id of the row, `active` and owned by the attempt of the request:
/// 1. 1 transaction: [`create_worktree`].
/// 2. An `orphaned` tree: its `rescue` job runs, then [`attach`].
/// 3. A `reserved` row: its `worktree_add` job runs through `jobs` with no
///    transaction open, then 1 transaction for [`observe`].
///
/// It returns only after the job passed (MAP.md of fleetd-p3, Q17).
pub fn create_and_wait(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    lease: &Lease<'_>,
) -> Result<String> {
    let tx = immediate(conn)?;
    let clock = fleet_clock(&tx)?;
    let outcome = create_worktree(ctx, &tx, lease, clock)?;
    tx.commit()?;
    match outcome {
        CreateOutcome::Attached(row) => Ok(row.worktree_id),
        CreateOutcome::NeedsRescue(row) => {
            rescue_now(conn, ctx, jobs, &row)?;
            let tx = immediate(conn)?;
            let id = &row.worktree_id;
            let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id}"))?;
            match attach(&tx, &row, lease.worker_id, lease.attempt)? {
                Attach::Attached(row) => {
                    tx.commit()?;
                    Ok(row.worktree_id)
                }
                Attach::NeedsRescue(_) => {
                    bail!("worktree {id} is still orphaned after its rescue")
                }
            }
        }
        CreateOutcome::Reserved { row, job } => wait_added(conn, ctx, jobs, &row, job),
    }
}

/// Finish the tree of the `reserved` row `id`, as [`create_and_wait`]
/// does: its ended `worktree_add` run, its active run, or a new one, runs
/// to its end and is observed. A stop between the run and its observation
/// leaves the row `reserved`. A row that the pass moved on first is given
/// as it is. It gives the id of the tree.
pub fn finish_reserved(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    id: &str,
) -> Result<String> {
    let tx = immediate(conn)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id}"))?;
    if row.state != WorktreeState::Reserved {
        return Ok(row.worktree_id);
    }
    // A `worktree_add` run is not used again: an ended run that the stop
    // left unobserved is observed, not run a second time.
    let last = jobs_store::latest(&tx, Kind::WorktreeAdd, id, &worktree_add_key(&row))?;
    let job = match last {
        Some(r) if matches!(r.state, State::Passed | State::Failed) => r.id,
        _ => add_run(ctx, &tx, &row, fleet_clock(&tx)?)?,
    };
    tx.commit()?;
    wait_added(conn, ctx, jobs, &row, job)
}

/// Run the `worktree_add` run `job` of the `reserved` row `row` to its
/// end with no transaction open, then observe it in 1 transaction.
fn wait_added(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    row: &WorktreeRow,
    job: i64,
) -> Result<String> {
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = observe(ctx, &tx, &ended)?;
    let now = store::get(&tx, &row.worktree_id)?;
    tx.commit()?;
    match (seen, now) {
        (Observed::Dropped(reason), _) => bail!(
            "git worktree add failed, so the ledger keeps no worktree {}: {reason}",
            row.path
        ),
        // The pass observed the run first.
        (_, Some(now)) if now.state == WorktreeState::Active => Ok(now.worktree_id),
        (Observed::Kept(why), _) => bail!(
            "worktree {} is not made yet: {why}; run the command again",
            row.path
        ),
        (_, _) => bail!("worktree {} is not active", row.path),
    }
}

/// Give the tree `id` that a restore made to the operator: `active` to
/// `created`, with no owner. The reconciler leaves a `created` tree as it
/// is, but it orphans an `active` tree with no owner (MAP.md of fleetd-p3,
/// Q10 and Q23). `horch worktree restore` calls it for a rescue ref and for
/// a triage ref.
pub fn give_to_operator(tx: &Connection, id: &str) -> Result<()> {
    let cause = Cause {
        actor: Actor::Operator,
        text: "horch worktree restore gives it to the operator, with no owner",
    };
    transition(
        tx,
        None,
        Entity::Worktree,
        id,
        "created",
        cause,
        &NeedsYouList,
    )?;
    Ok(())
}

/// The verified rescue of the `orphaned` tree `row`: its `rescue` job runs
/// through `jobs`, then 1 transaction observes it. Triage calls it too
/// before the removal of a tree (fleetd-p3 ticket 13).
pub fn rescue_now(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    row: &WorktreeRow,
) -> Result<()> {
    let root = executors::fleet_root(conn)?;
    let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)), root)
        .reading(&[PathBuf::from(&row.repo_root)]);
    let head = git
        .tree(Path::new(&row.path))?
        .with_context(|| format!("the orphaned worktree {} is not a work tree", row.path))?
        .head;
    let input_key = rescue_key(&row.path, &head);
    let key = JobKey {
        kind: Kind::Rescue,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    let tx = immediate(conn)?;
    let clock = fleet_clock(&tx)?;
    let job = queued_run(ctx, &tx, &key, &row.repo_root, clock)?;
    tx.commit()?;
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = rescue::observe(ctx, &tx, &ended)?;
    tx.commit()?;
    match seen {
        rescue::Observed::Rescued(_) => Ok(()),
        _ => bail!(
            "the rescue of the orphaned worktree {} did not pass ({}: {}); its tree stays",
            row.path,
            ended.state.as_str(),
            outcome(&ended).map(|o| o.text).unwrap_or_default()
        ),
    }
}

fn immediate(conn: &mut Connection) -> Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

/// The fleet clock. A ledger that no fleetd ever ticked has no `fleetd`
/// row: its clock is 0.
pub fn fleet_clock(conn: &Connection) -> Result<FleetSeconds> {
    match clock_store::read(conn) {
        Ok(clock) => Ok(clock),
        Err(e)
            if matches!(
                e.downcast_ref::<rusqlite::Error>(),
                Some(rusqlite::Error::QueryReturnedNoRows)
            ) =>
        {
            Ok(FleetSeconds(0))
        }
        Err(e) => Err(e),
    }
}

/// The job runner of a unit test: the job process is `true`, started
/// through the launcher, and the executor runs in the test process with
/// the recorded pid, as `horch job run <id>` runs it. A test never starts
/// a `horch` binary. `tests/common/test_jobs.rs` is the same runner for the
/// integration tests.
#[cfg(test)]
pub(crate) struct TestJobs;

#[cfg(test)]
impl RunJob for TestJobs {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow> {
        use crate::fleetd::fleet_runtime::adapters::jobs_store;
        use crate::fleetd::fleet_runtime::application::jobs::{
            CheckRunner, JobRunner, SandboxProfiles,
        };
        use crate::fleetd::worker_profiles::application::ports::LaunchSpec;
        use crate::taskledger::workers::Worker;

        struct NoProfiles;
        impl SandboxProfiles for NoProfiles {
            fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> Result<Option<LaunchSpec>> {
                Ok(None)
            }
        }
        let get = |conn: &Connection| -> Result<JobRow> {
            jobs_store::get(conn, job_id)?.with_context(|| format!("no job {job_id}"))
        };
        let job = get(conn)?;
        if job.state == State::Queued {
            let spec = LaunchSpec {
                program: "true".into(),
                args: Vec::new(),
                cwd: None,
                env_rule: Default::default(),
                detach: false,
                output: None,
            };
            JobRunner::new(&Launcher).start(ctx, conn, &job, &spec, FleetSeconds(0))?;
        }
        let job = get(conn)?;
        if job.state == State::Running {
            let checks = CheckRunner {
                host: &Launcher,
                profiles: &NoProfiles,
            };
            let pid = job.pid.context("a running job has a pid")?;
            jobs::run_job(ctx, conn, job_id, pid, &checks)?;
        }
        get(conn)
    }
}
