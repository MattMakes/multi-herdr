//! `horch ledger`: the task ledger CLI (docs/specs/task-ledger.md, #28).
//!
//! Orchestrators queue work (`run new`, `add`, `add-map`, `plan-workers`),
//! workers claim and report it (`claim`, `progress`, `note`), and anyone reads
//! the board (`status`, `show`). The old session-ledger plumbing lives under
//! `horch ledger legacy`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use horch_core::execution::lifecycle::ReportStatus;
use horch_core::messaging::delivery;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::{self, land};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::import;
use horch_core::taskledger::map;
use horch_core::taskledger::model::{
    Actor, EventKind, ProgressSource, StepName, TaskKind, WorkerKind, WorkerStatus, WorktreeMode,
    WorktreeStatus,
};
use horch_core::taskledger::model::{FindingStatus, StopRequester};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::sql_words::word;
use horch_core::taskledger::status;
use horch_core::taskledger::stop;
use horch_core::taskledger::tasks::{self, ClaimOutcome, Claimed, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{self, Store};

use super::sessionscmd::LegacyCommand;
use crate::output;

/// Exit code of `status` when work remains.
pub const EXIT_NOT_COMPLETE: u8 = 1;

#[derive(Subcommand)]
pub enum LedgerCommand {
    /// The board of a run. Exits 0 only when every task is done or cancelled
    /// and every worktree is cleaned or unlanded; 1 while work remains.
    Status {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// 1 task (`T-<n>`) or worker (id or role) with its history.
    Show {
        key: String,
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Runs: `run new <slug>` creates a run and its run branch; `run close`
    /// ends it.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Queue 1 task from a ticket file.
    Add(AddArgs),
    /// Queue every ticket of a ticket map (MAP.md) in 1 transaction.
    AddMap {
        map: PathBuf,
        #[command(flatten)]
        run: RunArg,
    },
    /// Per persona: how many workers to spawn now.
    PlanWorkers {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Let a queued task's fallback persona claim it.
    Fallback {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// Drop a task. Its worker lets go of it at once; `--stop` also stops
    /// that worker by its recorded PIDs and pane.
    Cancel {
        task: String,
        reason: String,
        #[arg(long)]
        stop: bool,
        #[command(flatten)]
        run: RunArg,
    },
    /// Drop 1 dependency: TASK no longer waits for --from (orchestrator).
    Unblock {
        task: String,
        #[arg(long = "from", value_name = "BLOCKER")]
        from: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// Queue a held or cancelled task again (orchestrator). It keeps its
    /// attempt history and gets the run's attempt limit anew.
    Requeue {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// (orchestrator) Rule that 1 code-sweep finding of a task's current
    /// round does not block. Never the author.
    Waive {
        task: String,
        /// The finding id in the code-sweep report, e.g. F1.
        finding: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// (worker) Take the next task meant for this worker's persona.
    Claim {
        #[arg(long)]
        json: bool,
    },
    /// (worker) Your worktree, on a branch from the run branch.
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
    /// (worker) Run the finish steps in order: check, code-sweep,
    /// complexity-sweep, recheck, land, pr, cleanup. Stops at the first
    /// that does not pass; fix it and run finish again.
    Finish {
        #[arg(long)]
        json: bool,
        /// Start a new code-sweep round for the same tree.
        #[arg(long)]
        rereview: bool,
        /// Run the steps in this process and wait for them, also in a
        /// pane where finish would run detached.
        #[arg(long)]
        wait: bool,
        /// The detached run that a plain finish starts (#160).
        #[arg(long, hide = true)]
        run_steps: bool,
    },
    /// The finish steps of a task (default: this worker's) with evidence.
    Steps {
        task: Option<String>,
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// (worker) Finish the task. Refuses until every finish step passed.
    Done {
        summary: Vec<String>,
        /// Last resort: the work cannot land. Keeps the branch and worktree.
        #[arg(long, value_name = "REASON")]
        unlanded: Option<String>,
    },
    /// (worker) Record a milestone; it counts as progress.
    Progress { text: String },
    /// (worker) Record a note; it counts as progress.
    Note { text: String },
    /// Stop 1 worker by the PIDs and pane its row records. Takes only a
    /// worker id or role: never a PID, a name or a pattern.
    Stop {
        worker: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Reap a dead worker: stop it by its recorded PIDs, end its attempt
    /// incomplete, and queue its task again, first in line.
    Reap {
        worker: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// The watchdog: check every working worker every check interval, reap
    /// the dead, wake the orchestrator on a verdict change. `--once` runs 1
    /// pass (every worker, due or not, with `--force`).
    Watch {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        once: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Import every old JSON session ledger and worktree registry (raw first,
    /// typed after), and record what never landed.
    Import {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Past work that did not finish as intended: worktrees that never
    /// landed, work marked done that did not land, workers that ended
    /// without done. Newest first.
    Unfinished {
        /// Only this project (default: the current one; `all` for every one).
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete the worktree triage trash refs (`refs/horch/trash/...`) of
    /// this project that are older than 30 days (#308).
    Gc,
    /// Queue a task for an unfinished finding; its claim gets the old
    /// branch, worktree and transcript.
    Reopen {
        finding: i64,
        #[arg(long = "file")]
        file: PathBuf,
        #[arg(long)]
        primary: String,
        #[arg(long)]
        fallback: Option<String>,
        #[arg(long)]
        check: Option<String>,
        #[command(flatten)]
        run: RunArg,
    },
    /// Close a finding with a reason. `--cleanup` removes its 1 recorded
    /// worktree, only when no work can be lost.
    Dismiss {
        finding: i64,
        reason: String,
        #[arg(long)]
        cleanup: bool,
    },
    /// Worker rows (plumbing for tests and debugging).
    #[command(hide = true)]
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },
    /// The old session-ledger plumbing.
    Legacy {
        #[command(subcommand)]
        command: LegacyCommand,
    },
    /// The run's history: every event, oldest first (wakes included).
    Events {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Print the task ledger's file path.
    Path,
}

#[derive(Subcommand)]
pub enum WorktreeCommand {
    /// Create this worker's worktree (or get the task's existing one back).
    New {
        name: String,
        #[arg(long)]
        from: Option<String>,
    },
    /// Create a registered scratch worktree that does not block completion.
    Scratch { name: String },
    /// Abandon a clean registered worktree without losing unique commits.
    Drop {
        name: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        keep_branch: bool,
    },
}

#[derive(Subcommand)]
pub enum RunCommand {
    /// Create a run and its run branch (from the main branch).
    New(NewRunArgs),
    /// Close the run when its work is done. Refuses while a task is queued,
    /// held, running or landing, or a worktree is open or landing.
    Close {
        #[command(flatten)]
        run: RunArg,
        /// Cancel every open task with this reason, then close the run as
        /// abandoned.
        #[arg(long, value_name = "REASON")]
        abandon: Option<String>,
    },
    /// Every run of this project.
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct NewRunArgs {
    slug: String,
    /// The branch tasks land on; default `fleet/<slug>`.
    #[arg(long)]
    branch: Option<String>,
    /// The main branch the run branch starts from and the PR targets;
    /// default the branch checked out in the project.
    #[arg(long)]
    base: Option<String>,
    /// The remote the run branch is pushed to; default `origin` when it
    /// exists.
    #[arg(long)]
    remote: Option<String>,
    /// The finish steps, comma-separated; default all 7.
    #[arg(long, value_delimiter = ',')]
    steps: Vec<String>,
    /// Seconds between watchdog checks (default 600).
    #[arg(long, hide = true)]
    check_every: Option<i64>,
    /// Checks in a row without progress before a worker is dead (default 3).
    #[arg(long, hide = true)]
    stall_rounds: Option<i64>,
    /// Seconds between supervisor checkins (default 30).
    #[arg(long, hide = true)]
    heartbeat_every: Option<i64>,
    /// Seconds after which a checkin is missed (default 120).
    #[arg(long, hide = true)]
    heartbeat_stale: Option<i64>,
    /// Slack, in seconds, when a screen timer is compared with the clock.
    #[arg(long, hide = true)]
    timer_tolerance: Option<i64>,
    /// Attempts at 1 task before horch holds it and asks the operator
    /// (default 3).
    #[arg(long)]
    max_attempts: Option<i64>,
}

#[derive(Subcommand)]
pub enum WorkerCommand {
    /// Insert a worker row and print its id.
    New {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        replaces: Option<String>,
        /// The herdr pane the worker runs in.
        #[arg(long)]
        pane: Option<String>,
        #[command(flatten)]
        run: RunArg,
    },
    /// Record a PID for a worker row (supervisor or agent).
    Pid {
        worker: String,
        #[arg(long)]
        supervisor: Option<u32>,
        #[arg(long)]
        agent: Option<u32>,
    },
}

#[derive(Args, Clone, Default)]
pub struct RunArg {
    /// The run: an id or a slug. Default: the 1 active run of this project.
    #[arg(long = "run", value_name = "RUN")]
    pub run: Option<String>,
}

#[derive(Args)]
pub struct AddArgs {
    /// The ticket file the worker reads.
    #[arg(long = "file")]
    file: PathBuf,
    /// The persona that claims it.
    #[arg(long)]
    primary: String,
    #[arg(long)]
    fallback: Option<String>,
    /// Default: the ticket's first heading.
    #[arg(long)]
    title: Option<String>,
    /// Run alone: no other task of the run runs beside it.
    #[arg(long, conflicts_with = "parallel")]
    exclusive: bool,
    /// May run beside other tasks (the default).
    #[arg(long)]
    parallel: bool,
    /// Tasks this one waits for: `T-<n>` or ids.
    #[arg(long = "blocked-by", value_delimiter = ',')]
    blocked_by: Vec<String>,
    /// Files this task owns.
    #[arg(long = "own", value_delimiter = ',')]
    own: Vec<String>,
    /// Files this task must not touch.
    #[arg(long = "no-touch", value_delimiter = ',')]
    no_touch: Vec<String>,
    /// The check the finish steps run.
    #[arg(long)]
    check: Option<String>,
    /// Seconds the check may run before it is killed (default 1200).
    #[arg(long = "check-timeout")]
    check_timeout: Option<u64>,
    #[arg(long)]
    phase: Option<String>,
    #[arg(long)]
    issue: Option<String>,
    #[arg(long)]
    spec: Option<String>,
    #[arg(long, default_value_t = 100)]
    priority: i64,
    /// Work in the shared checkout, not a worktree.
    #[arg(long)]
    shared: bool,
    #[command(flatten)]
    run: RunArg,
}

/// The project root a command means: the main checkout of the repository
/// the project dir is in, else the project dir itself.
pub fn project_root(ctx: &RuntimeContext) -> Result<PathBuf> {
    let dir = ctx.paths.project()?;
    let git = Git::new(&ctx.bins.harness.git);
    Ok(git
        .main_root(&dir)
        .unwrap_or_else(|| dir.canonicalize().unwrap_or(dir)))
}

/// The run a command means: `--run`, else `HORCH_RUN_ID`, else the worker's
/// run, else the project's 1 active run.
pub fn resolve_run(ctx: &RuntimeContext, store: &Store, arg: &RunArg) -> Result<Run> {
    let explicit = named_run(arg);
    if explicit.is_none() {
        if let Some(run) = worker_run(ctx, store)? {
            return Ok(run);
        }
    }
    let workspace = ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string());
    let (run, note) = runs::resolve(
        store.conn(),
        &project_root(ctx)?,
        explicit.as_deref(),
        workspace.as_deref(),
    )?;
    if let Some(note) = note {
        eprintln!("horch: {note}");
    }
    Ok(run)
}

/// The run `--run` or `HORCH_RUN_ID` names.
fn named_run(arg: &RunArg) -> Option<String> {
    arg.run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()))
}

/// The run of the ledger worker in `HORCH_WORKER_ID`, when it has one.
fn worker_run(ctx: &RuntimeContext, store: &Store) -> Result<Option<Run>> {
    let Some(wid) = worker_id_opt(ctx) else {
        return Ok(None);
    };
    let Some(w) = workers::find(store.conn(), &wid)? else {
        return Ok(None);
    };
    w.run_id.map(|r| runs::get(store.conn(), &r)).transpose()
}

/// Refuse `what` when the caller is a ledger worker: the orchestrator
/// decides it.
fn orchestrator_only(ctx: &RuntimeContext, store: &Store, what: &str) -> Result<()> {
    if let Some(wid) = worker_id_opt(ctx) {
        if let Some(w) = workers::find(store.conn(), &wid)? {
            if w.kind == WorkerKind::Worker.as_str() {
                bail!("horch ledger {what} is for the orchestrator, not a worker");
            }
        }
    }
    Ok(())
}

fn worker_id_opt(ctx: &RuntimeContext) -> Option<String> {
    ctx.worker.as_ref().and_then(|w| w.worker_id.clone())
}

/// This worker's id, from `HORCH_WORKER_ID`.
pub fn worker_id(ctx: &RuntimeContext) -> Result<String> {
    worker_id_opt(ctx)
        .context("HORCH_WORKER_ID is not set: run this from a worker pane that horch spawn opened")
}

pub(super) fn json(v: &impl serde::Serialize) -> Result<()> {
    output::println(&serde_json::to_string_pretty(v)?);
    Ok(())
}

/// The dispatch of 1 group of `LedgerCommand` variants.
type GroupCmd = fn(&RuntimeContext, Store, LedgerCommand) -> Result<ExitCode>;

/// The groups of `LedgerCommand` variants, each with its own dispatch.
#[derive(Clone, Copy)]
enum Group {
    Board,
    Control,
    Worker,
    Admin,
}

fn group_of(command: &LedgerCommand) -> Group {
    use LedgerCommand as C;
    if matches!(
        command,
        C::Status { .. }
            | C::Show { .. }
            | C::Run { .. }
            | C::Add(_)
            | C::AddMap { .. }
            | C::PlanWorkers { .. }
            | C::Events { .. }
    ) {
        Group::Board
    } else if matches!(
        command,
        C::Fallback { .. }
            | C::Cancel { .. }
            | C::Unblock { .. }
            | C::Requeue { .. }
            | C::Waive { .. }
            | C::Stop { .. }
            | C::Reap { .. }
    ) {
        Group::Control
    } else if matches!(
        command,
        C::Claim { .. }
            | C::Worktree { .. }
            | C::Finish { .. }
            | C::Steps { .. }
            | C::Done { .. }
            | C::Progress { .. }
            | C::Note { .. }
    ) {
        Group::Worker
    } else {
        Group::Admin
    }
}

pub fn run(ctx: &RuntimeContext, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Legacy { command } = command {
        return super::sessionscmd::run(ctx, command);
    }
    if let LedgerCommand::Gc = command {
        return super::worktree::gc(ctx).map(|()| ExitCode::SUCCESS);
    }
    if let LedgerCommand::Path = command {
        output::println(&ctx.paths.ledger_db.display().to_string());
        return Ok(ExitCode::SUCCESS);
    }
    let store = taskledger::open(ctx)?;
    // One entry per `Group`, in its order.
    let dispatch: [GroupCmd; 4] = [board_cmd, control_cmd, worker_side_cmd, admin_cmd];
    dispatch[group_of(&command) as usize](ctx, store, command)
}

/// The orchestrator reads and fills the board.
fn board_cmd(ctx: &RuntimeContext, mut store: Store, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Status { run, json: as_json } = command {
        return status_cmd(ctx, &store, &run, as_json);
    }
    if let LedgerCommand::Show {
        key,
        run,
        json: as_json,
    } = command
    {
        return show(ctx, &store, &key, &run, as_json);
    }
    if let LedgerCommand::Run { command } = command {
        return run_cmd(ctx, &mut store, command);
    }
    if let LedgerCommand::Add(args) = command {
        return add(ctx, &mut store, args);
    }
    if let LedgerCommand::AddMap { map: path, run } = command {
        return add_map(ctx, &mut store, &path, &run);
    }
    if let LedgerCommand::PlanWorkers { run, json: as_json } = command {
        return plan_workers_cmd(ctx, &store, &run, as_json);
    }
    if let LedgerCommand::Events { run, json: as_json } = command {
        return events_cmd(ctx, &store, &run, as_json);
    }
    unreachable!("not a board command")
}

/// The orchestrator steers tasks and workers.
fn control_cmd(ctx: &RuntimeContext, mut store: Store, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Fallback { task, reason, run } = command {
        return fallback_cmd(ctx, &mut store, &task, &reason, &run);
    }
    if let LedgerCommand::Cancel {
        task,
        reason,
        stop: stop_it,
        run,
    } = command
    {
        return cancel_cmd(ctx, &mut store, &task, &reason, stop_it, &run);
    }
    if let LedgerCommand::Unblock { task, from, run } = command {
        return unblock_cmd(ctx, &mut store, &task, &from, &run);
    }
    if let LedgerCommand::Requeue { task, reason, run } = command {
        return requeue_cmd(ctx, &mut store, &task, &reason, &run);
    }
    if let LedgerCommand::Waive {
        task,
        finding,
        reason,
        run,
    } = command
    {
        return waive_cmd(ctx, &store, &task, &finding, &reason, &run);
    }
    if let LedgerCommand::Stop {
        worker,
        reason,
        run,
        json: as_json,
    } = command
    {
        return stop_cmd(ctx, &mut store, &worker, &reason, &run, as_json);
    }
    if let LedgerCommand::Reap {
        worker,
        reason,
        run,
        json: as_json,
    } = command
    {
        return reap_cmd(ctx, &mut store, &worker, &reason, &run, as_json);
    }
    unreachable!("not a control command")
}

/// What a worker does with its task.
fn worker_side_cmd(
    ctx: &RuntimeContext,
    mut store: Store,
    command: LedgerCommand,
) -> Result<ExitCode> {
    if let LedgerCommand::Claim { json: as_json } = command {
        return claim(ctx, &mut store, as_json);
    }
    if let LedgerCommand::Worktree { command } = command {
        return worktree_cmd(ctx, &mut store, command);
    }
    if let LedgerCommand::Finish {
        json: as_json,
        rereview,
        wait,
        run_steps,
    } = command
    {
        let how = super::taskledger_finish::How {
            as_json,
            rereview,
            wait,
            run_steps,
        };
        return super::taskledger_finish::finish_cmd(ctx, &mut store, how);
    }
    if let LedgerCommand::Steps {
        task,
        run,
        json: as_json,
    } = command
    {
        return steps_cmd(ctx, &store, task, &run, as_json);
    }
    if let LedgerCommand::Done { summary, unlanded } = command {
        return done_exit(ctx, store, &summary.join(" "), unlanded.as_deref());
    }
    if let LedgerCommand::Progress { text } = command {
        return progress(ctx, &mut store, EventKind::Progress, &text);
    }
    if let LedgerCommand::Note { text } = command {
        return progress(ctx, &mut store, EventKind::Note, &text);
    }
    unreachable!("not a worker command")
}

/// `horch ledger done`: the exit code once the finish is recorded.
fn done_exit(
    ctx: &RuntimeContext,
    store: Store,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<ExitCode> {
    done_cmd(ctx, store, summary, unlanded).map(|()| ExitCode::SUCCESS)
}

/// Import, unfinished work, the watchdog and worker rows.
fn admin_cmd(ctx: &RuntimeContext, mut store: Store, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Import {
        dry_run,
        json: as_json,
    } = command
    {
        return import_cmd(ctx, &mut store, dry_run, as_json);
    }
    if let LedgerCommand::Unfinished {
        project,
        json: as_json,
    } = command
    {
        return unfinished_cmd(ctx, &mut store, project.as_deref(), as_json);
    }
    if let LedgerCommand::Reopen {
        finding,
        file,
        primary,
        fallback,
        check,
        run,
    } = command
    {
        return {
            let ticket = ReopenTicket {
                file,
                primary,
                fallback,
                check,
            };
            reopen_cmd(ctx, &mut store, finding, ticket, &run)
        };
    }
    if let LedgerCommand::Dismiss {
        finding,
        reason,
        cleanup,
    } = command
    {
        return dismiss_cmd(ctx, &store, finding, &reason, cleanup);
    }
    if let LedgerCommand::Worker { command } = command {
        return worker_cmd(ctx, &mut store, command);
    }
    if let LedgerCommand::Watch {
        run,
        once,
        force,
        json: as_json,
    } = command
    {
        return {
            let r = resolve_run(ctx, &store, &run)?;
            super::taskledger_watch::watch(ctx, store, &r, once, force, as_json)
        };
    }
    unreachable!("not a admin command")
}

fn plan_workers_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    adopt_workspace(ctx, store, &r)?;
    let plan = tasks::plan_workers(store.conn(), &r.run_id)?;
    if as_json {
        json(&plan)?;
        return Ok(ExitCode::SUCCESS);
    }
    if plan.is_empty() {
        output::println("No task can start now. Spawn no worker.");
    }
    for p in &plan {
        let mut line = format!(
            "{}: spawn {} ({} can start, {} on the way).",
            p.persona, p.spawn, p.startable, p.on_the_way
        );
        if !p.replaces.is_empty() {
            line.push_str(&format!(
                " Replaces: {}. Spawn those with --replaces <worker>.",
                p.replaces.join(", ")
            ));
        }
        output::println(&line);
    }
    Ok(ExitCode::SUCCESS)
}

fn fallback_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let t = store.immediate(|tx| {
        let t = tasks::resolve(tx, &r.run_id, task)?;
        tasks::fallback(tx, &t, reason)
    })?;
    output::println(&format!(
        "{} can now be claimed by {}.",
        t.short(),
        t.claimable_by.as_deref().unwrap_or("?")
    ));
    Ok(ExitCode::SUCCESS)
}

fn cancel_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    task: &str,
    reason: &str,
    stop_it: bool,
    run: &RunArg,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let holder = tasks::resolve(store.conn(), &r.run_id, task)?.current_worker_id;
    if let (true, Some(w)) = (stop_it, &holder) {
        stop_holder(ctx, store, w, reason)?;
    }
    let t = cancel_task(store, &r, task, reason)?;
    output::println(&format!("{} cancelled.", t.short()));
    if let (false, Some(w)) = (stop_it, &holder) {
        warn_still_running(w, &t.short());
    }
    wake_dependents(ctx, store, &r, &t)
}

fn cancel_task(store: &mut Store, run: &Run, task: &str, reason: &str) -> Result<tasks::Task> {
    store.immediate(|tx| {
        let t = tasks::resolve(tx, &run.run_id, task)?;
        tasks::cancel(tx, &t, reason)
    })
}

/// Name the tasks that wait for a cancelled task, and wake on them.
fn wake_dependents(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    t: &tasks::Task,
) -> Result<ExitCode> {
    let waiting = tasks::dependents(store.conn(), &t.task_id)?;
    print_waiting(&t.short(), &waiting);
    super::taskledger_wake::cancelled_dependents(ctx, store, run, &t.short(), &waiting);
    Ok(ExitCode::SUCCESS)
}

fn warn_still_running(worker: &str, task: &str) {
    eprintln!(
        "horch ledger: worker {worker} held {task} and can no longer finish it. Its process may still run: horch ledger stop {worker} \"<reason>\"."
    );
}

/// Stop the worker that held a cancelled task.
fn stop_holder(ctx: &RuntimeContext, store: &mut Store, worker: &str, reason: &str) -> Result<()> {
    let report = stop::stop_worker(
        store,
        worker,
        StopRequester::Orchestrator,
        &format!("task cancelled: {reason}"),
        &HerdrPanes::new(ctx),
        &stop::Os,
        &stop::StopOptions {
            grace: stop_grace(),
            ..Default::default()
        },
    )?;
    print_stop(&report, false)
}

/// The tasks that wait for a cancelled task.
fn print_waiting(task: &str, waiting: &[tasks::Task]) {
    if waiting.is_empty() {
        return;
    }
    let names: Vec<String> = waiting.iter().map(|d| d.short()).collect();
    output::println(&format!("Waiting for {task}: {}.", names.join(", ")));
}

fn unblock_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    task: &str,
    from: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    orchestrator_only(ctx, store, "unblock")?;
    let r = resolve_run(ctx, store, run)?;
    let t = store.immediate(|tx| {
        let t = tasks::resolve(tx, &r.run_id, task)?;
        let b = tasks::resolve(tx, &r.run_id, from)?;
        tasks::unblock(tx, &t, &b)?;
        Ok(t)
    })?;
    output::println(&format!("{} no longer waits for {from}.", t.short()));
    super::taskledger_wake::nudge_startable(ctx, store, &r);
    Ok(ExitCode::SUCCESS)
}

fn requeue_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    orchestrator_only(ctx, store, "requeue")?;
    let r = resolve_run(ctx, store, run)?;
    let t = store.immediate(|tx| {
        let t = tasks::resolve(tx, &r.run_id, task)?;
        tasks::requeue_by_operator(tx, &t, reason)
    })?;
    output::println(&format!("{} is queued again.", t.short()));
    super::taskledger_wake::nudge_startable(ctx, store, &r);
    Ok(ExitCode::SUCCESS)
}

fn waive_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    task: &str,
    finding: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    // The caller: its worker id, else the run's worker in its pane, so
    // a worker that unsets HORCH_WORKER_ID is still known.
    let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
    let caller = worker_id_opt(ctx)
        .or_else(|| finish::codesweep::worker_in_pane(store.conn(), &r.run_id, pane.as_deref()));
    if let Some(w) = caller {
        let w = workers::get(store.conn(), &w)?;
        if w.kind != WorkerKind::Orchestrator.as_str() {
            bail!("only the orchestrator waives a code-sweep finding; fix it, or answer an Important finding in the answers file");
        }
    }
    let t = tasks::resolve(store.conn(), &r.run_id, task)?;
    let w = finish::codesweep::waive(store, &tools(ctx).evidence_root, &t, finding, reason)?;
    output::println(&format!(
        "{} round {}: {} waived. The next horch ledger finish of {} reads it.",
        t.short(),
        w.round,
        w.id,
        t.short()
    ));
    Ok(ExitCode::SUCCESS)
}

fn worktree_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    command: WorktreeCommand,
) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let git = Git::new(&ctx.bins.harness.git);
    match command {
        WorktreeCommand::New { name, from } => {
            let wt =
                taskledger::worktree_ops::create(store, &git, &wid, &name, from.as_deref(), false)?;
            output::println(&format!("path: {}", wt.path));
            output::println(&format!("branch: {}", wt.branch));
            output::println(&format!("base: {}", wt.base));
            output::println(&format!(
                "Work and commit in {}. When the work is complete, run: horch ledger finish",
                wt.path
            ));
        }
        WorktreeCommand::Scratch { name } => {
            let wt = taskledger::worktree_ops::create(store, &git, &wid, &name, None, true)?;
            output::println(&format!("path: {}", wt.path));
            output::println(&format!("branch: {}", wt.branch));
            output::println("Scratch worktree: drop it when it is no longer needed; it does not block completion.");
        }
        WorktreeCommand::Drop {
            name,
            reason,
            keep_branch,
        } => drop_worktree_cmd(store, &git, &wid, &name, &reason, keep_branch)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn drop_worktree_cmd(
    store: &mut Store,
    git: &Git,
    wid: &str,
    name: &str,
    reason: &str,
    keep_branch: bool,
) -> Result<()> {
    let worker = workers::get(store.conn(), wid)?;
    let run_id = worker.run_id.as_deref().context("worker has no run")?;
    let wt =
        taskledger::worktree_ops::drop_worktree(store, git, run_id, name, reason, keep_branch)?;
    output::println(&format!("abandoned: {} ({reason})", wt.path));
    Ok(())
}

fn steps_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    task: Option<String>,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let (task_id, worker) = steps_target(ctx, store, task, run)?;
    let steps = finish::steps_of(store.conn(), &task_id, &worker)?;
    if as_json {
        json(&steps)?;
    } else {
        print_steps(&steps);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_steps(steps: &[finish::Step]) {
    for st in steps {
        output::println(&format!(
            "{:<2} {:<17} {:<15} {:<16} {}",
            st.seq,
            st.name,
            st.status,
            st.verdict.as_deref().unwrap_or("-"),
            st.evidence_path.as_deref().unwrap_or("")
        ));
    }
}

/// The task and worker whose steps to show: the named task's, else this
/// worker's own.
fn steps_target(
    ctx: &RuntimeContext,
    store: &Store,
    task: Option<String>,
    run: &RunArg,
) -> Result<(String, String)> {
    if let Some(key) = task {
        return task_attempt(ctx, store, &key, run);
    }
    let wid = worker_id(ctx)?;
    let w = workers::get(store.conn(), &wid)?;
    Ok((w.task_id.context("this worker holds no task")?, wid))
}

/// A task's id and the worker of its current or last attempt.
fn task_attempt(
    ctx: &RuntimeContext,
    store: &Store,
    key: &str,
    run: &RunArg,
) -> Result<(String, String)> {
    let r = resolve_run(ctx, store, run)?;
    let t = tasks::resolve(store.conn(), &r.run_id, key)?;
    let w = t
        .current_worker_id
        .clone()
        .or_else(|| {
            workers::attempts_at(store.conn(), &t.task_id)
                .ok()
                .and_then(|a| a.last().map(|w| w.worker_id.clone()))
        })
        .context("no attempt at this task yet")?;
    Ok((t.task_id, w))
}

fn events_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let ev = status::events(store.conn(), "run_id", &r.run_id)?;
    if as_json {
        json(&ev)?;
    } else {
        for e in ev {
            output::println(&format!(
                "{} {:<12} {:<13} {}",
                e.at, e.actor, e.kind, e.text
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn import_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    dry_run: bool,
    as_json: bool,
) -> Result<ExitCode> {
    let git = Git::new(&ctx.bins.harness.git);
    let r = import::import(store, &git, &ctx.paths.state_root, dry_run)?;
    if as_json {
        json(&r)?;
        return Ok(ExitCode::SUCCESS);
    }
    output::println(&format!(
        "{}{} files: {} records copied ({} already there); {} workers, {} tasks, {} worktrees mapped; {} could not be mapped.",
        if r.dry_run { "Dry run, nothing kept. " } else { "" },
        r.files.len(),
        r.raw_added,
        r.raw_skipped,
        r.workers,
        r.tasks,
        r.worktrees,
        r.map_errors
    ));
    for (k, n) in &r.findings {
        output::println(&format!("  {k}: {n}"));
    }
    if !r.findings.is_empty() {
        output::println("Read them with: horch ledger unfinished");
    }
    Ok(ExitCode::SUCCESS)
}

fn unfinished_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    project: Option<&str>,
    as_json: bool,
) -> Result<ExitCode> {
    let root = match project {
        Some("all") => None,
        Some(p) => Some(PathBuf::from(p)),
        None => Some(project_root(ctx)?),
    };
    let list = store.immediate(|tx| import::unfinished(tx, root.as_deref()))?;
    if as_json {
        json(&list)?;
    } else {
        print_unfinished(&list);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_unfinished(list: &[import::Unfinished]) {
    if list.is_empty() {
        output::println("No unfinished work.");
        return;
    }
    for f in list {
        output::println(&format!(
            "F{} {} [{}]: {}",
            f.finding_id,
            f.kind,
            f.run.as_deref().unwrap_or("-"),
            f.detail
        ));
    }
    output::println("Reopen 1: horch ledger reopen <id> --file <ticket> --primary <persona>. Close 1: horch ledger dismiss <id> \"<reason>\"");
}

/// The ticket, persona and check of `horch ledger reopen`.
struct ReopenTicket {
    file: PathBuf,
    primary: String,
    fallback: Option<String>,
    check: Option<String>,
}

/// The run, the new task and the finding that `reopen` works from.
fn reopen_plan(
    ctx: &RuntimeContext,
    store: &Store,
    finding: i64,
    ticket: ReopenTicket,
    run: &RunArg,
) -> Result<(Run, NewTask, import::Unfinished)> {
    let root = project_root(ctx)?;
    let cwd = ctx.paths.current_dir().unwrap_or_else(|_| root.clone());
    let path = ticket_path(&root, &ticket.file, &cwd)?;
    let r = resolve_run(ctx, store, run)?;
    let f = import::get_finding(store.conn(), finding)?;
    let mut t = NewTask::work(&ticket_title(&root.join(&path)), &path, &ticket.primary);
    t.fallback = ticket.fallback;
    t.land_check = ticket.check;
    t.spec_ref = Some(format!("finding F{finding}"));
    Ok((r, t, f))
}

/// Queue the reopened task and close the finding; the finding's old
/// worktree, when it is still on disk, becomes the task's.
fn queue_reopened(
    store: &mut Store,
    run: &Run,
    new: &NewTask,
    finding: &import::Unfinished,
) -> Result<tasks::Task> {
    store.immediate(|tx| {
        let task = tasks::add(tx, run, new, Actor::Orchestrator)?;
        import::decide(
            tx,
            finding.finding_id,
            FindingStatus::Requeued,
            "orchestrator",
            &task.task_id,
        )?;
        // The old worktree, when it is still on disk, is this task's now.
        if let Some(wt) = &finding.worktree_id {
            if reusable(&finish::worktree(tx, wt)?) {
                tx.execute(
                    &format!(
                        "UPDATE worktrees SET task_id = ?2, worker_id = NULL, status = {open} WHERE worktree_id = ?1",
                        open = word(WorktreeStatus::Open)
                    ),
                    [wt, &task.task_id],
                )?;
            }
        }
        Ok(task)
    })
}

fn reopen_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    finding: i64,
    ticket: ReopenTicket,
    run: &RunArg,
) -> Result<ExitCode> {
    let primary = ticket.primary.clone();
    let (r, t, f) = reopen_plan(ctx, store, finding, ticket, run)?;
    let task = queue_reopened(store, &r, &t, &f)?;
    output::println(&format!(
        "{} queued for {} from finding F{finding}.",
        task.short(),
        primary
    ));
    Ok(ExitCode::SUCCESS)
}

/// Whether a finding's old worktree is still on disk and not cleaned.
fn reusable(w: &finish::WorktreeRow) -> bool {
    Path::new(&w.path).exists() && w.status != "cleaned"
}

fn dismiss_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    finding: i64,
    reason: &str,
    cleanup: bool,
) -> Result<ExitCode> {
    let f = import::get_finding(store.conn(), finding)?;
    if cleanup {
        let Some(wt) = &f.worktree_id else {
            bail!("finding F{finding} has no recorded worktree; --cleanup removes only a recorded path");
        };
        let w = finish::worktree(store.conn(), wt)?;
        let git = Git::new(&ctx.bins.harness.git);
        let o = land::remove(store, &git, &w, &w.base, None)?;
        if !o.status.is_through() {
            bail!("{}", o.note);
        }
        output::println(&o.note);
    }
    import::decide(
        store.conn(),
        finding,
        FindingStatus::Dismissed,
        "orchestrator",
        reason,
    )?;
    output::println(&format!("Finding F{finding} dismissed."));
    Ok(ExitCode::SUCCESS)
}

fn stop_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    worker: &str,
    reason: &str,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let w = workers::resolve(store.conn(), &r.run_id, worker)?;
    let herdr = HerdrPanes::new(ctx);
    let report = stop::stop_worker(
        store,
        &w.worker_id,
        StopRequester::Orchestrator,
        reason,
        &herdr,
        &stop::Os,
        &stop::StopOptions {
            grace: stop_grace(),
            ..Default::default()
        },
    )?;
    print_stop(&report, as_json)?;
    if let Some(t) = &report.requeued {
        super::taskledger_wake::nudge_startable(ctx, store, &r);
        eprintln!("horch ledger: {t} is queued again.");
    }
    Ok(ExitCode::SUCCESS)
}

fn reap_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    worker: &str,
    reason: &str,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let w = workers::resolve(store.conn(), &r.run_id, worker)?;
    let herdr = HerdrPanes::new(ctx);
    let report = stop::reap(
        store,
        &w.worker_id,
        StopRequester::Orchestrator,
        reason,
        &herdr,
        &stop::Os,
        stop_grace(),
    )?;
    print_stop(&report.stop, as_json)?;
    super::taskledger_wake::after_reap(ctx, store, &r, &report);
    Ok(ExitCode::SUCCESS)
}

/// A run created outside herdr has no workspace (#218). The first
/// `status` or `plan-workers` from a herdr pane that is not a worker's
/// records that pane's workspace on the run, so wakes find its orchestrator.
fn adopt_workspace(ctx: &RuntimeContext, store: &Store, run: &Run) -> Result<()> {
    let (None, Some(ws), Some(_pane)) = (&run.workspace_id, &ctx.herdr.workspace, &ctx.herdr.pane)
    else {
        return Ok(());
    };
    let role = ctx.worker.as_ref().and_then(|w| w.role.as_deref());
    if role.is_some_and(|r| r != "orchestrator") {
        return Ok(());
    }
    let changed = store.conn().execute(
        "UPDATE runs SET workspace_id = ?2 WHERE run_id = ?1 AND workspace_id IS NULL",
        [&run.run_id, ws.as_str()],
    )?;
    if changed == 1 {
        eprintln!(
            "NOTE: run {} had no workspace. It now has workspace {}, so wakes go to the orchestrator of this workspace.",
            run.slug,
            ws.as_str()
        );
    }
    Ok(())
}

/// Whether the project has no run, and nothing names one.
fn no_runs_to_show(ctx: &RuntimeContext, store: &Store, arg: &RunArg) -> Result<bool> {
    let explicit = arg.run.is_some() || std::env::var("HORCH_RUN_ID").is_ok_and(|v| !v.is_empty());
    let all = runs::of_project(store.conn(), &project_root(ctx)?)?;
    Ok(all.is_empty() && !explicit && worker_id_opt(ctx).is_none())
}

fn print_no_runs(as_json: bool) -> Result<ExitCode> {
    if as_json {
        json(&serde_json::json!({ "runs": [] }))?;
    } else {
        output::println("No runs.");
    }
    Ok(ExitCode::SUCCESS)
}

/// Print the board; the exit code is 1 while work remains.
fn print_status(report: &status::Report, as_json: bool) -> Result<ExitCode> {
    if as_json {
        json(report)?;
    } else {
        output::print(&status::render(report));
    }
    Ok(exit_of(report.complete))
}

pub(super) fn exit_of(complete: bool) -> ExitCode {
    if complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOT_COMPLETE)
    }
}

fn status_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    arg: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    if no_runs_to_show(ctx, store, arg)? {
        return print_no_runs(as_json);
    }
    let run = resolve_run(ctx, store, arg)?;
    adopt_workspace(ctx, store, &run)?;
    let report = status::report(store.conn(), &run)?;
    print_status(&report, as_json)
}

fn show(
    ctx: &RuntimeContext,
    store: &Store,
    key: &str,
    arg: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let run = resolve_run(ctx, store, arg)?;
    let conn = store.conn();
    if let Ok(t) = tasks::resolve(conn, &run.run_id, key) {
        return show_task(store, &t, as_json);
    }
    show_worker(store, &run, key, as_json)
}

/// A worker (id or role) with its events and stops.
fn show_worker(store: &Store, run: &Run, key: &str, as_json: bool) -> Result<ExitCode> {
    let conn = store.conn();
    let w = workers::resolve(conn, &run.run_id, key)
        .with_context(|| format!("'{key}' is neither a task nor a worker of run {}", run.slug))?;
    let events = status::events(conn, "worker_id", &w.worker_id)?;
    if as_json {
        let stops = status::stops(conn, &w.worker_id)?;
        json(&serde_json::json!({ "worker": w, "events": events, "stops": stops }))?;
    } else {
        print_worker(&w, &events);
    }
    Ok(ExitCode::SUCCESS)
}

/// A task with its events, attempts and blockers.
fn show_task(store: &Store, t: &tasks::Task, as_json: bool) -> Result<ExitCode> {
    let conn = store.conn();
    let events = status::events(conn, "task_id", &t.task_id)?;
    let attempts = workers::attempts_at(conn, &t.task_id)?;
    let blockers = tasks::blockers(conn, &t.task_id)?;
    if as_json {
        let own = tasks::files(
            conn,
            &t.task_id,
            horch_core::taskledger::model::FileMode::Own,
        )?;
        json(&serde_json::json!({
            "task": t, "events": events, "attempts": attempts,
            "blocked_by": blockers.iter().map(|b| b.short()).collect::<Vec<_>>(),
            "own": own,
        }))?;
    } else {
        print_task(t, &blockers, &attempts, &events);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_task(
    t: &tasks::Task,
    blockers: &[tasks::Task],
    attempts: &[workers::Worker],
    events: &[status::EventLine],
) {
    output::println(&format!("{} {}: {}", t.short(), t.status, t.title));
    output::println(&format!(
        "Ticket: {}",
        t.task_file.as_deref().unwrap_or("-")
    ));
    output::println(&format!(
        "Persona: {} (primary {}, fallback {}).",
        t.claimable_by.as_deref().unwrap_or("-"),
        t.primary_persona.as_deref().unwrap_or("-"),
        t.fallback_persona.as_deref().unwrap_or("-")
    ));
    if !blockers.is_empty() {
        output::println(&format!(
            "Blocked by: {}",
            blockers
                .iter()
                .map(|b| format!("{} ({})", b.short(), b.status))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for a in attempts {
        output::println(&format!(
            "Attempt {}: {} {}{}",
            a.attempt.unwrap_or(0),
            a.label(),
            a.status,
            a.end_reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ));
    }
    print_events(events);
}

fn print_events(events: &[status::EventLine]) {
    for e in events {
        output::println(&format!(
            "  {} {:<10} {:<13} {}",
            e.at, e.actor, e.kind, e.text
        ));
    }
}

fn print_worker(w: &workers::Worker, events: &[status::EventLine]) {
    output::println(&format!(
        "{} {} ({}), persona {}.",
        w.label(),
        w.status,
        w.worker_id,
        w.persona.as_deref().unwrap_or("-")
    ));
    output::println(&format!(
        "Supervisor PID {} (start {}), agent PID {} (start {}), pane {}, host {}.",
        opt(w.supervisor_pid),
        opt(w.supervisor_started),
        opt(w.agent_pid),
        opt(w.agent_started),
        w.pane_id.as_deref().unwrap_or("-"),
        w.host.as_deref().unwrap_or("-")
    ));
    output::println(&format!(
        "Session {}. Transcript {}.",
        w.session_id.as_deref().unwrap_or("-"),
        w.transcript_path.as_deref().unwrap_or("-")
    ));
    output::println(&format!(
        "Last checkin {}. Last progress {} ({}). Stall rounds {}.",
        w.last_checkin_at.as_deref().unwrap_or("-"),
        w.last_progress_at.as_deref().unwrap_or("-"),
        w.last_progress_source.as_deref().unwrap_or("-"),
        w.stall_rounds
    ));
    print_events(events);
}

/// The grace between TERM and KILL: 10 s, or `HORCH_STOP_GRACE_MS`.
pub fn stop_grace() -> std::time::Duration {
    std::env::var("HORCH_STOP_GRACE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(10))
}

/// herdr as the stop's pane closer: 1 pane, by its id.
pub struct HerdrPanes(horch_core::workspace::herdr::Herdr);

impl HerdrPanes {
    pub fn new(ctx: &RuntimeContext) -> HerdrPanes {
        HerdrPanes(horch_core::workspace::herdr::Herdr::with_bin(
            &ctx.bins.harness.herdr,
        ))
    }
}

impl stop::Panes for HerdrPanes {
    fn exists(&self, pane: &str) -> bool {
        self.0.pane_exists(pane)
    }
    fn close(&self, pane: &str) -> Result<()> {
        self.0.pane_close(pane)
    }
}

fn print_stop(r: &stop::StopReport, as_json: bool) -> Result<()> {
    if as_json {
        return json(r);
    }
    output::println(&format!(
        "Stopped {}:",
        r.role.as_deref().unwrap_or(&r.worker_id)
    ));
    for s in &r.steps {
        output::println(&format!(
            "  {:<10} {:<8} {:<5} {}",
            s.target,
            s.pid
                .map(|p| p.to_string())
                .or(s.pane_id.clone())
                .unwrap_or_else(|| "-".into()),
            s.signal.as_deref().unwrap_or("-"),
            s.result
        ));
    }
    Ok(())
}

fn opt(v: Option<i64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn run_cmd(ctx: &RuntimeContext, store: &mut Store, command: RunCommand) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    match command {
        RunCommand::Close { run, abandon } => close_run_cmd(ctx, store, &run, abandon.as_deref()),
        RunCommand::List { json: as_json } => list_runs(store, &root, as_json),
        RunCommand::New(args) => new_run_cmd(ctx, store, &root, args),
    }
}

fn close_run_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    run: &RunArg,
    abandon: Option<&str>,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let closed = store.immediate(|tx| runs::close_run(tx, &r, abandon))?;
    for (task, holder) in &closed.cancelled {
        match holder {
            Some(w) => output::println(&format!(
                "{task} cancelled. Its worker {w} may still run: horch ledger stop {w} \"run abandoned\" --run {}",
                r.slug
            )),
            None => output::println(&format!("{task} cancelled.")),
        }
    }
    if !closed.parked.is_empty() {
        bail!(
            "{}. The open tasks are cancelled; the run stays active",
            runs::parked_message(&r, &closed.parked)
        );
    }
    output::println(&format!("Run {} closed as {}.", r.slug, closed.status));
    Ok(ExitCode::SUCCESS)
}

fn list_runs(store: &Store, root: &Path, as_json: bool) -> Result<ExitCode> {
    let all = runs::of_project(store.conn(), root)?;
    if as_json {
        json(&all)?;
        return Ok(ExitCode::SUCCESS);
    }
    if all.is_empty() {
        output::println("No runs.");
    }
    for r in &all {
        output::println(&format!(
            "{} {} {} -> {} ({})",
            r.slug,
            r.status,
            r.run_branch.as_deref().unwrap_or("-"),
            r.base_branch.as_deref().unwrap_or("-"),
            r.run_id
        ));
    }
    Ok(ExitCode::SUCCESS)
}

fn new_run_cmd(
    ctx: &RuntimeContext,
    store: &mut Store,
    root: &Path,
    a: NewRunArgs,
) -> Result<ExitCode> {
    let steps = parse_steps(&a.steps)?;
    let git = Git::new(&ctx.bins.harness.git);
    let is_repo = git.main_root(root).is_some();
    let (run_branch, base, remote) = default_branches(&git, root, is_repo, &a);
    if is_repo {
        run_branches(&git, root, &run_branch, base.as_deref(), remote.as_deref())?;
    }
    let new = NewRun {
        slug: a.slug.clone(),
        workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
        base_branch: base,
        run_branch: is_repo.then_some(run_branch),
        remote,
        finish_steps: steps,
        thresholds: runs::Thresholds {
            check_every_s: a.check_every,
            stall_rounds: a.stall_rounds,
            heartbeat_every_s: a.heartbeat_every,
            heartbeat_stale_s: a.heartbeat_stale,
            timer_tolerance_s: a.timer_tolerance,
            max_attempts: a.max_attempts,
        },
    };
    let r = insert_run(ctx, store, root, &new)?;
    report_created(&r);
    Ok(ExitCode::SUCCESS)
}

fn parse_steps(steps: &[String]) -> Result<Vec<StepName>> {
    steps
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| StepName::parse(s))
        .collect()
}

/// The run branch, base and remote: as named, else `fleet/<slug>`, the
/// branch checked out, and `origin` when it exists.
fn default_branches(
    git: &Git,
    root: &Path,
    is_repo: bool,
    a: &NewRunArgs,
) -> (String, Option<String>, Option<String>) {
    let run_branch = (a.branch.clone()).unwrap_or_else(|| format!("fleet/{}", a.slug));
    let base = (a.base.clone()).or_else(|| is_repo.then(|| git.current_branch(root)).flatten());
    let remote = (a.remote.clone())
        .or_else(|| (is_repo && git.has_remote(root, "origin")).then(|| "origin".to_string()));
    (run_branch, base, remote)
}

fn report_created(r: &Run) {
    output::println(&format!(
        "Run {} created ({}). Run branch: {}. Finish steps: {}.",
        r.slug,
        r.run_id,
        r.run_branch.as_deref().unwrap_or("(no git repository)"),
        r.finish_steps.join(", ")
    ));
    if r.workspace_id.is_none() {
        eprintln!(
            "horch ledger: warning: run {} has no workspace, because this command ran outside a herdr pane. Wakes go only to an orchestrator that runs a ledger command from its pane. Run horch ledger plan-workers in the orchestrator pane to record its workspace.",
            r.slug
        );
    }
}

/// Create the run branch from `base` when it is missing, refuse an old
/// branch that lacks the base's commits, and push it to the remote.
fn run_branches(
    git: &Git,
    root: &Path,
    run_branch: &str,
    base: Option<&str>,
    remote: Option<&str>,
) -> Result<()> {
    let Some(base) = base else {
        bail!("the project has no branch checked out; name the main branch with --base");
    };
    if !git.branch_exists(root, run_branch) {
        git.run(root, &["branch", run_branch, base])?;
        eprintln!("horch ledger: created run branch {run_branch} from {base}.");
    } else if !git.is_ancestor(root, base, run_branch) {
        // An earlier run's branch: tasks would land on old code.
        bail!(
            "branch {run_branch} exists and lacks commits of {base}. Delete it \
             (git branch -D {run_branch}) or name another with --branch"
        );
    }
    if let Some(remote) = remote {
        let out = git.try_run(root, &["push", "--set-upstream", remote, run_branch])?;
        if !out.ok {
            eprintln!(
                "horch ledger: warning: could not push {run_branch} to {remote}: {}",
                out.stderr
            );
        }
    }
    Ok(())
}

/// Insert the run and the orchestrator's own row: the pane it runs in. It is
/// never watched for progress (section 9).
fn insert_run(ctx: &RuntimeContext, store: &mut Store, root: &Path, new: &NewRun) -> Result<Run> {
    let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
    store.immediate(|tx| {
        let r = runs::create(tx, root, new)?;
        let id = workers::insert(
            tx,
            &NewWorker {
                run_id: r.run_id.clone(),
                kind: Some(WorkerKind::Orchestrator),
                persona: "orchestrator".into(),
                role: Some("orchestrator".into()),
                workspace_id: new.workspace_id.clone(),
                pane_id: pane.clone(),
                ..Default::default()
            },
        )?;
        tx.execute(
            &format!(
                "UPDATE workers SET status = {working}, started_at = spawned_at WHERE worker_id = ?1",
                working = word(WorkerStatus::Working)
            ),
            [&id],
        )?;
        runs::set_orchestrator(tx, &r.run_id, &id)?;
        runs::get(tx, &r.run_id)
    })
}

/// The ticket's first heading, or its file stem.
fn ticket_title(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| map::ticket_facts(&t).title)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "task".into())
        })
}

/// A ticket path as the worker will read it: relative to the project root
/// when it is inside it.
fn ticket_path(root: &Path, file: &Path, cwd: &Path) -> Result<String> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    if !abs.is_file() {
        bail!("ticket file {} does not exist", abs.display());
    }
    let canon = abs.canonicalize()?;
    Ok(canon
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| canon.to_string_lossy().into_owned()))
}

/// The current directory, else the project root.
fn cwd_or_root(ctx: &RuntimeContext, root: &Path) -> PathBuf {
    ctx.paths
        .current_dir()
        .unwrap_or_else(|_| root.to_path_buf())
}

/// What a ticket file says about itself; nothing when it cannot be read.
fn facts_of(root: &Path, file: &str) -> map::TicketFacts {
    std::fs::read_to_string(root.join(file))
        .map(|t| map::ticket_facts(&t))
        .unwrap_or_default()
}

/// The task `add` queues: what a flag names, else what the ticket says.
fn new_task(root: &Path, file: String, facts: map::TicketFacts, a: AddArgs) -> NewTask {
    NewTask {
        title: a.title.unwrap_or_else(|| ticket_title(&root.join(&file))),
        task_file: file,
        issue_url: a.issue,
        spec_ref: a.spec,
        primary: a.primary,
        fallback: a.fallback,
        phase: a.phase,
        parallel: !a.exclusive,
        priority: a.priority,
        wave: None,
        worktree_mode: if a.shared {
            WorktreeMode::Shared
        } else {
            WorktreeMode::Worktree
        },
        land_check: a.check.or(facts.check),
        kind: TaskKind::Work,
        parent_task_id: None,
        blocked_by: a.blocked_by,
        own: if a.own.is_empty() { facts.own } else { a.own },
        no_touch: if a.no_touch.is_empty() {
            facts.no_touch
        } else {
            a.no_touch
        },
    }
}

fn add(ctx: &RuntimeContext, store: &mut Store, a: AddArgs) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let file = ticket_path(&root, &a.file, &cwd_or_root(ctx, &root))?;
    let facts = facts_of(&root, &file);
    let run = resolve_run(ctx, store, &a.run)?;
    let _ = a.parallel;
    let check_timeout = a.check_timeout;
    let new = new_task(&root, file, facts, a);
    let t = store.immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))?;
    if let Some(secs) = check_timeout {
        finish::land::set_check_timeout(
            &tools(ctx).evidence_root,
            &t.task_id,
            std::time::Duration::from_secs(secs),
        )?;
    }
    output::println(&format!(
        "{} queued for {}: {}.",
        t.short(),
        t.claimable_by.as_deref().unwrap_or("?"),
        t.title
    ));
    Ok(ExitCode::SUCCESS)
}

fn add_map(ctx: &RuntimeContext, store: &mut Store, path: &Path, arg: &RunArg) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let path = cwd_or_root(ctx, &root).join(path);
    let parsed = map::read(&path, &root)?;
    let run = resolve_run(ctx, store, arg)?;
    let items: Vec<(String, NewTask)> = parsed.items.into_iter().map(|i| (i.key, i.task)).collect();
    let added = store.immediate(|tx| tasks::add_many(tx, &run, &items, Actor::Orchestrator))?;
    for t in &added {
        output::println(&format!(
            "{} queued for {}: {}.",
            t.short(),
            t.claimable_by.as_deref().unwrap_or("?"),
            t.title
        ));
    }
    output::println(&format!(
        "{} tasks queued from {}.",
        added.len(),
        path.display()
    ));
    Ok(ExitCode::SUCCESS)
}

fn claim(ctx: &RuntimeContext, store: &mut Store, as_json: bool) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let mut outcome = claim_task(store, &tools(ctx), &wid)?;
    if let ClaimOutcome::Claimed(c) = &mut outcome {
        absolute_ticket_if_missing(ctx, store, c);
    }
    if as_json {
        json(&outcome)?;
        return Ok(ExitCode::SUCCESS);
    }
    print_claim(ctx, outcome)
}

/// Claim the next task for `wid`. A shared-checkout task gets its checkout in
/// the same transaction.
fn claim_task(store: &mut Store, tools: &finish::Tools, wid: &str) -> Result<ClaimOutcome> {
    store.immediate(|tx| {
        let o = tasks::claim(tx, wid)?;
        if let ClaimOutcome::Claimed(c) = &o {
            finish::shared_claim(tools, &runs::get(tx, &c.task.run_id)?, &c.task)?;
        }
        Ok(o)
    })
}

/// What a claim prints: the line to send when no task fits, else the task and
/// the project facts.
fn print_claim(ctx: &RuntimeContext, outcome: ClaimOutcome) -> Result<ExitCode> {
    match outcome {
        ClaimOutcome::NoTask { tell, persona, .. } => {
            output::println(&format!(
                "No task for {persona}. Send this line, then end your turn:"
            ));
            output::println(&tell);
        }
        ClaimOutcome::Claimed(c) => {
            output::print(&render_claim(&c));
            let facts =
                horch_core::project_notes::display(&ctx.paths.state_root, &ctx.paths.project()?)?;
            if !facts.is_empty() {
                output::println(&facts);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A ticket queued from an ignored folder is not in the worker's worktree.
/// Then the claim names the ticket by its absolute path in the project root.
/// A tracked ticket keeps its relative path. A shared-checkout task works in
/// the project root, so its relative path always opens.
fn absolute_ticket_if_missing(ctx: &RuntimeContext, store: &Store, c: &mut Claimed) {
    let Some(file) = c.task.task_file.clone() else {
        return;
    };
    if Path::new(&file).is_absolute() || (c.worktree.is_none() && !c.task.uses_worktree()) {
        return;
    }
    let Ok(run) = runs::get(store.conn(), &c.task.run_id) else {
        return;
    };
    let in_root = Path::new(&run.project_root).join(&file);
    if !in_root.is_file() {
        return;
    }
    if !worktree_holds(ctx, c, &run, &file) {
        c.task.task_file = Some(in_root.to_string_lossy().into_owned());
    }
}

/// Whether the worktree of the claim, or the one cut for it, holds `file`.
fn worktree_holds(ctx: &RuntimeContext, c: &Claimed, run: &Run, file: &str) -> bool {
    match (&c.worktree, &run.run_branch) {
        (Some(w), _) => Path::new(w).join(file).is_file(),
        // A new worktree is cut from the run branch: it holds the ticket
        // only when the branch tracks a file (a blob) at that path.
        (None, Some(b)) => Git::new(&ctx.bins.harness.git)
            .try_run(
                Path::new(&run.project_root),
                &["cat-file", "-t", &format!("{b}:{file}")],
            )
            .is_ok_and(|o| o.ok && o.stdout == "blob"),
        (None, None) => true,
    }
}

/// The code-sweep reviewer personas that cannot run here: not in the roster,
/// or their harness CLI is not installed.
pub(super) fn reviewers_down(ctx: &RuntimeContext) -> Vec<(String, String)> {
    let roster = super::load_roster_unwarned(ctx, None).ok();
    ["codex-reviewer", "architect-reviewer"]
        .into_iter()
        .filter_map(|p| {
            let why = match roster.as_ref().and_then(|r| r.get(p)) {
                None => "not in the roster".to_string(),
                Some(t) => {
                    let bin = t.agent.binary(&ctx.bins.harness)?;
                    let found = if bin.is_absolute() {
                        bin.is_file()
                    } else {
                        horch_core::runtime::process::which(
                            ctx.inherited.path.as_deref(),
                            ctx.inherited.pathext.as_deref(),
                            &bin.to_string_lossy(),
                        )
                        .is_some()
                    };
                    if found {
                        return None;
                    }
                    format!("{} is not installed", bin.display())
                }
            };
            Some((p.to_string(), why))
        })
        .collect()
}

/// The finish steps' tools: git, gh, node, the sweep skills.
pub fn tools(ctx: &RuntimeContext) -> finish::Tools {
    let mut roots = vec![
        ctx.paths.home.join(".claude").join("plugins"),
        ctx.paths.data_root.clone(),
    ];
    roots.extend(local_marketplaces(&ctx.paths.data_root));
    finish::Tools {
        git: Git::new(&ctx.bins.harness.git),
        gh: tool_bin("HORCH_GH_BIN", "gh"),
        node: tool_bin("HORCH_NODE_BIN", "node"),
        complexity_cli: complexity_cli(&roots),
        code_sweep_dir: code_sweep_dir(&roots),
        evidence_root: evidence_root(ctx),
        // An absolute shell: a pane's PATH may not reach one.
        shell: PathBuf::from(if cfg!(unix) { "/bin/sh" } else { "sh" }),
        reviewers_down: Vec::new(),
        codex: finish::CodexSandbox {
            bin: ctx.bins.harness.codex.clone(),
            state_root: ctx.paths.state_root.clone(),
            personas: Vec::new(),
        },
    }
}

/// The path in the variable `key`; an empty one counts as unset.
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The program in the variable `key`, else `default`, found on the PATH.
fn tool_bin(key: &str, default: &str) -> PathBuf {
    env_path(key).unwrap_or_else(|| PathBuf::from(default))
}

/// Whether `p` is the folder of the skill `skill`. A plugin holds `<skill>/`;
/// the skill store holds `skills/<skill>/<source>+<hash>/`.
fn is_skill_dir(p: &Path, skill: &str) -> bool {
    p.file_name().is_some_and(|n| n == skill)
        || p.parent()
            .and_then(|d| d.file_name())
            .is_some_and(|n| n == skill)
}

/// The code-sweep skill folder: `HORCH_CODE_SWEEP_DIR`, else the first one
/// under `roots` that holds `reviewers/`.
fn code_sweep_dir(roots: &[PathBuf]) -> Option<PathBuf> {
    env_path("HORCH_CODE_SWEEP_DIR").or_else(|| {
        find_dir(roots, |p| {
            is_skill_dir(p, "code-sweep") && p.join("reviewers").is_dir()
        })
    })
}

/// The complexity analyser: `HORCH_COMPLEXITY_CLI`, else the one in the first
/// complexity-sweep folder under `roots` that holds it.
fn complexity_cli(roots: &[PathBuf]) -> Option<PathBuf> {
    env_path("HORCH_COMPLEXITY_CLI").or_else(|| {
        find_dir(roots, |p| {
            is_skill_dir(p, "complexity-sweep") && p.join("code/cli/dist/index.js").is_file()
        })
        .map(|d| d.join("code/cli/dist/index.js"))
    })
}

/// The folder beside the ledger file that holds the evidence of the steps.
fn evidence_root(ctx: &RuntimeContext) -> PathBuf {
    ctx.paths
        .ledger_db
        .parent()
        .map(|p| p.join("evidence"))
        .unwrap_or_else(|| PathBuf::from("evidence"))
}

/// The roster's Codex teammates: their check runs in a Codex sandbox.
pub(super) fn codex_personas(ctx: &RuntimeContext) -> Vec<String> {
    let Ok(roster) = super::load_roster_unwarned(ctx, None) else {
        return Vec::new();
    };
    roster
        .names()
        .into_iter()
        .filter(|n| {
            roster
                .get(n)
                .is_some_and(|t| t.agent == horch_core::harness::HarnessKind::Codex)
        })
        .map(str::to_string)
        .collect()
}

/// The `plugins/` folder of each local marketplace checkout that the skill
/// store installed a skill from. The store cannot hold complexity-sweep (its
/// npm links are symlinks), but its checkout sits next to code-sweep's.
fn local_marketplaces(data_root: &Path) -> Vec<PathBuf> {
    let lock = horch_core::skills::catalog::marketplace::Store::new(data_root).lock_path();
    let Ok(lock) = horch_core::skills::catalog::marketplace::Lockfile::read(&lock) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for e in &lock.skills {
        let source = Path::new(&e.source);
        if !source.is_absolute() {
            continue;
        }
        if let Some(p) = source
            .ancestors()
            .find(|a| a.file_name().is_some_and(|n| n == "plugins"))
        {
            if !out.iter().any(|o| o == p) {
                out.push(p.to_path_buf());
            }
        }
    }
    out
}

/// The first directory under `roots` (at most 10 levels down) that `want`
/// accepts.
fn find_dir(roots: &[PathBuf], want: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    roots.iter().find_map(|r| walk(r, 0, &want))
}

fn walk(dir: &Path, depth: usize, want: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    if depth > 10 {
        return None;
    }
    let subs = sorted_subdirs(dir)?;
    subs.iter()
        .find(|p| want(p))
        .cloned()
        .or_else(|| subs.into_iter().find_map(|p| walk(&p, depth + 1, want)))
}

/// The directories under `dir`, sorted, but not `node_modules` or `.git`.
fn sorted_subdirs(dir: &Path) -> Option<Vec<PathBuf>> {
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n == "node_modules" || n == ".git")
        })
        .collect();
    subs.sort();
    Some(subs)
}

/// `horch ledger done` (and `horch done` from a ledger worker): record the
/// finish, then (in a pane) report, wake whoever must act on it and close
/// the pane.
pub fn done_cmd(
    ctx: &RuntimeContext,
    mut store: Store,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<()> {
    let wid = worker_id(ctx)?;
    // The report to the orchestrator and the pane close: `horch done`'s
    // machinery, when this runs in a worker pane.
    let in_pane = in_worker_pane(ctx);
    // One push budget for the wakes and the report, inside the agent's
    // command timeout (#67).
    let budget = delivery::start_push_budget(delivery::DONE_PUSH_BUDGET);
    if budget.is_some() && in_pane {
        eprintln!("{}", super::messaging::CLOSING);
    }
    let c = finish::complete(&mut store, &wid, summary, unlanded)?;
    let run = runs::get(store.conn(), &c.run_id)?;
    // The wakes, at most once. In a pane they run after `done` marks the
    // record done and before its report push, in the same budget (#67).
    let woken = std::cell::Cell::new(c.already);
    let wake = || {
        if woken.replace(true) {
            return;
        }
        wake_after_done(ctx, &store, &run, &c);
    };
    output::println(&done_line(&c));
    if !in_pane {
        wake();
        return Ok(());
    }
    let done = super::messaging::done_then(ctx, summary, unlanded, &wake);
    // A `done` that stopped before the record, or a retry, ran no wake.
    wake();
    done
}

/// Whether `horch done`'s report and pane close run: in a herdr pane of a
/// worker with a record and a role.
fn in_worker_pane(ctx: &RuntimeContext) -> bool {
    ctx.herdr.pane.is_some()
        && ctx
            .worker
            .as_ref()
            .is_some_and(|w| w.record_id.is_some() && w.role.is_some())
}

/// The wakes of a finished task: the author of a reviewed task, the tasks a
/// parked one cancels, and the orchestrator.
fn wake_after_done(ctx: &RuntimeContext, store: &Store, run: &Run, c: &finish::Completed) {
    if let Some((parent, Some(role))) = &c.reviews_in_for {
        tell_reviews_in(ctx, store, run, &c.task_id, parent, role);
    }
    if c.unlanded {
        wake_unlanded(ctx, store, run, c);
    }
    super::taskledger_wake::after_done(ctx, store, run);
}

/// The line `done` prints for the finished task.
fn done_line(c: &finish::Completed) -> String {
    let what = if c.unlanded {
        "parked as UNLANDED"
    } else {
        "done"
    };
    format!("{} {what}.", c.task)
}

/// The last review of `parent` is in: tell its author to run finish, and
/// wake the orchestrator when the tell fails.
fn tell_reviews_in(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    review_task_id: &str,
    parent: &str,
    role: &str,
) {
    let line = format!("[horch] NOTE: The reviews of {parent} are in. Run horch ledger finish.");
    // The author waits for this line until it runs finish (#177).
    if let Err(e) = hold_for_finish(store, review_task_id) {
        eprintln!("horch ledger: could not record the wait of {role}: {e:#}");
    }
    let told = match super::messaging::tell_as(ctx, Some("horch".into()), role, &line) {
        Ok(ReportStatus::Delivered) => Ok(()),
        Ok(ReportStatus::Queued(cause)) => Err(anyhow::anyhow!(
            "{cause}; the courier types it when the pane is idle"
        )),
        Err(e) => Err(e),
    };
    if let Err(e) = told {
        eprintln!("horch ledger: could not tell {role}: {e:#}");
        // A retried done skips the tell, so the orchestrator gets it
        // as durable mail too.
        super::taskledger_wake::wake(
            ctx,
            store,
            run,
            &format!(
                "NOTE: The reviews of {parent} are in, but {role} was not told. \
                 Tell {role} to run horch ledger finish."
            ),
        );
    }
}

/// A parked task cancels what waited on it: wake on those tasks.
fn wake_unlanded(ctx: &RuntimeContext, store: &Store, run: &Run, c: &finish::Completed) {
    // The task is already done here, so a read error only costs the wake.
    match tasks::dependents(store.conn(), &c.task_id) {
        Ok(waiting) => {
            super::taskledger_wake::cancelled_dependents(ctx, store, run, &c.task, &waiting)
        }
        Err(e) => eprintln!(
            "horch ledger: could not read the tasks that wait on {}: {e:#}",
            c.task
        ),
    }
}

/// After the last review of a task: the author holds its reviewers wait
/// until it runs `horch ledger finish` ([`workers::hold_for_finish`]).
fn hold_for_finish(store: &Store, review_task_id: &str) -> Result<()> {
    let Some(parent) = tasks::get(store.conn(), review_task_id)?.parent_task_id else {
        return Ok(());
    };
    let Some(author) = tasks::get(store.conn(), &parent)?.current_worker_id else {
        return Ok(());
    };
    workers::hold_for_finish(store.conn(), &author)
}

/// The slug of the active task-ledger run of this project that the caller's
/// herdr workspace owns, if any (#89). Never creates the ledger file.
pub fn active_run_slug(ctx: &RuntimeContext) -> Option<String> {
    if !ctx.paths.ledger_db.exists() {
        return None;
    }
    let workspace = ctx.herdr.workspace.as_ref()?.as_str().to_string();
    let store = taskledger::open(ctx).ok()?;
    let root = project_root(ctx).ok()?;
    runs::of_project(store.conn(), &root)
        .ok()?
        .into_iter()
        .find(|r| r.status == "active" && r.workspace_id.as_deref() == Some(workspace.as_str()))
        .map(|r| r.slug)
}

/// Whether this process is a task-ledger worker that holds (or finished) a
/// task: `horch done` then goes through [`done_cmd`]. A worker whose task
/// was cancelled or taken from it ends with the plain `horch done` (#136).
pub fn is_ledger_worker(ctx: &RuntimeContext) -> bool {
    let Some(wid) = worker_id_opt(ctx) else {
        return false;
    };
    taskledger::open(ctx)
        .ok()
        .and_then(|s| workers::find(s.conn(), &wid).ok().flatten())
        .is_some_and(|w| {
            w.task_id.is_some() && (w.status == "done" || w.status().is_ok_and(|s| s.is_live()))
        })
}

/// The text a worker reads after a claim.
pub fn render_claim(c: &Claimed) -> String {
    let t = &c.task;
    let mut out = format!(
        "Claimed {} (attempt {}): {}\nTicket: {}. Read it in full and follow it exactly.\n",
        t.short(),
        c.attempt,
        t.title,
        t.task_file.as_deref().unwrap_or("-")
    );
    if let Some(p) = &t.phase {
        out.push_str(&format!("Phase: {p}.\n"));
    }
    if !c.own.is_empty() {
        out.push_str(&format!("Own: {}\n", c.own.join(", ")));
    }
    if !c.do_not_touch.is_empty() {
        out.push_str(&format!("Do not touch: {}\n", c.do_not_touch.join(", ")));
    }
    if t.is_review() {
        out.push_str("This is a review task. Write your findings file as the ticket says, then run: horch ledger done \"<summary>\"\n");
        return out;
    }
    render_workplace(c, &mut out);
    if let Some(from) = &c.reopened_from {
        out.push_str(&format!(
            "Reopened from unfinished past work:\n    {from}\nUse what you can of it. Read each earlier transcript in chunks first.\n"
        ));
    }
    render_prior(&c.prior, &mut out);
    out.push_str("Record milestones with: horch ledger progress \"<text>\"\n");
    out.push_str("When the work is complete, run: horch ledger finish\n");
    out
}

/// Where the worker works: an earlier attempt's worktree, a new one, or the
/// shared checkout.
fn render_workplace(c: &Claimed, out: &mut String) {
    let t = &c.task;
    match (&c.worktree, t.uses_worktree()) {
        (Some(path), _) => {
            out.push_str(&format!(
                "Worktree: {path} (branch {}). It holds the earlier attempt's work. Work there.\n",
                c.worktree_branch.as_deref().unwrap_or("?")
            ));
            if let Some(landed) = &c.landed_commit {
                out.push_str(&format!(
                    "Its work already landed at {}. The finish steps left: {}. A new commit is checked, reviewed and landed again.\n",
                    &landed[..landed.len().min(12)],
                    if c.steps_after_land.is_empty() {
                        "none".to_string()
                    } else {
                        c.steps_after_land.join(", ")
                    }
                ));
            }
        }
        (None, true) => out.push_str(&format!(
            "Make your worktree first: horch ledger worktree new {}\n",
            slug(&t.title)
        )),
        (None, false) => out.push_str("Work in the shared checkout.\n"),
    }
}

/// The earlier attempts at the task, oldest first.
fn render_prior(prior: &[tasks::PriorAttempt], out: &mut String) {
    if prior.is_empty() {
        return;
    }
    out.push_str("Earlier attempts, oldest first:\n");
    for p in prior {
        out.push_str(&format!(
            "  Attempt {} by {}: {}{}.\n    Summary: {}\n    Transcript: {}\n",
            p.attempt.unwrap_or(0),
            p.role.as_deref().unwrap_or(&p.worker_id),
            p.status,
            p.end_reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default(),
            p.summary.as_deref().unwrap_or("(none)"),
            p.transcript_path.as_deref().unwrap_or("(unknown)")
        ));
    }
    out.push_str(
        "Use what you can of the earlier work. Before you continue, read each earlier transcript in chunks to learn what went wrong.\n",
    );
}

/// A short branch-safe name from a title.
pub fn slug(title: &str) -> String {
    let mut s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-');
    s.chars()
        .take(40)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn progress(
    ctx: &RuntimeContext,
    store: &mut Store,
    kind: EventKind,
    text: &str,
) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    store.immediate(|tx| workers::progress(tx, &wid, ProgressSource::Agent, kind, text))?;
    if let Err(error) = super::compact::worker_note_context(ctx, text) {
        eprintln!("horch ledger: context check failed: {error:#}");
    }
    Ok(ExitCode::SUCCESS)
}

/// `horch note` and `horch done` also write the ledger when the worker has
/// a row; a failure there never blocks them.
pub fn note_from_worker(ctx: &RuntimeContext, text: &str) {
    let Some(wid) = worker_id_opt(ctx) else {
        return;
    };
    let result = taskledger::open(ctx).and_then(|mut s| {
        if workers::find(s.conn(), &wid)?.is_none() {
            return Ok(());
        }
        s.immediate(|tx| workers::progress(tx, &wid, ProgressSource::Agent, EventKind::Note, text))
    });
    if let Err(e) = result {
        eprintln!("horch: the task ledger did not record the note: {e:#}");
    }
}

fn worker_cmd(ctx: &RuntimeContext, store: &mut Store, command: WorkerCommand) -> Result<ExitCode> {
    match command {
        WorkerCommand::New {
            persona,
            role,
            replaces,
            pane,
            run,
        } => new_worker(ctx, store, persona, role, replaces, pane, &run),
        WorkerCommand::Pid {
            worker,
            supervisor,
            agent,
        } => record_pids(store, &worker, supervisor, agent),
    }
}

/// Insert a worker row and print its id.
fn new_worker(
    ctx: &RuntimeContext,
    store: &mut Store,
    persona: String,
    role: Option<String>,
    replaces: Option<String>,
    pane: Option<String>,
    run: &RunArg,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let id = store.immediate(|tx| {
        workers::insert(
            tx,
            &NewWorker {
                run_id: r.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona,
                role,
                previous_worker_id: replaces,
                pane_id: pane,
                ..Default::default()
            },
        )
    })?;
    output::println(&id);
    Ok(ExitCode::SUCCESS)
}

/// Record the PIDs of a worker's supervisor and agent.
fn record_pids(
    store: &mut Store,
    worker: &str,
    supervisor: Option<u32>,
    agent: Option<u32>,
) -> Result<ExitCode> {
    store.immediate(|tx| {
        if let Some(pid) = supervisor {
            workers::supervisor_started(tx, worker, pid, horch_core::procid::start_time(pid))?;
        }
        if let Some(pid) = agent {
            workers::agent_started(tx, worker, pid, horch_core::procid::start_time(pid))?;
        }
        Ok(ExitCode::SUCCESS)
    })
}

#[cfg(test)]
mod tests {
    use super::{absolute_ticket_if_missing, LedgerCommand};
    use clap::Subcommand;
    use horch_core::roster::{ExecDecision, Roster};
    use horch_core::runtime::{MapEnv, RuntimeContext};
    use horch_core::taskledger::model::{Actor, WorktreeMode};
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::{self, ClaimOutcome, Claimed, NewTask};
    use horch_core::taskledger::workers::{self, NewWorker};
    use horch_core::taskledger::Store;
    use std::path::Path;

    /// The task "t" (ticket `tickets/t.md`, shaped by `shape`) of the run "r1"
    /// of project root `root` (run branch `branch`), claimed by a new worker,
    /// with the ledger and the context in `dir`.
    fn claimed(
        dir: &Path,
        root: &Path,
        branch: Option<&str>,
        shape: impl FnOnce(&mut NewTask),
    ) -> (RuntimeContext, Store, Claimed) {
        let ctx = RuntimeContext::from_env(&MapEnv::new(dir)).unwrap();
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            run_branch: branch.map(str::to_string),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, root, &new_run))
            .unwrap();
        let mut task = NewTask::work("t", "tickets/t.md", "dev");
        shape(&mut task);
        store
            .immediate(|tx| tasks::add(tx, &run, &task, Actor::Orchestrator))
            .unwrap();
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let worker = store.immediate(|tx| workers::insert(tx, &worker)).unwrap();
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| tasks::claim(tx, &worker)).unwrap()
        else {
            panic!("the worker claims the task");
        };
        (ctx, store, *c)
    }

    /// The project root under `dir` (by its canonical path, as a run records
    /// it), with the ticket `tickets/t.md` in it when `ticket` is true.
    fn project(dir: &Path, ticket: bool) -> std::path::PathBuf {
        let root = dir.join("project");
        std::fs::create_dir_all(root.join("tickets")).unwrap();
        if ticket {
            std::fs::write(root.join("tickets/t.md"), "# t\n").unwrap();
        }
        root.canonicalize().unwrap()
    }

    fn ticket_of(c: &Claimed) -> &str {
        c.task.task_file.as_deref().unwrap()
    }

    #[test]
    fn a_claim_keeps_the_ticket_path_when_the_project_root_has_no_such_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let (ctx, store, mut c) = claimed(dir.path(), &root, Some("fleet/r1"), |_| {});
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(ticket_of(&c), "tickets/t.md");
    }

    #[test]
    fn a_claim_whose_run_is_gone_keeps_the_ticket_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), true);
        let (ctx, store, mut c) = claimed(dir.path(), &root, Some("fleet/r1"), |_| {});
        c.task.run_id = "no-such-run".into();
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(ticket_of(&c), "tickets/t.md");
    }

    #[test]
    fn a_shared_task_without_a_worktree_keeps_its_relative_ticket_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), true);
        let (ctx, store, mut c) = claimed(dir.path(), &root, Some("fleet/r1"), |t| {
            t.worktree_mode = WorktreeMode::Shared
        });
        assert!(c.worktree.is_none());
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(ticket_of(&c), "tickets/t.md");
    }

    #[test]
    fn a_claim_with_a_worktree_names_the_ticket_by_the_root_only_when_the_worktree_lacks_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), true);
        let with = dir.path().join("with");
        std::fs::create_dir_all(with.join("tickets")).unwrap();
        std::fs::write(with.join("tickets/t.md"), "# t\n").unwrap();
        let without = dir.path().join("without");
        std::fs::create_dir_all(&without).unwrap();

        let (ctx, store, mut c) = claimed(dir.path(), &root, Some("fleet/r1"), |_| {});
        c.worktree = Some(with.to_string_lossy().into_owned());
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(ticket_of(&c), "tickets/t.md");

        c.worktree = Some(without.to_string_lossy().into_owned());
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(
            Path::new(ticket_of(&c)),
            root.join("tickets/t.md"),
            "the claim names the ticket in the project root"
        );

        // A shared task with a worktree is looked at like any other.
        let other = tempfile::tempdir().unwrap();
        let (ctx, store, mut shared) = claimed(other.path(), &root, Some("fleet/r1"), |t| {
            t.worktree_mode = WorktreeMode::Shared
        });
        shared.worktree = Some(without.to_string_lossy().into_owned());
        absolute_ticket_if_missing(&ctx, &store, &mut shared);
        assert_eq!(Path::new(ticket_of(&shared)), root.join("tickets/t.md"));
    }

    #[test]
    fn a_run_without_a_branch_keeps_the_relative_ticket_path_and_one_with_a_branch_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), true);
        let (ctx, store, mut c) = claimed(dir.path(), &root, None, |_| {});
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(ticket_of(&c), "tickets/t.md", "no branch to cut from");

        // The root is no git repository, so no branch of it holds the ticket.
        let other = tempfile::tempdir().unwrap();
        let (ctx, store, mut c) = claimed(other.path(), &root, Some("fleet/r1"), |_| {});
        absolute_ticket_if_missing(&ctx, &store, &mut c);
        assert_eq!(Path::new(ticket_of(&c)), root.join("tickets/t.md"));
    }

    /// Whether `cmd` or a subcommand under it takes `--check`.
    fn takes_check(cmd: &clap::Command) -> bool {
        cmd.get_arguments().any(|a| a.get_long() == Some("check"))
            || cmd.get_subcommands().any(takes_check)
    }

    /// #181: an allow rule runs a codex worker's `horch ledger <sub>` outside
    /// its sandbox, and `finish` runs a task's check command there. So no
    /// subcommand that takes `--check` may be allowed, and every allow rule
    /// names a real subcommand. Read from the clap definition, so a new
    /// `--check` on an allowed subcommand fails here.
    #[test]
    fn no_allowed_ledger_subcommand_takes_a_check_command() {
        let roster = Roster::builtin().unwrap();
        let ledger = LedgerCommand::augment_subcommands(clap::Command::new("ledger"));
        let tokens = |pattern: &str| -> Vec<String> {
            pattern
                .split(',')
                .map(|t| t.trim().trim_matches('"').to_string())
                .collect()
        };
        let allowed = |sub: &str| {
            let argv = ["horch", "ledger", sub];
            let matching: Vec<ExecDecision> = roster
                .exec_rules()
                .iter()
                .filter(|r| {
                    let p = tokens(&r.pattern);
                    p.len() <= argv.len() && p.iter().zip(argv).all(|(t, a)| t == a)
                })
                .map(|r| r.decision)
                .collect();
            !matching.is_empty() && !matching.contains(&ExecDecision::Forbidden)
        };
        let with_check: Vec<&str> = ledger
            .get_subcommands()
            .filter(|c| takes_check(c))
            .map(|c| c.get_name())
            .collect();
        assert!(with_check.contains(&"add"), "{with_check:?}");
        assert!(with_check.contains(&"reopen"), "{with_check:?}");
        for sub in &with_check {
            assert!(
                !allowed(sub),
                "'horch ledger {sub}' takes --check and is allowed"
            );
        }
        for rule in roster.exec_rules() {
            let p = tokens(&rule.pattern);
            if p.len() > 2 && p[..2] == ["horch", "ledger"] {
                assert!(
                    ledger.find_subcommand(&p[2]).is_some(),
                    "the rule {:?} names no ledger subcommand",
                    rule.pattern
                );
            }
        }
    }
}
