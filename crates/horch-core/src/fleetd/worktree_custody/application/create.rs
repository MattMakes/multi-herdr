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
use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::adapters::sqlite::drop_reserved;
use crate::fleetd::fleet_runtime::application::jobs::{self, outcome, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worktree_custody::adapters::git::{worktree_add_key, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use crate::fleetd::worktree_custody::adapters::run_now::{run_now, Starter};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::ports::WorktreeGitGateway;
use crate::fleetd::worktree_custody::application::rescue;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::paths::nonempty_path;
use crate::taskledger::model::Actor;
use crate::taskledger::sql_words::WORKTREES_GONE;

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
    let input_key = worktree_add_key(&row);
    let key = JobKey {
        kind: Kind::WorktreeAdd,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    let job = queued_run(ctx, tx, &key, &row.repo_root, clock)?;
    Ok(CreateOutcome::Reserved { row, job })
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
/// Claim and handoff call it too (ticket 11 of fleetd-p3).
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

/// Observe the `worktree_add` run `job`, in `tx`:
/// - `passed`, and the folder of the `reserved` row exists: the row moves
///   to `created`, then to `active` with the owner of the request.
/// - `failed`, or `passed` with no folder: the `reserved` row is
///   dropped, with the cause `worktree_add_failed`.
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
        CreateOutcome::Reserved { row, job } => {
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
    }
}

/// The verified rescue of the `orphaned` tree `row`: its `rescue` job runs
/// through `jobs`, then 1 transaction observes it.
fn rescue_now(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    row: &WorktreeRow,
) -> Result<()> {
    let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)));
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
fn fleet_clock(conn: &Connection) -> Result<FleetSeconds> {
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
