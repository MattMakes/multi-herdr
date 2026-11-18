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
use horch_core::execution::records::SessionLedger;
use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply};
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::task_execution::adapters::outbox_reader::handoff_inside;
use horch_core::fleetd::task_execution::adapters::outbox_writer::OutboxWriter;
use horch_core::fleetd::task_execution::domain::outbox::{handoff_result, LineKind};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::messaging::delivery;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::import;
use horch_core::taskledger::map;
use horch_core::taskledger::model::{EventKind, ProgressSource, StepName, WorkerKind};
use horch_core::taskledger::model::{VerifyProfile, VERIFY_ENV};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::status;
use horch_core::taskledger::stop;
use horch_core::taskledger::tasks;
use horch_core::taskledger::wakes;
use horch_core::taskledger::workers;
use horch_core::taskledger::{self, Store};
use serde::Deserialize;
use serde_json::{json, Value};

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
    /// Queue 1 one-off task in this workspace's solo run
    /// (`solo-<workspace>`). Creates that run with the minimal profile when
    /// it is absent. Prints the spawn line to run next.
    Task(TaskArgs),
    /// Queue every ticket of a ticket map (MAP.md) in 1 transaction.
    /// `--dry-run` checks the map and every ticket against the ticket
    /// format (C1) and writes nothing.
    AddMap {
        map: PathBuf,
        #[command(flatten)]
        run: RunArg,
        /// Check the map and its tickets; queue nothing. Exit 1 on a problem.
        #[arg(long)]
        dry_run: bool,
        /// Queue a ticket again that is already a task in the run.
        #[arg(long)]
        again: bool,
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
    /// Drop a task. Its worker lets go of it at once and is stopped by its
    /// recorded PIDs and pane, as `horch ledger stop` does.
    Cancel {
        task: String,
        reason: String,
        /// Kept so that old command lines still run: a cancel always stops.
        #[arg(long, hide = true)]
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
    /// Queue a held task again (orchestrator). It keeps its attempt
    /// history and gets the run's attempt limit anew. A cancelled task is
    /// terminal: drop each dependency on it (unblock), or cancel the
    /// dependents. Add its ticket again for a replacement task.
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
    /// (worker) Run the run's finish steps in order (under `--verify full`:
    /// check, code-sweep, complexity-sweep, recheck, land, pr, cleanup).
    /// Stops at the first that does not pass; fix it and run finish again.
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
    /// (worker) Hand the running task off: this worker ends `handed_off`,
    /// the task is queued again first with its worktree, and the next claim
    /// reads the handoff file first. Write the file before you run it.
    Handoff { path: PathBuf },
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
    /// fleetd does the watch: it checks every working worker every check
    /// interval, reaps the dead and wakes the orchestrator. With no `--once`
    /// this runs the check of `horch fleetd check`. `--once` runs 1 pass of
    /// fleetd when no fleetd runs (every worker, due or not, with
    /// `--force`), and prints the checks of the run.
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

/// The report request of a ledger command in a `horch_access: report`
/// pane: only `progress`. Every other ledger command is refused there.
pub fn report_request(command: &LedgerCommand) -> Option<horch_core::messaging::report::Request> {
    match command {
        LedgerCommand::Progress { text } => {
            Some(horch_core::messaging::report::Request::Progress { text: text.clone() })
        }
        _ => None,
    }
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
        /// The run that owns the worktree. Default: this worker's run.
        #[command(flatten)]
        run: RunArg,
    },
}

#[derive(Subcommand)]
pub enum RunCommand {
    /// Create a run and its run branch (from the main branch).
    New(NewRunArgs),
    /// Close the run when its work is done. Refuses while the run is not
    /// settled, without a passing gate on the run-branch tip, or while the
    /// run PR is neither merged nor declined.
    Close {
        #[command(flatten)]
        run: RunArg,
        /// Cancel every open task with this reason, then close the run as
        /// abandoned.
        #[arg(long, value_name = "REASON")]
        abandon: Option<String>,
        /// Close without a merged PR, and record why.
        #[arg(long, value_name = "REASON", conflicts_with = "abandon")]
        pr_declined: Option<String>,
    },
    /// Run the run check on the run-branch tip and record the result.
    /// `run close` needs a passing gate on the current tip.
    Gate {
        #[command(flatten)]
        run: RunArg,
        /// The check command; default the run's check (`run new --check`).
        #[arg(long)]
        check: Option<String>,
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
    /// The finish steps, comma-separated. Overrides `--verify`.
    #[arg(long, value_delimiter = ',')]
    steps: Vec<String>,
    /// The verification profile, which picks the finish steps: `full`,
    /// `light` or `minimal`. Default: `HORCH_VERIFY` (set by `horch fleet
    /// --verify`), else `light`.
    #[arg(long, value_name = "PROFILE", conflicts_with = "steps")]
    verify: Option<VerifyProfile>,
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
    /// The run's default check command: the check of each task that names
    /// none. A task with no check, and no `**Check:** none` in its ticket,
    /// does not land.
    #[arg(long, value_name = "CMD")]
    check: Option<String>,
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

/// `horch ledger task`: 1 one-off task (TRF-42).
#[derive(Args)]
pub struct TaskArgs {
    /// The task's title. With no `--file`, the title is the whole ticket.
    title: String,
    /// The persona that claims it.
    #[arg(long)]
    persona: String,
    /// The ticket file the worker reads.
    #[arg(long = "file")]
    file: Option<PathBuf>,
    /// The check the finish steps run. `none` lands the task without a
    /// check. Default: the ticket's `**Check:**`.
    #[arg(long)]
    check: Option<String>,
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

pub(crate) fn worker_id_opt(ctx: &RuntimeContext) -> Option<String> {
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
            | C::Task(_)
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
            | C::Handoff { .. }
            | C::Progress { .. }
            | C::Note { .. }
    ) {
        Group::Worker
    } else {
        Group::Admin
    }
}

pub fn run(ctx: &RuntimeContext, command: LedgerCommand) -> Result<ExitCode> {
    match run_command(ctx, command) {
        Err(e) => match e.downcast_ref::<NotApplied>() {
            Some(NotApplied::Refused(reason)) => {
                eprintln!("refused: {reason}");
                Ok(ExitCode::from(1))
            }
            // The client printed where the request waits.
            Some(NotApplied::Queued) => Ok(ExitCode::SUCCESS),
            None => Err(e),
        },
        done => done,
    }
}

fn run_command(ctx: &RuntimeContext, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Legacy { command } = command {
        return super::sessionscmd::run(ctx, command);
    }
    if let LedgerCommand::Gc = command {
        // The trash refs are git refs, not rows: the request records them.
        let deleted = super::worktree::gc(ctx)?;
        request(ctx, Kind::LedgerGc, json!({ "deleted": deleted }))?;
        return Ok(ExitCode::SUCCESS);
    }
    if let LedgerCommand::Path = command {
        output::println(&ctx.paths.ledger_db.display().to_string());
        return Ok(ExitCode::SUCCESS);
    }
    // A dry run reads only files: it opens no ledger, so it creates and
    // migrates nothing (TRF-11).
    if let LedgerCommand::AddMap {
        map: path,
        dry_run: true,
        ..
    } = &command
    {
        return add_map_dry_run(ctx, path);
    }
    if let Some(code) = worker_files_cmd(ctx, &command)? {
        return Ok(code);
    }
    let store = if reads_only(&command) {
        read_only(ctx)?
    } else {
        taskledger::open(ctx)?
    };
    // One entry per `Group`, in its order.
    let dispatch: [GroupCmd; 4] = [board_cmd, control_cmd, worker_side_cmd, admin_cmd];
    dispatch[group_of(&command) as usize](ctx, store, command)
}

/// Whether `command` changes the ledger only through requests to fleetd,
/// so that it opens the ledger read-only (RR-5). `run close` and
/// `run gate` still open it to write: the custody of the gate folder and
/// the stop of a worker are not their requests.
fn reads_only(command: &LedgerCommand) -> bool {
    use LedgerCommand as C;
    match command {
        C::Run { command } => matches!(command, RunCommand::New(_) | RunCommand::List { .. }),
        C::Import { dry_run, .. } => !dry_run,
        C::Add(_) | C::Task(_) | C::AddMap { .. } | C::PlanWorkers { .. } => true,
        C::Fallback { .. }
        | C::Cancel { .. }
        | C::Unblock { .. }
        | C::Requeue { .. }
        | C::Waive { .. }
        | C::Stop { .. }
        | C::Reap { .. }
        | C::Reopen { .. }
        | C::Dismiss { .. }
        | C::Worker { .. }
        | C::Unfinished { .. } => true,
        C::Worktree { command } => matches!(
            command,
            WorktreeCommand::Scratch { .. } | WorktreeCommand::Drop { .. }
        ),
        _ => false,
    }
}

/// The ledger, opened read-only. fleetd creates it at its start.
fn read_only(ctx: &RuntimeContext) -> Result<Store> {
    taskledger::open_existing_read_only(ctx)?.with_context(|| {
        format!(
            "the task ledger {} does not exist. fleetd creates it at its start: run horch fleetd check",
            ctx.paths.ledger_db.display()
        )
    })
}

/// A request that fleetd did not apply. [`run`] prints a refusal as
/// `refused: <reason>` and exits 1, and exits 0 for a queued request.
#[derive(Debug)]
enum NotApplied {
    Refused(String),
    Queued,
}

impl std::fmt::Display for NotApplied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotApplied::Refused(reason) => write!(f, "refused: {reason}"),
            NotApplied::Queued => {
                f.write_str("the request waits in a request file until fleetd starts")
            }
        }
    }
}

impl std::error::Error for NotApplied {}

/// Send 1 request of `kind` with `payload` to fleetd, and return its result
/// when fleetd applied it. A refused or queued request is a [`NotApplied`]
/// error.
fn request(ctx: &RuntimeContext, kind: Kind, payload: Value) -> Result<Value> {
    let Value::Object(payload) = payload else {
        bail!("the payload of a request is a JSON object");
    };
    match channel_client::send(ctx, kind, payload)? {
        Sent::Replied(Reply {
            outcome: Outcome::Applied(result),
            ..
        }) => Ok(result),
        Sent::Replied(Reply {
            outcome: Outcome::Refused(reason),
            ..
        }) => Err(NotApplied::Refused(reason).into()),
        Sent::Queued { .. } => Err(NotApplied::Queued.into()),
    }
}

/// The field `key` of a request result as `T`.
fn field<T: serde::de::DeserializeOwned>(result: &Value, key: &str) -> Result<T> {
    serde_json::from_value(result.get(key).cloned().unwrap_or(Value::Null))
        .with_context(|| format!("the result of fleetd has no valid `{key}`"))
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
    if let LedgerCommand::Task(args) = command {
        return task_cmd(ctx, &mut store, args);
    }
    if let LedgerCommand::AddMap {
        map: path,
        run,
        dry_run,
        again,
    } = command
    {
        debug_assert!(
            !dry_run,
            "run() answers a dry run before it opens the ledger"
        );
        return add_map(ctx, &mut store, &path, &run, again);
    }
    if let LedgerCommand::PlanWorkers { run, json: as_json } = command {
        return plan_workers_cmd(ctx, &store, &run, as_json);
    }
    if let LedgerCommand::Events { run, json: as_json } = command {
        return events_cmd(ctx, &store, &run, as_json);
    }
    unreachable!("not a board command")
}

/// What `plan-workers` prints of 1 persona of the plan.
#[derive(Deserialize)]
struct PlanLine {
    persona: String,
    spawn: i64,
    startable: i64,
    on_the_way: i64,
    replaces: Vec<String>,
}

/// The orchestrator steers tasks and workers.
fn control_cmd(ctx: &RuntimeContext, store: Store, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Fallback { task, reason, run } = command {
        return fallback_cmd(ctx, &store, &task, &reason, &run);
    }
    if let LedgerCommand::Cancel {
        task, reason, run, ..
    } = command
    {
        return cancel_cmd(ctx, &store, &task, &reason, &run);
    }
    if let LedgerCommand::Unblock { task, from, run } = command {
        return unblock_cmd(ctx, &store, &task, &from, &run);
    }
    if let LedgerCommand::Requeue { task, reason, run } = command {
        return requeue_cmd(ctx, &store, &task, &reason, &run);
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
        return stop_cmd(ctx, &store, &worker, &reason, &run, as_json);
    }
    if let LedgerCommand::Reap {
        worker,
        reason,
        run,
        json: as_json,
    } = command
    {
        return reap_cmd(ctx, &store, &worker, &reason, &run, as_json);
    }
    unreachable!("not a control command")
}

/// What a worker does with its task.
fn worker_side_cmd(
    ctx: &RuntimeContext,
    mut store: Store,
    command: LedgerCommand,
) -> Result<ExitCode> {
    if let LedgerCommand::Worktree { command } = command {
        return worktree_cmd(ctx, &mut store, command);
    }
    if let LedgerCommand::Steps {
        task,
        run,
        json: as_json,
    } = command
    {
        return steps_cmd(ctx, &store, task, &run, as_json);
    }
    unreachable!("worker_files_cmd runs each other worker command")
}

/// The line of `claim`, `worktree new` and a worker's `worktree scratch`:
/// since Phase 4 fleetd pushes each task (`docs/tickets/fleetd-p5/MAP.md`,
/// Q3).
pub const NOT_FOR_A_WORKER: &str =
    "fleetd binds each task and makes its worktree; this command is not for a worker";

/// The line of `done` and `finish` after the write of `result.json`.
pub(super) const FLEETD_FINISHES: &str =
    "fleetd runs the finish steps of this task and reports the result to the orchestrator, as horch ledger done does.";

/// The worker commands of `horch ledger` (RR-5, FR-7, Q3 and Q4 of
/// `docs/tickets/fleetd-p5/MAP.md`). Each one writes the outbox of its own
/// attempt, or is refused, and opens the ledger only to read. `None`:
/// `command` is not one of them.
fn worker_files_cmd(ctx: &RuntimeContext, command: &LedgerCommand) -> Result<Option<ExitCode>> {
    use LedgerCommand as C;
    let code = match command {
        C::Claim { .. }
        | C::Worktree {
            command: WorktreeCommand::New { .. },
        } => not_for_a_worker(),
        C::Worktree {
            command: WorktreeCommand::Scratch { .. },
        } => {
            // The scratch of the operator is a request (ticket 22).
            let in_attempt = read_only(ctx).is_ok_and(|store| own_outbox(ctx, &store).is_ok());
            if !in_attempt {
                return Ok(None);
            }
            not_for_a_worker()
        }
        C::Progress { text } => progress(ctx, LineKind::Progress, text)?,
        C::Note { text } => progress(ctx, LineKind::Note, text)?,
        C::Done { summary, unlanded } => {
            let notes = unlanded
                .as_ref()
                .map(|reason| format!("unlanded: {reason}"));
            write_completed(ctx, &summary.join(" "), notes)?
        }
        C::Finish { .. } => super::taskledger_finish::finish_cmd(ctx)?,
        C::Handoff { path } => handoff(ctx, path)?,
        _ => return Ok(None),
    };
    Ok(Some(code))
}

fn not_for_a_worker() -> ExitCode {
    eprintln!("{NOT_FOR_A_WORKER}");
    ExitCode::from(1)
}

/// The outbox writer of the own attempt of this process: the live attempt
/// whose wrapper is in its parent chain. `store` is the ledger, opened
/// read-only. No environment variable names the attempt.
fn own_outbox(ctx: &RuntimeContext, store: &Store) -> Result<OutboxWriter> {
    let config = config_file::load(&ctx.paths.fleet_root)?;
    OutboxWriter::of_this_process(&ctx.paths.fleet_root, store.conn(), &config, &PlatformFacts)
}

/// Write `result.json` with `status: completed`, `summary` and `notes` to
/// the outbox of the own attempt: `horch ledger done` and `finish`.
pub(super) fn write_completed(
    ctx: &RuntimeContext,
    summary: &str,
    notes: Option<String>,
) -> Result<ExitCode> {
    let out = own_outbox(ctx, &read_only(ctx)?)?;
    let a = out.attempt();
    let path = out.write_result(&ResultFile {
        task: a.task_id.clone(),
        attempt: a.attempt().to_string(),
        status: ResultStatus::Completed,
        summary: summary.to_string(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: notes,
        handoff: None,
    })?;
    output::println(&format!("wrote {}", path.display()));
    output::println(FLEETD_FINISHES);
    Ok(ExitCode::SUCCESS)
}

/// `horch ledger handoff <path>` (Q4): `result.json` with `status: blocked`,
/// `reason: handoff` and the real path of the handoff file. The path must
/// be absolute and inside the task folder or the worktree of the attempt.
/// fleetd checks it again at ingest.
fn handoff(ctx: &RuntimeContext, path: &Path) -> Result<ExitCode> {
    if !path.is_absolute() {
        bail!(
            "the handoff path {} is not absolute: give the absolute path of the handoff file",
            path.display()
        );
    }
    let store = read_only(ctx)?;
    let out = own_outbox(ctx, &store)?;
    let a = out.attempt();
    let mut roots = vec![ctx.paths.fleet_root.join("tasks").join(&a.task_id)];
    if let Some(tree) = trees::of_task(store.conn(), &a.task_id)? {
        roots.push(PathBuf::from(tree.path));
    }
    let file = handoff_inside(&path.to_string_lossy(), &roots).map_err(anyhow::Error::msg)?;
    let summary = format!("I handed off {}. The handoff file is {file}.", a.task_id);
    out.write_result(&handoff_result(&a.task_id, a.attempt(), &summary, &file))?;
    output::println(&format!(
        "{} is handed off. The next attempt reads {file} first.",
        a.task_id
    ));
    Ok(ExitCode::SUCCESS)
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
        return unfinished_cmd(ctx, &store, project.as_deref(), as_json);
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
            reopen_cmd(ctx, &store, finding, ticket, &run)
        };
    }
    if let LedgerCommand::Dismiss {
        finding,
        reason,
        cleanup,
    } = command
    {
        return dismiss_cmd(ctx, finding, &reason, cleanup);
    }
    if let LedgerCommand::Worker { command } = command {
        return worker_cmd(ctx, &store, command);
    }
    if let LedgerCommand::Watch {
        run,
        once,
        force,
        json: as_json,
    } = command
    {
        let r = resolve_run(ctx, &store, &run)?;
        // The pass of fleetd opens the ledger itself.
        drop(store);
        return super::taskledger_watch::watch(ctx, &r, once, force, as_json);
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
    let ws = adopted_workspace(ctx, &r);
    let result = request(
        ctx,
        Kind::PlanWorkers,
        json!({ "run_id": r.run_id, "workspace_id": ws }),
    )?;
    if let (Some(ws), true) = (ws, field::<bool>(&result, "adopted")?) {
        print_adopted(&r, ws);
    }
    if as_json {
        json(&result["plan"])?;
        return Ok(ExitCode::SUCCESS);
    }
    let plan: Vec<PlanLine> = field(&result, "plan")?;
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
    store: &Store,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "task": task, "reason": reason });
    print_lines(&request(ctx, Kind::TaskFallback, run_payload(&r, args))?)?;
    Ok(ExitCode::SUCCESS)
}

/// The payload of a request on the run `r`: the fields of `args`, and the
/// run by its project and its id, as the task handlers of fleetd resolve it.
fn run_payload(r: &Run, mut args: Value) -> Value {
    args["project"] = json!(r.project_root);
    args["run"] = json!(r.run_id);
    args
}

/// Print each line of the result of a request.
fn print_lines(result: &Value) -> Result<()> {
    for line in field::<Vec<String>>(result, "lines")? {
        output::println(&line);
    }
    Ok(())
}

/// `horch ledger cancel` and `horch task cancel`. fleetd cancels the task.
/// With a live attempt, its pass stops the processes of the attempt first
/// and cancels the task when they are gone. A task cancelled at once gets
/// 1 `branch_abandon` request for each open task branch. The pass abandons
/// the branches of a task that it cancels later, and wakes the orchestrator
/// on the tasks that wait for it (the lost wakes of the stalled blocker).
pub(super) fn cancel_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    orchestrator_only(ctx, store, "cancel")?;
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "task": task, "reason": reason });
    let result = request(ctx, Kind::TaskCancel, run_payload(&r, args))?;
    print_lines(&result)?;
    if field::<String>(&result, "status")? == "cancelled" {
        let t = tasks::resolve(store.conn(), &r.run_id, task)?;
        // A cancelled task's branch: its rescue ref, then `abandoned`.
        super::branchcmd::abandon_branches_of(ctx, store, &t);
        print_waiting(&t.short(), &tasks::dependents(store.conn(), &t.task_id)?);
    }
    Ok(ExitCode::SUCCESS)
}

/// How a cancel stops the workers it let go of: herdr and the processes of
/// this machine, or fakes in tests.
pub(super) struct Stopper<'a> {
    panes: &'a dyn stop::Panes,
    pub(super) procs: &'a dyn stop::Processes,
    opts: stop::StopOptions,
}

impl<'a> Stopper<'a> {
    pub(super) fn real(panes: &'a HerdrPanes) -> Stopper<'a> {
        Stopper {
            panes,
            procs: &stop::Os,
            opts: stop::StopOptions {
                grace: stop_grace(),
                ..Default::default()
            },
        }
    }
}

/// Stop the workers that held cancelled tasks, by their recorded PIDs and
/// panes (TRF-36). A worker that cannot be stopped gets a warning, and the
/// cancel stands.
pub(super) fn stop_holders(
    store: &mut Store,
    holders: &[String],
    reason: &str,
    s: &Stopper,
) -> Result<()> {
    for (w, r) in stop::stop_cancelled(store, holders, reason, s.panes, s.procs, &s.opts) {
        match r {
            Ok(report) => print_stop(&report, false)?,
            Err(e) => eprintln!(
                "horch ledger: worker {w} was not stopped: {e:#}. Its process may still run: horch ledger stop {w} \"<reason>\"."
            ),
        }
    }
    Ok(())
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
    store: &Store,
    task: &str,
    from: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    orchestrator_only(ctx, store, "unblock")?;
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "task": task, "from": from });
    print_lines(&request(ctx, Kind::TaskUnblock, run_payload(&r, args))?)?;
    Ok(ExitCode::SUCCESS)
}

/// `horch ledger requeue` and `horch task requeue`.
pub(super) fn requeue_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    orchestrator_only(ctx, store, "requeue")?;
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "task": task, "reason": reason });
    print_lines(&request(ctx, Kind::TaskRequeue, run_payload(&r, args))?)?;
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
    let args = json!({ "task": task, "finding": finding, "reason": reason });
    print_lines(&request(ctx, Kind::LedgerWaive, run_payload(&r, args))?)?;
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
        WorktreeCommand::New { .. } => unreachable!("worker_files_cmd refuses it"),
        WorktreeCommand::Scratch { name } => scratch_cmd(ctx, store, &git, &wid, &name)?,
        WorktreeCommand::Drop {
            name,
            reason,
            keep_branch,
            run,
        } => {
            let run_id = drop_run_id(ctx, store, &wid, &run)?;
            let drop = DropArgs {
                run_id: &run_id,
                name: &name,
                reason: &reason,
                keep_branch,
            };
            drop_worktree_cmd(ctx, store, &git, &drop)?
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The run whose worktree `drop` looks for: `--run`, else the worker's run.
fn drop_run_id(ctx: &RuntimeContext, store: &Store, wid: &str, run: &RunArg) -> Result<String> {
    if named_run(run).is_some() {
        return Ok(resolve_run(ctx, store, run)?.run_id);
    }
    workers::get(store.conn(), wid)?
        .run_id
        .context("worker has no run")
}

/// `horch ledger worktree scratch`: the request with the git facts that
/// fleetd checks. The `worktree_add` job of fleetd makes the tree.
fn scratch_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    git: &Git,
    wid: &str,
    name: &str,
) -> Result<()> {
    taskledger::worktree_ops::valid_name(name)?;
    let w = workers::get(store.conn(), wid)?;
    let run = runs::get(
        store.conn(),
        w.run_id.as_deref().context("worker has no run")?,
    )?;
    let base = run
        .run_branch
        .as_deref()
        .or(run.base_branch.as_deref())
        .context("run has no git base")?;
    if base.starts_with('-') || base.contains(['\n', '\r']) {
        bail!("invalid base ref");
    }
    let root = Path::new(&run.project_root);
    let (path, branch) = taskledger::worktree_ops::tree_names(&run, None, name);
    let result = request(
        ctx,
        Kind::WorktreeScratch,
        json!({
            "worker_id": wid,
            "name": name,
            "base": base,
            "base_commit": git.rev(root, base)?,
            "path_exists": path.exists(),
            "branch_exists": git.branch_exists(root, &branch),
        }),
    )?;
    output::println(&format!("path: {}", field::<String>(&result, "path")?));
    output::println(&format!("branch: {}", field::<String>(&result, "branch")?));
    output::println(
        "Scratch worktree: drop it when it is no longer needed; it does not block completion.",
    );
    Ok(())
}

/// The arguments of `horch ledger worktree drop`.
struct DropArgs<'a> {
    run_id: &'a str,
    name: &'a str,
    reason: &'a str,
    keep_branch: bool,
}

/// `horch ledger worktree drop`: the request with the git facts of the
/// tree. fleetd refuses it as today's drop does, or removes the tree
/// through custody.
fn drop_worktree_cmd(ctx: &RuntimeContext, store: &Store, git: &Git, d: &DropArgs) -> Result<()> {
    let wt = taskledger::worktree_ops::find_in(store.conn(), d.run_id, d.name)?;
    let facts = drop_facts(
        git,
        Path::new(&wt.repo_root),
        Path::new(&wt.path),
        &wt.branch,
    )?;
    request(
        ctx,
        Kind::WorktreeDrop,
        json!({
            "run_id": d.run_id,
            "name": d.name,
            "reason": d.reason,
            "keep_branch": d.keep_branch,
            "facts": facts,
        }),
    )?;
    output::println(&format!("abandoned: {} ({})", wt.path, d.reason));
    Ok(())
}

/// The git facts of the drop of the tree at `path` with the recorded
/// `branch` in the repository `root`. A fact of the folder that cannot be
/// read is `null`, and fleetd refuses the drop on it.
fn drop_facts(git: &Git, root: &Path, path: &Path, branch: &str) -> Result<Value> {
    let path_exists = path.exists();
    let (dirty, head_branch, head_is_recorded) = if path_exists {
        let recorded = git.rev(root, branch).ok();
        let head = git.rev(path, "HEAD").ok();
        (
            git.dirty(path).ok().map(|d| !d.is_empty()),
            git.current_branch(path),
            recorded.zip(head).map(|(a, b)| a == b),
        )
    } else {
        (None, None, None)
    };
    let stash = !git.stashes_on(root, branch)?.is_empty();
    let head = if path_exists {
        git.rev(path, "HEAD")?
    } else {
        git.rev(root, branch)?
    };
    // A local branch or tag, or a remote ref, is a copy of the work. A
    // reflog or a linked HEAD is not.
    let refs = git.run(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let own = format!("refs/heads/{branch}");
    let mut args = vec!["rev-list", head.as_str(), "--not"];
    args.extend(refs.lines().filter(|r| *r != own));
    let unique_commits = !git.run(root, &args)?.is_empty();
    Ok(json!({
        "path_exists": path_exists,
        "dirty": dirty,
        "head_branch": head_branch,
        "head_is_recorded": head_is_recorded,
        "stash": stash,
        "unique_commits": unique_commits,
        "head": head,
    }))
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
    let state_root = &ctx.paths.state_root;
    let report = if dry_run {
        serde_json::to_value(import::import(store, &git, state_root, true)?)?
    } else {
        let facts = import::gather(store.conn(), &git, state_root)?;
        let result = request(
            ctx,
            Kind::LedgerImport,
            json!({ "state_root": state_root, "facts": facts }),
        )?;
        result.get("report").cloned().unwrap_or(Value::Null)
    };
    if as_json {
        json(&report)?;
        return Ok(ExitCode::SUCCESS);
    }
    let r: ImportLines = serde_json::from_value(report)
        .context("the result of fleetd has no valid import report")?;
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

/// What `import` prints of an [`import::ImportReport`].
#[derive(Deserialize)]
struct ImportLines {
    dry_run: bool,
    files: Vec<String>,
    raw_added: i64,
    raw_skipped: i64,
    workers: i64,
    tasks: i64,
    worktrees: i64,
    map_errors: i64,
    findings: std::collections::BTreeMap<String, i64>,
}

fn unfinished_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    project: Option<&str>,
    as_json: bool,
) -> Result<ExitCode> {
    let root = match project {
        Some("all") => None,
        Some(p) => Some(PathBuf::from(p)),
        None => Some(project_root(ctx)?),
    };
    let list = import::unfinished(store.conn(), root.as_deref())?;
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

fn reopen_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    finding: i64,
    ticket: ReopenTicket,
    run: &RunArg,
) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let cwd = ctx.paths.current_dir().unwrap_or_else(|_| root.clone());
    let path = ticket_path(&root, &ticket.file, &cwd)?;
    let r = resolve_run(ctx, store, run)?;
    let args = json!({
        "finding": finding,
        "file": path,
        "title": ticket_title(&root.join(&path)),
        "primary": ticket.primary,
        "fallback": ticket.fallback,
        "check": ticket.check,
    });
    print_lines(&request(ctx, Kind::LedgerReopen, run_payload(&r, args))?)?;
    Ok(ExitCode::SUCCESS)
}

/// `horch ledger dismiss`. With `--cleanup`, fleetd queues the removal of
/// the recorded tree of the finding through custody.
fn dismiss_cmd(
    ctx: &RuntimeContext,
    finding: i64,
    reason: &str,
    cleanup: bool,
) -> Result<ExitCode> {
    let args = json!({ "finding": finding, "reason": reason, "cleanup": cleanup });
    print_lines(&request(ctx, Kind::LedgerDismiss, args)?)?;
    Ok(ExitCode::SUCCESS)
}

fn stop_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    worker: &str,
    reason: &str,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "run_id": r.run_id, "worker": worker, "reason": reason });
    print_ended(&request(ctx, Kind::WorkerStop, args)?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn reap_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    worker: &str,
    reason: &str,
    run: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let r = resolve_run(ctx, store, run)?;
    let args = json!({ "run_id": r.run_id, "worker": worker, "reason": reason });
    let result = request(ctx, Kind::WorkerReap, args)?;
    print_ended(&result, as_json)?;
    if !as_json {
        // The caller is the orchestrator: it reads what to spawn here.
        let id: String = field(&result, "worker_id")?;
        let who: String = field(&result, "worker")?;
        let persona: Option<String> = field(&result, "persona")?;
        let next = wakes::reaped_wake(store.conn(), &who, &id, persona.as_deref());
        output::println(&next.text);
    }
    Ok(ExitCode::SUCCESS)
}

/// What `stop` and `reap` print of the result of fleetd. The pass of
/// fleetd stops the processes of the ended worker row and closes its pane.
fn print_ended(result: &Value, as_json: bool) -> Result<()> {
    if as_json {
        return json(result);
    }
    output::println(&format!(
        "Stopped {}: the next pass of fleetd stops its processes and closes its pane.",
        field::<String>(result, "worker")?
    ));
    if let Some(t) = field::<Option<String>>(result, "requeued")? {
        eprintln!("horch ledger: {t} is queued again.");
    }
    Ok(())
}

/// A run created outside herdr has no workspace (#218). The first
/// `status` or `plan-workers` from a herdr pane that is not a worker's
/// records that pane's workspace on the run, so wakes find its orchestrator.
fn adopt_workspace(ctx: &RuntimeContext, store: &Store, run: &Run) -> Result<()> {
    let Some(ws) = adopted_workspace(ctx, run) else {
        return Ok(());
    };
    let changed = store.conn().execute(
        "UPDATE runs SET workspace_id = ?2 WHERE run_id = ?1 AND workspace_id IS NULL",
        [&run.run_id, ws],
    )?;
    if changed == 1 {
        print_adopted(run, ws);
    }
    Ok(())
}

/// The workspace that `run` adopts: the workspace of the orchestrator pane
/// that runs this command, when `run` has none.
fn adopted_workspace<'a>(ctx: &'a RuntimeContext, run: &Run) -> Option<&'a str> {
    let (None, Some(ws), Some(_pane)) = (&run.workspace_id, &ctx.herdr.workspace, &ctx.herdr.pane)
    else {
        return None;
    };
    let role = ctx.worker.as_ref().and_then(|w| w.role.as_deref());
    if role.is_some_and(|r| r != "orchestrator") {
        return None;
    }
    Some(ws.as_str())
}

fn print_adopted(run: &Run, ws: &str) {
    eprintln!(
        "NOTE: run {} had no workspace. It now has workspace {ws}, so wakes go to the orchestrator of this workspace.",
        run.slug
    );
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
    output::println(&format!("{} {}: {}", t.short(), t.shown_status(), t.title));
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
                .map(|b| format!("{} ({})", b.short(), b.shown_status()))
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
        RunCommand::Close {
            run,
            abandon,
            pr_declined,
        } => {
            let r = resolve_run(ctx, store, &run)?;
            let herdr = HerdrPanes::new(ctx);
            let s = Stopper::real(&herdr);
            close_run_cmd(
                ctx,
                &tools(ctx),
                store,
                &r,
                abandon.as_deref(),
                pr_declined,
                &s,
            )
        }
        RunCommand::Gate { run, check } => {
            let r = resolve_run(ctx, store, &run)?;
            gate_cmd(ctx, &tools(ctx), store, &r, check.as_deref())
        }
        RunCommand::List { json: as_json } => list_runs(store, &root, as_json),
        RunCommand::New(args) => new_run_cmd(ctx, store, &root, args),
    }
}

fn close_run_cmd(
    ctx: &RuntimeContext,
    t: &finish::Tools,
    store: &mut Store,
    r: &Run,
    abandon: Option<&str>,
    pr_declined: Option<String>,
    s: &Stopper,
) -> Result<ExitCode> {
    // C6 first: an unsettled run gets the settled rule's refusal, before gh.
    let settled = status::unsettled(store.conn(), &r.run_id)?.is_empty();
    let guards = match abandon {
        None if settled => Some(guards_json(&close_guards(t, r, pr_declined)?)),
        _ => None,
    };
    let result = request(
        ctx,
        Kind::RunClose,
        json!({ "run_id": r.run_id, "abandon": abandon, "guards": guards }),
    )?;
    let closed: ClosedLines = field(&result, "closed")?;
    for note in &closed.notes {
        output::println(note);
    }
    // The gate folder goes after the close commits: its custody jobs run
    // with no transaction open.
    if closed.parked.is_empty() {
        if let Some(note) = runs::remove_gate_tree(store, t, r)? {
            output::println(&note);
        }
    }
    for (task, _) in &closed.cancelled {
        output::println(&format!("{task} cancelled."));
    }
    let holders: Vec<String> = closed
        .cancelled
        .iter()
        .filter_map(|(_, w)| w.clone())
        .collect();
    let why = format!("run abandoned: {}", abandon.unwrap_or_default());
    stop_holders(store, &holders, &why, s)?;
    if !closed.parked.is_empty() {
        bail!(
            "{}. The open tasks are cancelled; the run stays active",
            runs::parked_message(r, &closed.parked)
        );
    }
    output::println(&format!("Run {} closed as {}.", r.slug, closed.status));
    Ok(ExitCode::SUCCESS)
}

/// The C5 facts `run close` checks: the run-branch tip, and the PR as gh
/// reports it.
fn close_guards(
    t: &finish::Tools,
    r: &Run,
    pr_declined: Option<String>,
) -> Result<runs::CloseGuards> {
    let root = Path::new(&r.project_root);
    let tip = match &r.run_branch {
        Some(b) => Some(t.git.rev(root, b)?),
        None => None,
    };
    let pr = finish::pr::pr_check(&t.gh, &t.git, r)?;
    Ok(runs::CloseGuards {
        tip,
        pr,
        pr_declined: pr_declined.filter(|p| !p.trim().is_empty()),
    })
}

/// What `run close` prints of a [`runs::Closed`].
#[derive(Deserialize)]
struct ClosedLines {
    status: String,
    cancelled: Vec<(String, Option<String>)>,
    parked: Vec<String>,
    notes: Vec<String>,
}

/// The C5 facts of a close in the payload of `run_close`.
fn guards_json(g: &runs::CloseGuards) -> Value {
    let pr = match &g.pr {
        runs::PrCheck::NotApplicable(why) => json!({ "state": "not_applicable", "why": why }),
        runs::PrCheck::Merged(number) => json!({ "state": "merged", "number": number }),
        runs::PrCheck::Unmerged { number, state } => {
            json!({ "state": "unmerged", "number": number, "pr_state": state })
        }
        runs::PrCheck::Missing => json!({ "state": "missing" }),
    };
    json!({ "tip": g.tip, "pr": pr, "pr_declined": g.pr_declined })
}

/// What `run gate` prints of a recorded [`runs::Gate`].
#[derive(Deserialize)]
struct GateLines {
    tip_sha: String,
    exit_code: Option<i64>,
    log_path: Option<String>,
}

impl GateLines {
    fn passed(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// `horch ledger run gate` (C5, TRF-19).
fn gate_cmd(
    ctx: &RuntimeContext,
    t: &finish::Tools,
    store: &mut Store,
    r: &Run,
    check: Option<&str>,
) -> Result<ExitCode> {
    let command = match check.filter(|c| !c.trim().is_empty()) {
        Some(c) => c.to_string(),
        None => finish::default_check(store.conn(), &r.run_id)?.with_context(|| {
            format!(
                "run {} has no check. Pass --check \"<cmd>\", or create the run with horch ledger run new <slug> --check \"<cmd>\"",
                r.slug
            )
        })?,
    };
    let log_dir = t.evidence_root.join("gates").join(&r.run_id);
    output::println(&format!(
        "Run gate: running `{command}` on the tip of {} in {}.",
        r.run_branch.as_deref().unwrap_or("-"),
        runs::gate_dir(r).display()
    ));
    let ran = runs::run_gate(store, t, r, &command, &log_dir)?;
    let result = request(
        ctx,
        Kind::RunGate,
        json!({
            "run_id": ran.run_id,
            "tip_sha": ran.tip_sha,
            "command": ran.command,
            "exit_code": ran.exit_code,
            "log_path": ran.log_path,
            "started_at": ran.started_at,
            "ended_at": ran.ended_at,
        }),
    )?;
    let gate: GateLines = field(&result, "gate")?;
    let log = gate.log_path.as_deref().unwrap_or("(no log)");
    if gate.passed() {
        output::println(&format!("Run gate passed on {}. Log: {log}.", gate.tip_sha));
        return Ok(ExitCode::SUCCESS);
    }
    output::println(&format!(
        "Run gate failed on {}: exit {}. Read {log}.",
        gate.tip_sha,
        gate.exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    Ok(ExitCode::FAILURE)
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
    let env = std::env::var(VERIFY_ENV).ok();
    let steps = run_steps(&a.steps, a.verify, env.as_deref())?;
    let git = Git::new(&ctx.bins.harness.git);
    let is_repo = git.main_root(root).is_some();
    let (run_branch, base, remote) = default_branches(&git, root, is_repo, &a);
    let workspace_id = super::sessionscmd::caller_workspace(ctx);
    // Refuse before the git work, so a refused run leaves no branch (TRF-34).
    let probe = NewRun {
        slug: a.slug.clone(),
        workspace_id: workspace_id.clone(),
        ..Default::default()
    };
    runs::refuse_a_2nd_workspace_run(store.conn(), root, &probe)?;
    if is_repo {
        run_branches(&git, root, &run_branch, base.as_deref(), remote.as_deref())?;
    }
    let new = NewRun {
        slug: a.slug.clone(),
        workspace_id,
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
    let check = a.check.as_deref().filter(|c| !c.trim().is_empty());
    let t = &new.thresholds;
    let result = request(
        ctx,
        Kind::RunNew,
        json!({
            "project_root": root,
            "slug": new.slug,
            "workspace_id": new.workspace_id,
            "base_branch": new.base_branch,
            "run_branch": new.run_branch,
            "remote": new.remote,
            "finish_steps": new.finish_steps.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            "thresholds": {
                "check_every_s": t.check_every_s,
                "stall_rounds": t.stall_rounds,
                "heartbeat_every_s": t.heartbeat_every_s,
                "heartbeat_stale_s": t.heartbeat_stale_s,
                "timer_tolerance_s": t.timer_tolerance_s,
                "max_attempts": t.max_attempts,
            },
            "check": check,
            "seat": seat(ctx),
        }),
    )?;
    report_created(&result_run(store, &result)?);
    Ok(ExitCode::SUCCESS)
}

/// The orchestrator that runs this command, as a request that can create a
/// run names it: its pane, and the harness of its session record.
fn seat(ctx: &RuntimeContext) -> Value {
    json!({
        "pane_id": ctx.herdr.pane.as_ref().map(|p| p.as_str()),
        "harness": own_orchestrator_harness(ctx),
    })
}

/// The run that a request result names, read back from the ledger.
fn result_run(store: &Store, result: &Value) -> Result<Run> {
    let run_id = result
        .pointer("/run/run_id")
        .and_then(Value::as_str)
        .context("the result of fleetd names no run")?;
    runs::get(store.conn(), run_id)
}

/// The finish steps of a new run: `--steps` as given, else the steps of
/// `--verify`, else of `HORCH_VERIFY` (`env`), else of the default profile.
/// A bad `HORCH_VERIFY` is an error, not a silent default: it would change
/// what lands unchecked.
fn run_steps(
    steps: &[String],
    verify: Option<VerifyProfile>,
    env: Option<&str>,
) -> Result<Vec<StepName>> {
    let steps = parse_steps(steps)?;
    if !steps.is_empty() {
        return Ok(steps);
    }
    let profile = match (verify, env.filter(|v| !v.trim().is_empty())) {
        (Some(p), _) => p,
        (None, Some(v)) => v
            .parse()
            .with_context(|| format!("{VERIFY_ENV}='{v}' is not a verification profile"))?,
        (None, None) => VerifyProfile::DEFAULT,
    };
    Ok(profile.steps())
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
    let profile = r
        .steps()
        .ok()
        .and_then(|s| VerifyProfile::of_steps(&s))
        .map(|p| format!(" (verify: {p})"))
        .unwrap_or_default();
    output::println(&format!(
        "Run {} created ({}). Run branch: {}. Finish steps: {}{profile}.",
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
        let out = git.try_network(root, &["push", "--set-upstream", remote, run_branch])?;
        if !out.ok {
            eprintln!(
                "horch ledger: warning: could not push {run_branch} to {remote}: {}",
                out.stderr
            );
        }
    }
    Ok(())
}

/// A harness and its model, as an orchestrator row records them.
type HarnessModel = (Option<String>, Option<String>);

/// The harness and model of the orchestrator that runs this command
/// (report 28 D-c): its session record, which `HORCH_RECORD_ID` names.
/// `None` when no orchestrator record names a harness.
fn own_orchestrator_harness(ctx: &RuntimeContext) -> Option<HarnessModel> {
    let id = ctx.worker.as_ref()?.record_id.as_deref()?;
    let record = horch_core::execution::store::ExecutionStore::open_in(ctx)
        .ok()?
        .get(id)
        .ok()?;
    let nonempty = |s: &str| (!s.is_empty()).then(|| s.to_string());
    (record.is_orchestrator() && !record.agent.is_empty())
        .then(|| (nonempty(&record.agent), nonempty(&record.model)))
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

fn add(ctx: &RuntimeContext, store: &mut Store, a: AddArgs) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let file = ticket_path(&root, &a.file, &cwd_or_root(ctx, &root))?;
    let run = resolve_run(ctx, store, &a.run)?;
    let _ = a.parallel;
    let result = request(
        ctx,
        Kind::TaskAdd,
        json!({
            "run_id": run.run_id,
            "project_root": root,
            "ticket_sha256": file_sha256(&root.join(&file)),
            "task_file": file,
            "title": a.title,
            "issue": a.issue,
            "spec": a.spec,
            "primary": a.primary,
            "fallback": a.fallback,
            "phase": a.phase,
            "exclusive": a.exclusive,
            "priority": a.priority,
            "shared": a.shared,
            "check": a.check,
            "blocked_by": a.blocked_by,
            "own": a.own,
            "no_touch": a.no_touch,
        }),
    )?;
    let t: QueuedTask = field(&result, "task")?;
    if let Some(secs) = a.check_timeout {
        finish::land::set_check_timeout(
            &tools(ctx).evidence_root,
            &t.task_id,
            std::time::Duration::from_secs(secs),
        )?;
    }
    output::println(&format!(
        "{} queued for {}: {}.",
        t.short,
        t.claimable_by.as_deref().unwrap_or("?"),
        t.title
    ));
    Ok(ExitCode::SUCCESS)
}

/// What a command prints of a task that a request queued.
#[derive(Deserialize)]
struct QueuedTask {
    task_id: String,
    short: String,
    claimable_by: Option<String>,
    title: String,
}

/// The sha256 of the ticket file `path`, which a request sends in place of
/// its text (Q15). `None` when the file cannot be read.
fn file_sha256(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| tasks::ticket_sha(&text))
}

fn task_cmd(ctx: &RuntimeContext, store: &mut Store, a: TaskArgs) -> Result<ExitCode> {
    let (run, primary) = queue_one_off_in(ctx, store, a)?;
    output::println(&spawn_line(&primary, &run));
    Ok(ExitCode::SUCCESS)
}

/// `horch ledger task` without its `Run next` line: a caller that spawns
/// the worker itself (the fleet's worktree steward) uses it. It returns the
/// solo run that holds the task.
pub(super) fn queue_one_off(ctx: &RuntimeContext, a: TaskArgs) -> Result<Run> {
    let mut store = read_only(ctx)?;
    queue_one_off_in(ctx, &mut store, a).map(|(run, _)| run)
}

/// Queue the one-off task `a` in the caller's solo run. It returns the run
/// and the task's persona.
fn queue_one_off_in(ctx: &RuntimeContext, store: &mut Store, a: TaskArgs) -> Result<(Run, String)> {
    let root = project_root(ctx)?;
    let Some(workspace) = super::sessionscmd::caller_workspace(ctx) else {
        bail!("horch ledger task needs a herdr workspace. Run it in a herdr pane, or queue the task with horch ledger add --run <slug>");
    };
    if a.title.trim().is_empty() {
        bail!("a task needs a title");
    }
    let file = a
        .file
        .as_deref()
        .map(|f| ticket_path(&root, f, &cwd_or_root(ctx, &root)))
        .transpose()?;
    let solo = solo_branches(ctx, store, &root, &workspace)?;
    let result = request(
        ctx,
        Kind::TaskSolo,
        json!({
            "project_root": root,
            "workspace_id": workspace,
            "title": a.title,
            "persona": a.persona,
            "ticket_sha256": file.as_ref().and_then(|f| file_sha256(&root.join(f))),
            "task_file": file,
            "check": a.check,
            "solo": solo,
            "seat": seat(ctx),
        }),
    )?;
    let run = result_run(store, &result)?;
    if field::<bool>(&result, "created_run")? {
        report_created(&run);
    }
    let t: QueuedTask = field(&result, "task")?;
    let primary: String = field(&result, "primary")?;
    output::println(&format!(
        "{} queued in run {} for {}: {}.",
        t.short, run.slug, primary, t.title
    ));
    if field::<bool>(&result, "no_check")? {
        eprintln!(
            "horch ledger: warning: {} has no check, so it does not land. Queue it with --check \"<cmd>\" to run a check, or with --check none to land it without one.",
            t.short
        );
    }
    Ok((run, primary))
}

/// The line that starts a worker for a one-off task of `run`.
fn spawn_line(persona: &str, run: &Run) -> String {
    format!("Run next: horch spawn {persona} --run {}", run.slug)
}

/// The run branch of a new solo run of `workspace`, made with its branch
/// when the workspace has no active solo run. Empty when it has one.
fn solo_branches(
    ctx: &RuntimeContext,
    store: &Store,
    root: &Path,
    workspace: &str,
) -> Result<Value> {
    if runs::active_solo(store.conn(), root, workspace)?.is_some() {
        return Ok(json!({}));
    }
    let git = Git::new(&ctx.bins.harness.git);
    if git.main_root(root).is_none() {
        return Ok(json!({}));
    }
    let branch = format!("fleet/{}", runs::solo_new_run(workspace).slug);
    let base = git.current_branch(root);
    let remote = git.has_remote(root, "origin").then(|| "origin".to_string());
    run_branches(&git, root, &branch, base.as_deref(), remote.as_deref())?;
    Ok(json!({ "run_branch": branch, "base_branch": base, "remote": remote }))
}

/// `add-map --dry-run`: every problem as `<file>:<line>: <problem>`. It
/// needs no run and writes nothing.
fn add_map_dry_run(ctx: &RuntimeContext, path: &Path) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let path = cwd_or_root(ctx, &root).join(path);
    let (parsed, problems) = map::check(&path, &root)?;
    for p in &problems {
        output::println(&p.to_string());
    }
    if !problems.is_empty() {
        output::println(&format!(
            "{} problems in {}. Nothing was queued.",
            problems.len(),
            path.display()
        ));
        return Ok(ExitCode::from(1));
    }
    output::println(&format!(
        "{} tickets in {} follow the ticket format. Nothing was queued.",
        parsed.items.len(),
        path.display()
    ));
    Ok(ExitCode::SUCCESS)
}

fn add_map(
    ctx: &RuntimeContext,
    store: &mut Store,
    path: &Path,
    arg: &RunArg,
    again: bool,
) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let path = cwd_or_root(ctx, &root).join(path);
    let parsed = map::read(&path, &root)?;
    let run = resolve_run(ctx, store, arg)?;
    let map_sha256 = file_sha256(&path).with_context(|| format!("reading {}", path.display()))?;
    // Each ticket travels as its sha256: fleetd reads the files (Q15).
    let tickets: std::collections::BTreeMap<&str, String> = parsed
        .items
        .iter()
        .map(|i| {
            let text = i.task.ticket_snapshot.as_deref().unwrap_or_default();
            (i.task.task_file.as_str(), tasks::ticket_sha(text))
        })
        .collect();
    let result = request(
        ctx,
        Kind::MapAdd,
        json!({
            "run_id": run.run_id,
            "project_root": root,
            "map_path": path,
            "map_sha256": map_sha256,
            "tickets": tickets,
            "again": again,
        }),
    )?;
    let added: Vec<QueuedTask> = field(&result, "tasks")?;
    for t in &added {
        output::println(&format!(
            "{} queued for {}: {}.",
            t.short,
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

/// The finish steps of a push attempt are fleetd's: its worker runs
/// `horch result`, not `horch ledger finish` or `horch ledger done`. An
/// attempt that began with a claim ends by the old path.
fn refuse_push_attempt(store: &Store, wid: &str) -> Result<()> {
    if tasks::push_attempt_of(store.conn(), wid)?.is_some() {
        bail!("{}", tasks::PUSH_FINISH_REFUSAL);
    }
    Ok(())
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
        shell: PathBuf::from(horch_platform::shell::CHECK_SHELL),
        reviewers_down: Vec::new(),
        codex: finish::CodexSandbox {
            bin: ctx.bins.harness.codex.clone(),
            state_root: ctx.paths.state_root.clone(),
            personas: Vec::new(),
        },
        // The job process `horch job run` of this binary.
        jobs: None,
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
    refuse_push_attempt(&store, &wid)?;
    // The report to the orchestrator and the pane close: `horch done`'s
    // machinery, when this runs in a worker pane.
    let in_pane = in_worker_pane(ctx);
    // One push budget for the wakes and the report, inside the agent's
    // command timeout (#67).
    let budget = delivery::start_push_budget(delivery::done_push_budget(&ctx.settings.faults));
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

/// The wakes of a finished task: the author of a reviewed task, the tasks
/// that wait for a parked one, and the orchestrator.
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

/// The last review of `parent` is in: tell its author to run finish
/// through its keyed `reviews-in` row (TRF-51).
fn tell_reviews_in(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    review_task_id: &str,
    parent: &str,
    role: &str,
) {
    // The author waits for this line until it runs finish (#177).
    if let Err(e) = hold_for_finish(store, review_task_id) {
        eprintln!("horch ledger: could not record the wait of {role}: {e:#}");
    }
    super::taskledger_wake::reviews_in(ctx, store, run, parent, role);
}

/// A parked task is not cancelled, but the tasks that wait for it cannot
/// start until a requeue resumes it: wake on those tasks.
fn wake_unlanded(ctx: &RuntimeContext, store: &Store, run: &Run, c: &finish::Completed) {
    // The task is already done here, so a read error only costs the wake.
    match tasks::dependents(store.conn(), &c.task_id) {
        Ok(waiting) => {
            super::taskledger_wake::stalled_dependents(ctx, store, run, &c.task, &waiting)
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

/// `horch ledger progress` and `horch ledger note`: 1 line of `kind` in
/// `progress.jsonl` of the own attempt, then the session parts of
/// `horch note` ([`super::messaging::note_as`], TRF-45). A failure of a
/// session part is a warning: the line is written.
fn progress(ctx: &RuntimeContext, kind: LineKind, text: &str) -> Result<ExitCode> {
    own_outbox(ctx, &read_only(ctx)?)?.append(kind, &taskledger::util::now(), text)?;
    if let Some(record_id) = ctx.worker.as_ref().and_then(|w| w.record_id.as_deref()) {
        if let Err(e) = SessionLedger::open_in(ctx).and_then(|l| l.note(record_id, text)) {
            eprintln!("horch: the session record did not record the note: {e:#}");
        }
    }
    if let Some(line) = note_handoff_warning(ctx) {
        output::println(&line);
    }
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::compact::worker_note_context(ctx, text)
    }));
    std::panic::set_hook(hook);
    if let Ok(Err(error)) = result {
        eprintln!("horch note: context follow-up: {error:#}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Record `kind` with `text` on this worker's task ledger row. True when it
/// did: a worker without `HORCH_WORKER_ID`, or whose row the ledger does
/// not have, records nothing.
pub fn progress_from_worker(ctx: &RuntimeContext, kind: EventKind, text: &str) -> Result<bool> {
    let Some(wid) = worker_id_opt(ctx) else {
        return Ok(false);
    };
    let mut s = taskledger::open(ctx)?;
    if workers::find(s.conn(), &wid)?.is_none() {
        return Ok(false);
    }
    s.immediate(|tx| workers::progress(tx, &wid, ProgressSource::Agent, kind, text))?;
    Ok(true)
}

/// The 1 warning of `horch note` for a live session in the context state
/// `handoff` (TRF-09): it names `horch ledger handoff`. The row is built as
/// `horch context` builds it. Any read error means no warning.
pub(crate) fn note_handoff_warning(ctx: &RuntimeContext) -> Option<String> {
    use horch_core::execution::records::SessionLedger;
    let record_id = ctx.worker.as_ref()?.record_id.as_deref()?;
    let ledger = SessionLedger::open_in(ctx).ok()?;
    let record = ledger.get(record_id).ok()?;
    if !record.execution_status().is_live() {
        return None;
    }
    let roster = super::load_roster_unwarned(ctx, None)
        .or_else(|_| horch_core::roster::Roster::builtin())
        .ok()?;
    let loc = horch_core::usage::Locations::from_context(ctx);
    let cache = super::context::cache_dir(ctx);
    let src = super::context::Sources {
        ctx,
        ledger: &ledger,
        roster: &roster,
        loc: &loc,
        cache_dir: &cache,
    };
    let workspace = ctx.herdr.workspace.as_ref().map(|w| w.to_string());
    let row = super::context::build_row(&src, record, workspace.as_deref());
    handoff_warning(row.state, &row.record.role)
}

/// The `horch note` line for a session in context state `state`: a line
/// only for `handoff`.
fn handoff_warning(state: horch_core::compaction::policy::RowState, role: &str) -> Option<String> {
    use horch_core::compaction::policy::{self, RowState};
    (state == RowState::Handoff).then(|| {
        let file = policy::handoff_path(role);
        format!(
            "NOTE: Context handoff: {role} is still over its threshold after 1 compaction. \
             Do not wait for a compaction. Write {file}, then run: horch ledger handoff {file}"
        )
    })
}

fn worker_cmd(ctx: &RuntimeContext, store: &Store, command: WorkerCommand) -> Result<ExitCode> {
    match command {
        WorkerCommand::New {
            persona,
            role,
            replaces,
            pane,
            run,
        } => {
            let r = resolve_run(ctx, store, &run)?;
            let args = json!({
                "run_id": r.run_id,
                "persona": persona,
                "role": role,
                "replaces": replaces,
                "pane": pane,
            });
            let result = request(ctx, Kind::WorkerNew, args)?;
            output::println(&field::<String>(&result, "worker_id")?);
        }
        WorkerCommand::Pid {
            worker,
            supervisor,
            agent,
        } => {
            // The start time of each PID, read here as today.
            let pid =
                |pid: u32| json!({ "pid": pid, "started": horch_core::procid::start_time(pid) });
            let args = json!({
                "worker": worker,
                "supervisor": supervisor.map(pid),
                "agent": agent.map(pid),
            });
            request(ctx, Kind::WorkerPid, args)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The job runner of the integration tests of `horch-core`: a unit test is
/// not `horch`, so its custody jobs run in the test process.
#[cfg(test)]
#[path = "../../../horch-core/tests/common/test_jobs.rs"]
mod test_jobs;

#[cfg(test)]
mod tests {
    use super::{finish, handoff_warning, tools, ExitCode, Git, LedgerCommand, Run};
    use clap::Subcommand;
    use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
    use horch_core::fleetd::fleet_runtime::adapters::token_file;
    use horch_core::fleetd::fleet_runtime::application::handlers_run;
    use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
    use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
    use horch_core::roster::{ExecDecision, Roster};
    use horch_core::runtime::{MapEnv, RuntimeContext};
    use horch_core::taskledger::model::Actor;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::stop;
    use horch_core::taskledger::tasks::{self, ClaimOutcome, Claimed, NewTask};
    use horch_core::taskledger::workers::{self, NewWorker};
    use horch_core::taskledger::Store;
    use std::path::Path;

    /// The test process plays fleetd for the ledger of `ctx`: it writes the
    /// `fleetd` row of this process, makes the operator token, and serves
    /// the channel in-process with the handlers of the run and plan kinds.
    /// A command of these tests sends its request to it (ticket 21).
    fn serve(ctx: &RuntimeContext) -> ChannelServer {
        let ledger = &ctx.paths.ledger_db;
        let store = Store::open(ledger).unwrap();
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                rusqlite::params![
                    pid,
                    i64::try_from(start).unwrap(),
                    horch_core::taskledger::util::now()
                ],
            )
            .unwrap();
        token_file::ensure(&ctx.paths.fleet_root).unwrap();
        let mut handlers = Handlers::new();
        handlers_run::register(&mut handlers);
        let conn = rusqlite::Connection::open(ledger).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        ChannelServer::start(&ctx.paths.fleet_root, conn, Generation(1), handlers).unwrap()
    }

    /// A context whose ledger is `dir/ledger.sqlite3`, as the test worlds
    /// open it, so that its fleet root is `dir/fleet`.
    fn fleet_ctx(dir: &Path) -> RuntimeContext {
        let env = MapEnv::new(dir).with("HOME", &dir.to_string_lossy()).with(
            "HORCH_LEDGER_DB",
            &dir.join("ledger.sqlite3").to_string_lossy(),
        );
        RuntimeContext::from_env(&env).unwrap()
    }

    /// The run `slug`.
    fn run_of(store: &Store, slug: &str) -> Run {
        let id: String = store
            .conn()
            .query_row("SELECT run_id FROM runs WHERE slug = ?1", [slug], |r| {
                r.get(0)
            })
            .unwrap();
        runs::get(store.conn(), &id).unwrap()
    }

    /// Report 28 D-c: `horch ledger run new` writes the harness on the
    /// orchestrator row it inserts: from the orchestrator's own session
    /// record, else from the newest orchestrator row of the workspace.
    #[test]
    fn trf_45_run_new_writes_the_orchestrator_harness() {
        use horch_core::execution::legacy::{LedgerRecordV1, KIND_ORCHESTRATOR};
        use horch_core::execution::store::ExecutionStore;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let env = |record: Option<&str>| {
            let mut env = MapEnv::new(&root)
                .with("HOME", &dir.path().to_string_lossy())
                .with(
                    "HORCH_STATE_DIR",
                    &dir.path().join("state").to_string_lossy(),
                )
                .with("HORCH_LEDGER_DB", &db.to_string_lossy())
                .with("HORCH_PROJECT_DIR", &root.to_string_lossy())
                .with("HORCH_WORKSPACE_ID", "w1");
            if let Some(id) = record {
                env = env.with("HORCH_RECORD_ID", id);
            }
            RuntimeContext::from_env(&env).unwrap()
        };
        let ctx = env(Some("orch-1"));
        ExecutionStore::open_in(&ctx)
            .unwrap()
            .insert(LedgerRecordV1 {
                record_id: "orch-1".into(),
                agent: "codex".into(),
                model: "gpt-5.6-sol".into(),
                role: "orchestrator".into(),
                kind: KIND_ORCHESTRATOR.into(),
                ..LedgerRecordV1::default()
            })
            .unwrap();
        let mut store = Store::open(&db).unwrap();
        let _fleetd = serve(&ctx);
        let run = |store: &mut Store, ctx: &RuntimeContext, slug: &str| {
            super::new_run_cmd(ctx, store, &root, new_run_args(slug)).unwrap();
            let r = run_of(store, slug);
            let id = r.orchestrator_worker_id.unwrap();
            let w = workers::get(store.conn(), &id).unwrap();
            store
                .conn()
                .execute(
                    "UPDATE runs SET status = 'done' WHERE run_id = ?1",
                    [&r.run_id],
                )
                .unwrap();
            (w.harness, w.model)
        };
        let own = (Some("codex".to_string()), Some("gpt-5.6-sol".to_string()));
        assert_eq!(run(&mut store, &ctx, "a"), own);
        // No own record: the newest orchestrator row of the workspace.
        assert_eq!(run(&mut store, &env(None), "b"), own);
    }

    #[test]
    fn run_steps_prefer_steps_then_flag_then_env_then_light() {
        use super::run_steps;
        use horch_core::taskledger::model::{StepName, VerifyProfile};
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            run_steps(&words(&["check"]), None, Some("full")).unwrap(),
            vec![StepName::Check]
        );
        assert_eq!(
            run_steps(&[], Some(VerifyProfile::Minimal), Some("full")).unwrap(),
            VerifyProfile::Minimal.steps()
        );
        assert_eq!(
            run_steps(&[], None, Some("full")).unwrap(),
            VerifyProfile::Full.steps()
        );
        assert_eq!(
            run_steps(&[], None, Some(" ")).unwrap(),
            VerifyProfile::Light.steps()
        );
        assert_eq!(
            run_steps(&[], None, None).unwrap(),
            VerifyProfile::Light.steps()
        );
        let err = run_steps(&[], None, Some("strict")).unwrap_err();
        assert!(
            format!("{err:#}").contains("HORCH_VERIFY='strict'"),
            "{err:#}"
        );
    }

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

    #[test]
    fn trf_37_note_warns_once_only_in_the_handoff_state() {
        use horch_core::compaction::policy::RowState;
        let line = handoff_warning(RowState::Handoff, "opus-1").expect("a warning");
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(
            line.contains("horch ledger handoff ai_docs/handoffs/opus-1-whats-next.md"),
            "{line}"
        );
        for s in RowState::ALL
            .into_iter()
            .filter(|s| *s != RowState::Handoff)
        {
            assert_eq!(handoff_warning(s, "opus-1"), None, "{}", s.as_str());
        }
    }

    #[test]
    fn trf_16_run_new_check_sets_the_run_default_check() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            a: super::NewRunArgs,
        }
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let ctx = fleet_ctx(dir.path());
        let _fleetd = serve(&ctx);
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        for (slug, check, want) in [
            ("r1", Some("cargo test"), Some("cargo test")),
            ("r2", Some(" "), None),
            ("r3", None, None),
        ] {
            let mut argv = vec!["run-new", slug];
            if let Some(c) = check {
                argv.extend(["--check", c]);
            }
            let a = Cli::try_parse_from(argv).unwrap().a;
            super::new_run_cmd(&ctx, &mut store, &root, a).unwrap();
            let run = runs::of_project(store.conn(), &root)
                .unwrap()
                .into_iter()
                .find(|r| r.slug == slug)
                .unwrap();
            let got =
                horch_core::taskledger::finish::default_check(store.conn(), &run.run_id).unwrap();
            assert_eq!(got.as_deref(), want, "{slug}");
        }
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

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository whose run branch `fleet/r1` is 1 commit ahead of `main`,
    /// run r1 with remote `origin` and PR #7, and the finish tools with a
    /// fake gh that prints `{"state":"<state>"}`.
    fn gated_world(dir: &Path, state: &str) -> (finish::Tools, Store, Run) {
        let root = dir.join("repo");
        std::fs::create_dir(&root).unwrap();
        git_in(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a"), "a").unwrap();
        git_in(&root, &["add", "a"]);
        git_in(&root, &["commit", "-qm", "a"]);
        git_in(&root, &["checkout", "-q", "-b", "fleet/r1"]);
        std::fs::write(root.join("b"), "b").unwrap();
        git_in(&root, &["add", "b"]);
        git_in(&root, &["commit", "-qm", "b"]);
        git_in(&root, &["checkout", "-q", "main"]);
        let gh = dir.join("gh");
        std::fs::write(
            &gh,
            format!("#!/bin/sh\necho \"$*\" >> \"$0.args\"\necho '{{\"state\":\"{state}\"}}'\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Warm the fake: a new executable's first exec is slow on macOS.
        let _ = std::process::Command::new(&gh).output();
        let ctx = RuntimeContext::from_env(&MapEnv::new(dir)).unwrap();
        let mut t = tools(&ctx);
        t.git = Git::new("git");
        t.gh = gh;
        // A unit test is not `horch`: its jobs run in the test process.
        t.jobs = Some(std::sync::Arc::new(super::test_jobs::TestJobs));
        t.evidence_root = dir.join("evidence");
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                let r = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some("fleet/r1".into()),
                        remote: Some("origin".into()),
                        ..Default::default()
                    },
                )?;
                runs::set_pr(tx, &r.run_id, 7, "https://example.invalid/pull/7")?;
                runs::get(tx, &r.run_id)
            })
            .unwrap();
        (t, store, run)
    }

    fn status_of(store: &Store, run: &Run) -> String {
        runs::get(store.conn(), &run.run_id).unwrap().status
    }

    #[test]
    fn trf_19_run_gate_command_records_a_gate_and_exits_by_its_result() {
        let dir = tempfile::tempdir().unwrap();
        let (t, mut store, run) = gated_world(dir.path(), "MERGED");
        let ctx = fleet_ctx(dir.path());
        let _fleetd = serve(&ctx);
        let err = super::gate_cmd(&ctx, &t, &mut store, &run, None).unwrap_err();
        assert!(err.to_string().contains("has no check"), "{err}");
        finish::set_default_check(store.conn(), &run.run_id, "test -f b").unwrap();
        let code = super::gate_cmd(&ctx, &t, &mut store, &run, None).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        let code = super::gate_cmd(&ctx, &t, &mut store, &run, Some("exit 4")).unwrap();
        assert_eq!(code, ExitCode::FAILURE);
        let rows: String = store
            .conn()
            .query_row(
                "SELECT group_concat(command || '=' || exit_code, ';') FROM
                    (SELECT command, exit_code FROM run_gates ORDER BY gate_id)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, "test -f b=0;exit 4=4");
    }

    #[test]
    fn trf_20_run_close_asks_gh_and_refuses_an_open_pr() {
        let dir = tempfile::tempdir().unwrap();
        let (t, mut store, run) = gated_world(dir.path(), "OPEN");
        let ctx = fleet_ctx(dir.path());
        let _fleetd = serve(&ctx);
        let err =
            super::close_run_cmd(&ctx, &t, &mut store, &run, None, None, &no_stops()).unwrap_err();
        assert!(err.to_string().contains("no gate on its tip"), "{err}");
        assert_eq!(
            super::gate_cmd(&ctx, &t, &mut store, &run, Some("true")).unwrap(),
            ExitCode::SUCCESS
        );
        let err =
            super::close_run_cmd(&ctx, &t, &mut store, &run, None, None, &no_stops()).unwrap_err();
        assert!(
            err.to_string().contains("PR #7 is open, not merged"),
            "{err}"
        );
        let args = std::fs::read_to_string(dir.path().join("gh.args")).unwrap();
        assert!(args.contains("pr view 7 --json state"), "{args}");
        assert_eq!(status_of(&store, &run), "active");

        let code = super::close_run_cmd(
            &ctx,
            &t,
            &mut store,
            &run,
            None,
            Some("not wanted".into()),
            &no_stops(),
        );
        assert_eq!(code.unwrap(), ExitCode::SUCCESS);
        assert_eq!(status_of(&store, &run), "done");
    }

    #[test]
    fn trf_20_run_close_passes_with_a_merged_pr_and_without_gh() {
        let dir = tempfile::tempdir().unwrap();
        let (t, mut store, run) = gated_world(dir.path(), "MERGED");
        let ctx = fleet_ctx(dir.path());
        let _fleetd = serve(&ctx);
        super::gate_cmd(&ctx, &t, &mut store, &run, Some("true")).unwrap();
        super::close_run_cmd(&ctx, &t, &mut store, &run, None, None, &no_stops()).unwrap();
        assert_eq!(status_of(&store, &run), "done");

        // No gh: the PR condition is not applicable.
        let dir = tempfile::tempdir().unwrap();
        let (mut t, mut store, run) = gated_world(dir.path(), "OPEN");
        let ctx = fleet_ctx(dir.path());
        let _fleetd = serve(&ctx);
        t.gh = dir.path().join("no-such-gh");
        super::gate_cmd(&ctx, &t, &mut store, &run, Some("true")).unwrap();
        let pr = finish::pr::pr_check(&t.gh, &t.git, &run).unwrap();
        assert!(
            matches!(&pr, runs::PrCheck::NotApplicable(w) if w.contains("gh is not installed")),
            "{pr:?}"
        );
        super::close_run_cmd(&ctx, &t, &mut store, &run, None, None, &no_stops()).unwrap();
        assert_eq!(status_of(&store, &run), "done");
    }

    /// Processes that each run with start time 7 and end on TERM. Each
    /// signal is recorded with its PID.
    #[derive(Default)]
    struct FakeProcs {
        gone: std::cell::RefCell<Vec<u32>>,
        sent: std::cell::RefCell<Vec<(u32, horch_core::procid::Stop)>>,
    }

    impl stop::Processes for FakeProcs {
        fn is_same(&self, pid: u32, started: u64) -> bool {
            started == 7 && !self.gone.borrow().contains(&pid)
        }
        fn exists(&self, pid: u32) -> bool {
            !self.gone.borrow().contains(&pid)
        }
        fn stop_same(&self, pid: u32, started: u64, how: horch_core::procid::Stop) -> bool {
            if !self.is_same(pid, started) {
                return false;
            }
            self.sent.borrow_mut().push((pid, how));
            self.gone.borrow_mut().push(pid);
            true
        }
    }

    /// No process and no pane exists.
    struct Nothing;

    impl stop::Processes for Nothing {
        fn is_same(&self, _: u32, _: u64) -> bool {
            false
        }
        fn exists(&self, _: u32) -> bool {
            false
        }
        fn stop_same(&self, _: u32, _: u64, _: horch_core::procid::Stop) -> bool {
            false
        }
    }

    impl stop::Panes for Nothing {
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn close(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn fast() -> stop::StopOptions {
        stop::StopOptions {
            grace: std::time::Duration::from_millis(30),
            poll: std::time::Duration::from_millis(5),
            ..Default::default()
        }
    }

    /// A stopper that finds nothing to stop.
    fn no_stops() -> super::Stopper<'static> {
        super::Stopper {
            panes: &Nothing,
            procs: &Nothing,
            opts: fast(),
        }
    }

    fn stopper(procs: &FakeProcs) -> super::Stopper<'_> {
        super::Stopper {
            panes: &Nothing,
            procs,
            opts: fast(),
        }
    }

    /// The task "t" of run "r1", claimed by a worker whose agent (PID 4000)
    /// and supervisor (PID 4001) run with start time 7.
    fn running(dir: &Path) -> (RuntimeContext, Store, Run, Claimed) {
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let (ctx, store, c) = claimed(dir, &root, None, |_| {});
        store
            .conn()
            .execute(
                "UPDATE workers SET agent_pid = 4000, agent_started = 7,
                    supervisor_pid = 4001, supervisor_started = 7",
                [],
            )
            .unwrap();
        let run = runs::get(store.conn(), &c.task.run_id).unwrap();
        (ctx, store, run, c)
    }

    fn requeues(store: &Store) -> i64 {
        store
            .conn()
            .query_row(
                &format!(
                    "SELECT count(*) FROM events WHERE {}",
                    horch_core::taskledger::util::kind_is("?1")
                ),
                ["requeued"],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn term_to_both() -> Vec<(u32, horch_core::procid::Stop)> {
        use horch_core::procid::Stop;
        vec![(4000, Stop::Term), (4001, Stop::Term)]
    }

    #[test]
    fn trf_36_run_close_abandon_signals_the_recorded_pids() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, mut store, run, c) = running(dir.path());
        let fleet = fleet_ctx(dir.path());
        let _fleetd = serve(&fleet);
        let procs = FakeProcs::default();
        let code = super::close_run_cmd(
            &fleet,
            &tools(&ctx),
            &mut store,
            &run,
            Some("gave up"),
            None,
            &stopper(&procs),
        )
        .unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(*procs.sent.borrow(), term_to_both());
        assert_eq!(status_of(&store, &run), "abandoned");
        assert_eq!(
            tasks::get(store.conn(), &c.task.task_id).unwrap().status,
            "cancelled"
        );
        assert_eq!(requeues(&store), 0);
    }

    /// The context of a caller in herdr workspace `workspace` (none: outside
    /// herdr) whose project is `root`, and the ledger in `dir`.
    fn task_world(dir: &Path, root: &Path, workspace: Option<&str>) -> (RuntimeContext, Store) {
        let mut env = MapEnv::new(root)
            .with("HORCH_PROJECT_DIR", root.to_str().unwrap())
            .with("HOME", &dir.to_string_lossy())
            .with(
                "HORCH_LEDGER_DB",
                &dir.join("ledger.sqlite3").to_string_lossy(),
            );
        if let Some(ws) = workspace {
            env = env.with("HORCH_WORKSPACE_ID", ws);
        }
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        (ctx, store)
    }

    fn task_args(title: &str, file: Option<&Path>, check: Option<&str>) -> super::TaskArgs {
        super::TaskArgs {
            title: title.into(),
            persona: "opus".into(),
            file: file.map(Path::to_path_buf),
            check: check.map(str::to_string),
        }
    }

    #[test]
    fn trf_42_task_creates_the_solo_run_once_and_queues_each_task_in_it() {
        use horch_core::taskledger::model::VerifyProfile;
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let (ctx, mut store) = task_world(dir.path(), &root, Some("w7"));
        let _fleetd = serve(&ctx);
        super::task_cmd(
            &ctx,
            &mut store,
            task_args("fix a typo", None, Some("true")),
        )
        .unwrap();
        super::task_cmd(&ctx, &mut store, task_args("bump a dep", None, None)).unwrap();
        let all = runs::of_project(store.conn(), &root).unwrap();
        assert_eq!(all.len(), 1, "1 solo run: {all:?}");
        let run = &all[0];
        assert_eq!(run.slug, "solo-w7");
        assert_eq!(run.workspace_id.as_deref(), Some("w7"));
        assert_eq!(run.steps().unwrap(), VerifyProfile::Minimal.steps());
        let queued = tasks::of_run(store.conn(), &run.run_id).unwrap();
        let titles: Vec<&str> = queued.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["fix a typo", "bump a dep"]);
        let first = &queued[0];
        assert_eq!(first.claimable_by.as_deref(), Some("opus"));
        assert_eq!(first.land_check.as_deref(), Some("true"));
        assert_eq!(first.task_file.as_deref(), Some(tasks::ONE_OFF_TICKET));
        assert_eq!(first.ticket_snapshot.as_deref(), Some("# fix a typo\n"));
        assert_eq!(
            super::spawn_line("opus", run),
            "Run next: horch spawn opus --run solo-w7"
        );
    }

    #[test]
    fn trf_42_task_with_a_file_takes_the_tickets_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        std::fs::write(
            root.join("tickets/t.md"),
            "# t\n\n**Check:** `cargo test`\n",
        )
        .unwrap();
        let (ctx, mut store) = task_world(dir.path(), &root, Some("w7"));
        let _fleetd = serve(&ctx);
        let file = root.join("tickets/t.md");
        super::task_cmd(&ctx, &mut store, task_args("t", Some(&file), None)).unwrap();
        let run = runs::active_solo(store.conn(), &root, "w7")
            .unwrap()
            .unwrap();
        let t = &tasks::of_run(store.conn(), &run.run_id).unwrap()[0];
        assert_eq!(t.task_file.as_deref(), Some("tickets/t.md"));
        assert_eq!(t.land_check.as_deref(), Some("cargo test"));
        assert_eq!(t.ticket_snapshot, None);
    }

    /// `horch ledger run new <slug>` as the CLI parses it.
    fn new_run_args(slug: &str) -> super::NewRunArgs {
        #[derive(clap::Parser)]
        struct P {
            #[command(flatten)]
            a: super::NewRunArgs,
        }
        <P as clap::Parser>::parse_from(["run-new", slug]).a
    }

    #[test]
    fn trf_34_run_new_refuses_a_2nd_active_run_in_a_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let (ctx, mut store) = task_world(dir.path(), &root, Some("w7"));
        let _fleetd = serve(&ctx);
        super::new_run_cmd(&ctx, &mut store, &root, new_run_args("plan")).unwrap();
        let err = super::new_run_cmd(&ctx, &mut store, &root, new_run_args("next"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("active run plan"), "{err}");
        assert!(err.contains("horch ledger run close --run plan"), "{err}");
        assert!(err.contains("another workspace"), "{err}");
        let slugs: Vec<String> = runs::of_project(store.conn(), &root)
            .unwrap()
            .into_iter()
            .map(|r| r.slug)
            .collect();
        assert_eq!(slugs, ["plan"]);
        // Another workspace may create its run.
        let (other, mut store) = task_world(dir.path(), &root, Some("w8"));
        super::new_run_cmd(&other, &mut store, &root, new_run_args("next")).unwrap();
    }

    #[test]
    fn trf_34_run_new_ignores_solo_and_workspaceless_runs() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let (outside, mut store) = task_world(dir.path(), &root, None);
        let _fleetd = serve(&outside);
        super::new_run_cmd(&outside, &mut store, &root, new_run_args("loose")).unwrap();
        super::new_run_cmd(&outside, &mut store, &root, new_run_args("loose2")).unwrap();
        let (ctx, mut store) = task_world(dir.path(), &root, Some("w7"));
        super::task_cmd(&ctx, &mut store, task_args("t", None, None)).unwrap();
        super::new_run_cmd(&ctx, &mut store, &root, new_run_args("plan")).unwrap();
        // The solo run still takes one-off tasks next to the planned run.
        super::task_cmd(&ctx, &mut store, task_args("t2", None, None)).unwrap();
        assert_eq!(runs::of_project(store.conn(), &root).unwrap().len(), 4);
    }

    #[test]
    fn trf_42_task_outside_a_workspace_queues_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path(), false);
        let (ctx, mut store) = task_world(dir.path(), &root, None);
        let err = super::task_cmd(&ctx, &mut store, task_args("t", None, None)).unwrap_err();
        assert!(format!("{err:#}").contains("herdr workspace"), "{err:#}");
        assert!(runs::of_project(store.conn(), &root).unwrap().is_empty());
    }

    /// The land step of task `title` of the solo run of workspace w7, claimed
    /// by a new worker, with the checkout `root` on the run branch.
    fn land_one_off(
        dir: &Path,
        ctx: &RuntimeContext,
        store: &mut Store,
        root: &Path,
    ) -> (finish::Outcome, finish::Outcome) {
        use horch_core::taskledger::model::StepName;
        let run = runs::active_solo(store.conn(), root, "w7")
            .unwrap()
            .unwrap();
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "opus".into(),
            ..Default::default()
        };
        let worker = store.immediate(|tx| workers::insert(tx, &worker)).unwrap();
        let ClaimOutcome::Claimed(c) = store.immediate(|tx| tasks::claim(tx, &worker)).unwrap()
        else {
            panic!("the worker claims the one-off");
        };
        let worker = workers::get(store.conn(), &worker).unwrap();
        let mut t = tools(ctx);
        t.git = Git::new("git");
        t.evidence_root = dir.join("evidence");
        let evidence_dir = dir.join("evidence").join(&c.task.task_id);
        std::fs::create_dir_all(&evidence_dir).unwrap();
        let cx = finish::Ctx {
            tools: &t,
            run: &run,
            task: &c.task,
            worker: &worker,
            worktree: None,
            dir: root.to_path_buf(),
            root: root.to_path_buf(),
            evidence_dir,
            seq: 1,
            default_check: None,
        };
        let check = finish::land::check(&cx, StepName::Check).unwrap();
        let land = finish::land::land(store, &cx).unwrap();
        (check, land)
    }

    #[test]
    fn trf_42_a_one_off_with_check_none_lands() {
        use horch_core::taskledger::model::StepStatus;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git_in(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a"), "a").unwrap();
        git_in(&root, &["add", "a"]);
        git_in(&root, &["commit", "-qm", "a"]);
        let root = root.canonicalize().unwrap();
        let (ctx, mut store) = task_world(dir.path(), &root, Some("w7"));
        let _fleetd = serve(&ctx);
        super::task_cmd(&ctx, &mut store, task_args("no check", None, None)).unwrap();
        super::task_cmd(&ctx, &mut store, task_args("tidy", None, Some(" none "))).unwrap();
        let run = runs::active_solo(store.conn(), &root, "w7")
            .unwrap()
            .unwrap();
        let queued = tasks::of_run(store.conn(), &run.run_id).unwrap();
        assert!(!queued[0].check_none);
        assert!(queued[1].check_none, "--check none sets check_none");
        assert_eq!(queued[1].land_check, None, "none is not a command");
        git_in(&root, &["checkout", "-q", "fleet/solo-w7"]);

        let (check, land) = land_one_off(dir.path(), &ctx, &mut store, &root);
        assert_eq!(check.status, StepStatus::Failed, "no check: {check:?}");
        assert_eq!(land.status, StepStatus::Failed, "no check: {land:?}");

        let (check, land) = land_one_off(dir.path(), &ctx, &mut store, &root);
        assert_eq!(check.status, StepStatus::NotApplicable, "{check:?}");
        // A worktree task with no worktree changed no file: nothing lands (T-5).
        assert_eq!(land.status, StepStatus::NotApplicable, "{land:?}");
    }
}
