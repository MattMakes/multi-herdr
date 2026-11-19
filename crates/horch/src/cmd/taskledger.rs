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
use horch_core::fleetd::fleet_runtime::domain::requests::{retired, Kind, Retired};
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
use horch_core::taskledger::model::{EventKind, ProgressSource, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::status;
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
    /// Runs: `run list` lists the runs of this project.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Retired: `horch idea add --direct <ticket file>`.
    #[command(hide = true)]
    Add(RetiredArgs),
    /// Retired: `horch idea add --direct <ticket file>`.
    #[command(hide = true)]
    Task(RetiredArgs),
    /// `--dry-run` checks the map and every ticket against the ticket
    /// format (C1) and writes nothing. Without `--dry-run` it is retired:
    /// `horch idea add`.
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
    /// Retired: fleetd dispatches each ready task.
    #[command(hide = true)]
    PlanWorkers(RetiredArgs),
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
    /// Retired: `horch idea add "<idea>"`.
    #[command(hide = true)]
    New(RetiredArgs),
    /// Retired: merge the pull request of the feature.
    #[command(hide = true)]
    Close(RetiredArgs),
    /// Retired: fleetd runs the full gate of a feature.
    #[command(hide = true)]
    Gate(RetiredArgs),
    /// Every run of this project.
    List {
        #[arg(long)]
        json: bool,
    },
}

/// The arguments of a retired command: any, so each old call prints the
/// line of its new command (ticket 47 of fleetd-p6).
#[derive(Args)]
pub struct RetiredArgs {
    #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    args: Vec<String>,
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
        C::Status { .. } | C::Show { .. } | C::Run { .. } | C::Events { .. }
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

/// The retired kind of `command` when it is a retired command (ticket 47
/// of fleetd-p6).
fn retired_command(command: &LedgerCommand) -> Option<&'static Retired> {
    use LedgerCommand as C;
    let kind = match command {
        C::Run {
            command: RunCommand::New(_),
        } => "run_new",
        C::Run {
            command: RunCommand::Close(_),
        } => "run_close",
        C::Run {
            command: RunCommand::Gate(_),
        } => "run_gate",
        C::Add(_) => "task_add",
        C::AddMap { dry_run: false, .. } => "map_add",
        C::Task(_) => "task_solo",
        C::PlanWorkers(_) => "plan_workers",
        _ => return None,
    };
    retired(kind)
}

fn run_command(ctx: &RuntimeContext, command: LedgerCommand) -> Result<ExitCode> {
    if let Some(r) = retired_command(&command) {
        eprintln!("{}", r.line());
        return Ok(ExitCode::from(1));
    }
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
    // Each command opens the ledger read-only (RR-5): a command that changes
    // state sends a request to fleetd (T-885).
    let store = read_only(ctx)?;
    // One entry per `Group`, in its order.
    let dispatch: [GroupCmd; 4] = [board_cmd, control_cmd, worker_side_cmd, admin_cmd];
    dispatch[group_of(&command) as usize](ctx, store, command)
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
    if let LedgerCommand::Events { run, json: as_json } = command {
        return events_cmd(ctx, &store, &run, as_json);
    }
    unreachable!("not a board command")
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
fn admin_cmd(ctx: &RuntimeContext, store: Store, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Import {
        dry_run,
        json: as_json,
    } = command
    {
        return import_cmd(ctx, &store, dry_run, as_json);
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
    store: &Store,
    dry_run: bool,
    as_json: bool,
) -> Result<ExitCode> {
    let git = Git::new(&ctx.bins.harness.git);
    let state_root = &ctx.paths.state_root;
    // A dry run is a request too: fleetd rolls its rows back (T-885).
    let facts = import::gather(store.conn(), &git, state_root)?;
    let result = request(
        ctx,
        Kind::LedgerImport,
        json!({ "state_root": state_root, "facts": facts, "dry_run": dry_run }),
    )?;
    let report = result.get("report").cloned().unwrap_or(Value::Null);
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
fn adopt_workspace(ctx: &RuntimeContext, run: &Run) -> Result<()> {
    let Some(ws) = adopted_workspace(ctx, run) else {
        return Ok(());
    };
    // fleetd writes the workspace: 1 `run_adopt` request (T-885).
    let payload = json!({ "run_id": run.run_id, "workspace": ws });
    let adopted = match request(ctx, Kind::RunAdopt, payload) {
        Ok(result) => result["adopted"].as_bool() == Some(true),
        // The status is a read: a request that fails is a warning.
        Err(e) => {
            eprintln!(
                "horch: run {} did not adopt workspace {ws}: {e:#}",
                run.slug
            );
            false
        }
    };
    if adopted {
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
    adopt_workspace(ctx, &run)?;
    // The pool of each persona: a ready task of a paced pool shows its hold.
    let roster = super::load_roster_unwarned(ctx, None)
        .or_else(|_| horch_core::roster::Roster::builtin())
        .ok();
    let pool_of = |persona: &str| {
        let t = roster.as_ref()?.get(persona)?;
        let model = t.model.as_deref().unwrap_or_default();
        Some(horch_core::routing::quota::pool_for(t.agent.as_str(), model).to_string())
    };
    let report = status::report_with(store.conn(), &run, &pool_of)?;
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

fn opt(v: Option<i64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn run_cmd(ctx: &RuntimeContext, store: &mut Store, command: RunCommand) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    match command {
        RunCommand::List { json: as_json } => list_runs(store, &root, as_json),
        RunCommand::New(_) | RunCommand::Close(_) | RunCommand::Gate(_) => {
            unreachable!("run_command answers a retired command first")
        }
    }
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

/// `horch ledger task` without its `Run next` line, for the fleet's
/// worktree steward. The kind `task_solo` is retired (ticket 47 of
/// fleetd-p6), so it fails with the line of its new command.
pub(super) fn queue_one_off(_ctx: &RuntimeContext, _a: TaskArgs) -> Result<Run> {
    let r = retired("task_solo").expect("task_solo is a retired kind");
    bail!("{}", r.line())
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

/// `horch done` from a ledger worker with no live attempt: the retry of a
/// `done` whose task fleetd finished already. It writes no row (RR-5,
/// T-885): a worker finishes its task only through `result.json` of its
/// live attempt. In a pane it reports and closes the pane.
pub fn done_cmd(
    ctx: &RuntimeContext,
    store: Store,
    summary: &str,
    unlanded: Option<&str>,
) -> Result<()> {
    let wid = worker_id(ctx)?;
    refuse_push_attempt(&store, &wid)?;
    let c = finish::already_completed(store.conn(), &wid)?.with_context(|| {
        format!(
            "worker {wid} has no live attempt, so horch done writes no result.json. fleetd finishes the task of an attempt; ask the orchestrator"
        )
    })?;
    drop(store);
    // The report to the orchestrator and the pane close: `horch done`'s
    // machinery, when this runs in a worker pane.
    let in_pane = in_worker_pane(ctx);
    // One push budget for the report, inside the agent's command timeout
    // (#67).
    let budget = delivery::start_push_budget(delivery::done_push_budget(&ctx.settings.faults));
    if budget.is_some() && in_pane {
        eprintln!("{}", super::messaging::CLOSING);
    }
    output::println(&done_line(&c));
    if !in_pane {
        return Ok(());
    }
    super::messaging::done_then(ctx, summary, unlanded, &|| {})
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

/// The line `done` prints for the finished task.
fn done_line(c: &finish::Completed) -> String {
    let what = if c.unlanded {
        "parked as UNLANDED"
    } else {
        "done"
    };
    format!("{} {what}.", c.task)
}

/// Whether this process is a task-ledger worker that holds (or finished) a
/// task: `horch done` then goes through [`done_cmd`]. A worker whose task
/// was cancelled or taken from it ends with the plain `horch done` (#136).
pub fn is_ledger_worker(ctx: &RuntimeContext) -> bool {
    let Some(wid) = worker_id_opt(ctx) else {
        return false;
    };
    taskledger::open_existing_read_only(ctx)
        .ok()
        .flatten()
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
/// not have, records nothing. It writes no row (RR-5, T-885): a process of
/// a live attempt appends the line to `progress.jsonl` of its outbox, and
/// fleetd ingests it. Another worker sends 1 `worker_progress` request.
pub fn progress_from_worker(ctx: &RuntimeContext, kind: EventKind, text: &str) -> Result<bool> {
    let Some(wid) = worker_id_opt(ctx) else {
        return Ok(false);
    };
    let Some(store) = taskledger::open_existing_read_only(ctx)? else {
        return Ok(false);
    };
    if workers::find(store.conn(), &wid)?.is_none() {
        return Ok(false);
    }
    if let Ok(out) = own_outbox(ctx, &store) {
        let line = match kind {
            EventKind::Note => LineKind::Note,
            _ => LineKind::Progress,
        };
        out.append(line, &taskledger::util::now(), text)?;
        return Ok(true);
    }
    drop(store);
    let payload = json!({
        "worker_id": wid,
        "kind": kind.as_str(),
        "source": ProgressSource::Agent.as_str(),
        "text": text,
    });
    request(ctx, Kind::WorkerProgress, payload)?;
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

#[cfg(test)]
mod tests {
    use super::{handoff_warning, LedgerCommand};
    use clap::Subcommand;
    use horch_core::roster::{ExecDecision, Roster};

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
