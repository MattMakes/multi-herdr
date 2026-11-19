//! The run of 1 reconcile pass.
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass" (RR-4, RR-7, FM-7,
//! FM-11, FM-12, PL-5). [`Pass::run`] runs 1 pass in 3 parts:
//!
//! 1. It reads the facts with no transaction, each scan within its budget
//!    from its cursor: the request files (each applied in its own
//!    transactions, as the channel applies a request), the processes that
//!    rows name, the outboxes, the wrapper files of each attempt of the
//!    outbox scan, the job result files, the worktree folders and the
//!    folders of the work directories, the herdr panes, the health holds
//!    and the launch record of each attempt in `launching`. It makes the
//!    missing outbox of each live attempt of its outbox scan (Q3 of
//!    `docs/tickets/fleetd-p5/MAP.md`). It reads the last passed
//!    `worktree_list` run of each repository from `jobs`. It runs no git.
//! 2. In 1 transaction it calls these steps, in this order:
//!    `check_generation`, `tick_fleet_clock`, the ingest of the outbox,
//!    wrapper and job result files, the needs-you item of the operator
//!    token, `observe_intents`,
//!    `mark_orphans`, `finish_exited`, `reconcile_worktrees`,
//!    `disk_budget` (it gives the hold of the worktree disk budget from the
//!    rows that the reconciler wrote), the run
//!    duties ([`RunDuties`]), `observe_jobs`, `check_invariants` (with I6),
//!    the `worktree_list` job of each repository in each period,
//!    `record_hold_changes` and `intended_effects`. The worktree
//!    observations and orphan marks come before the run duties and the job
//!    queues (stops come before launches). Each effect is a row in an
//!    intended state when the transaction commits. No effect occurs inside
//!    it. The run duties have a [`DutyBudget`]: at most `pass_budget` rows
//!    of each paged duty, and no new duty work after the time budget of the
//!    pass.
//! 3. After the commit it does the effects, stops before launches, until
//!    the time budget of the pass ends. The rest stay intended rows for the
//!    next pass. When time is left, the run duties do their effects. It
//!    archives the job result files that the transaction ingested. Then it
//!    writes the heartbeat and pings the watchdog.
//!
//! At its start, after the lock and the generation, fleetd calls
//! [`Pass::start`] (the operator token, then the request files that wait)
//! and then [`Pass::serve`] (the channel server).
//!
//! A crash before the commit leaves no effect. A crash after it leaves
//! intended rows, and the next pass does each effect once: a launch is
//! keyed by its attempt id, a stop by the pid and start time of its
//! process, and a job start by the id of its row.
//!
//! 1 git writer for each repository (MAP.md of fleetd-p3, Q12): part 2
//! takes a due `worktree_add`, `worktree_remove` or `rescue` job only when
//! no such job of its repository runs or is already an effect of the pass.
//!
//! The integration of Phase 4 (ticket 47 of fleetd-p4), with the Phase 4
//! work of the pass: after the verifier, part 2 queues the due fetch and
//! push jobs, follows the task branches, the bundles, the ref moves, the
//! candidate refs, the features and the rebases, opens the resolver tasks
//! of a rebase conflict, and checks I11, I12 and the rescue classes. The
//! branches, the bundles and the features each have their cursor in
//! `pass_cursors`. Part 3 then writes the rescue refs, moves the feature
//! refs and deletes the refs that are due, each by compare-and-set, after
//! the other effects and within the time budget.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use crate::fleetd::fleet_runtime::adapters::clock_store::{self, ClockStore};
use crate::fleetd::fleet_runtime::adapters::effects::{self, Activity, LaunchRecord, Watch};
use crate::fleetd::fleet_runtime::adapters::job_result_file::FileRead as JobFile;
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::request_spool::{self, ScanOutcome, Spool};
use crate::fleetd::fleet_runtime::adapters::scans::{
    self, CliPath, FolderFact, FolderItem, Listing, Liveness, OutboxFact, Presence, ProcessFact,
    ProcessItem, ScanClock,
};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::adapters::token_file::{self, Token};
use crate::fleetd::fleet_runtime::adapters::{cursors_store, fleetd_row, polite_stops_store};
use crate::fleetd::fleet_runtime::application::holds::{self, record_hold_changes};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, JobCtx, JobKey, JobRunner, NeedsYouItems, NewItem,
};
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::budget::{
    id_key, Cursor, Deadline, PassBudget, RowKind, ScanKind,
};
use crate::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::health::Hold;
use crate::fleetd::fleet_runtime::domain::heartbeat::{Exe, Heartbeat};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::fleet_runtime::domain::pass::{
    breaches, observe_launch, stop_step, Breach, Holding, Launch, LaunchStep, Proc, StopStep,
    TIMED_OUT,
};
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::application::intake::{self, pending_idea_files, write_idea_file};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write as NeedsYouWrite;
use crate::fleetd::integration::adapters::exec_rebase::GitRebaseRefs;
use crate::fleetd::integration::adapters::git::{CliGitGateway, CliStackGit};
use crate::fleetd::integration::adapters::sqlite as integration_rows;
use crate::fleetd::integration::application as integration;
use crate::fleetd::integration::application::new_features::Project;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::domain::stack::StackPlanner;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::adapters::attempt_files::{
    self, Found, OsProcesses, WrapperRead,
};
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::dispatch::{self, DispatchCtx};
use crate::fleetd::task_execution::application::launch::{self as push, Launches};
use crate::fleetd::task_execution::application::outbox_ingest::{self, OutboxRead};
use crate::fleetd::task_execution::application::quota::{
    self, Pools, Restart, Restarted, Restarts,
};
use crate::fleetd::task_execution::application::stall as ladder;
use crate::fleetd::task_execution::application::verifier::{self, Passed, Verifier};
use crate::fleetd::task_execution::application::{cancel, commands, lease, retry};
use crate::fleetd::task_execution::application::{repair, review};
use crate::fleetd::task_execution::domain::dispatch::Holds as DispatchHolds;
use crate::fleetd::task_execution::domain::lease::Limits;
use crate::fleetd::task_execution::domain::outbox::DIR_MODE as OUTBOX_MODE;
use crate::fleetd::task_execution::domain::quota::limited;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::stall::{self as rungs, Decision, PaneStatus};
use crate::fleetd::task_execution::domain::task::{
    self, promotable, AttemptRow, AttemptState, Facts, InPane, ProcessScope, Reap, Recorded,
    RowRef, Rows, Seen, Stop, Target,
};
use crate::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use crate::fleetd::worker_profiles::application::conformance::read_conformance_json;
use crate::fleetd::worker_profiles::application::docs_jobs::{
    binary_id, queue_docs_jobs, scan_cli, CliBinary,
};
use crate::fleetd::worker_profiles::application::drift_idea::{add_drift_idea, KIND as DRIFT};
use crate::fleetd::worker_profiles::application::health::mark_after_conformance;
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::fleetd::worker_profiles::domain::conformance::ConformanceReport;
use crate::fleetd::worker_profiles::domain::profile::{Mode, Profile};
use crate::fleetd::worktree_custody::adapters::git::{worktree_list_key, ListResult};
use crate::fleetd::worktree_custody::adapters::sqlite as trees;
use crate::fleetd::worktree_custody::application::disk::disk_budget;
use crate::fleetd::worktree_custody::application::reconcile::{
    check_i6, reconcile_worktrees, Facts as WorktreeFacts, Listed, WorkDir,
};
use crate::fleetd::worktree_custody::application::rescue_refs::{self, RepoFacts};
use crate::fleetd::worktree_custody::domain::worktree::Breach as I6;
use crate::harness::HarnessKind;
use crate::roster::Roster;
use crate::routing::quota::QuotaView;
use crate::runtime::fault::Faults;
use crate::taskledger::finish::Tools;
use crate::taskledger::git::Git;
use crate::taskledger::model::{Actor, EventKind, ProgressSource, TaskStatus};
use crate::taskledger::sql_words::WORKTREES_GONE;
use crate::taskledger::store::Store;
use crate::taskledger::supervisor::OutcomeHint;
use crate::taskledger::triage::default_branch;
use crate::taskledger::util;
use crate::taskledger::watchdog::{self, Observation};
use crate::taskledger::workers::{self, Worker};
use crate::taskledger::{runs, tasks, timer};

/// After the first full pass, the next pass sleeps for ever before its
/// heartbeat (`p0_fleetd_hang.rs`).
pub const HANG_PASS: &str = "hang-pass";
/// The pass ends the process after the commit of part 2, before part 3.
pub const ABORT_AFTER_COMMIT: &str = "abort-after-commit";
/// The fault point that ends fleetd in part 2, before its commit.
pub const ABORT_BEFORE_COMMIT: &str = "abort-before-commit";
/// The file watching reports nothing.
pub const DROP_WATCH_EVENTS: &str = "drop-watch-events";

/// The cursor of the launches of part 3 in `pass_cursors`.
const LAUNCHES: &str = "launches";
/// The cursor of the job starts of part 3 in `pass_cursors`.
const JOB_STARTS: &str = "job_starts";
/// The cursor of the live attempts that part 1 observes, in `pass_cursors`.
const LIVE_ATTEMPTS: &str = "live_attempts";
/// The `pass_cursors` kind of the dispatch: the last task that a pass
/// bound. The rules of `domain/dispatch.rs` give the order of the binds.
const DISPATCH: &str = "dispatch";
/// The `attempts.via` of an attempt that fleetd bound (Q4 of
/// `docs/tickets/fleetd-p4/MAP.md`).
const PUSH: &str = "push";
/// The kind of the needs-you item of a breach that no step repairs.
const BREACH: &str = "invariant_breach";
/// The kind of the needs-you item of an operator token file that fails its
/// checks. While it is open, fleetd applies no operator request.
pub const TOKEN_REFUSED: &str = "operator_token_refused";
/// The kind of the needs-you item of an idle author of a `verifying` task
/// (rule D). Phase 4 removes it, when the attempt ends at `result.json`.
const FINISH_IDLE: &str = "finish_idle";

/// What a pass reads and does outside the ledger and the process table.
/// The composition root of `horch fleetd` gives it; a test gives fakes.
pub trait Outside {
    /// The panes of the herdr snapshot, each with what herdr reports of
    /// the process in it. `None` when herdr gives no snapshot.
    fn panes(&self) -> Option<BTreeMap<String, InPane>>;

    /// Close the exact pane `pane`.
    fn close_pane(&self, pane: &str) -> Result<()>;

    /// The health holds now (OR-4, OR-5). The pass gives the hold of the
    /// worktree disk budget itself, in part 2, in place of any that this
    /// gives.
    fn holds(&self) -> Vec<Hold>;

    /// The folder of each worktree row that is not `removed`, with its
    /// worktree id as its key, for the scan of worktree folders. A path is
    /// written as git writes it. The rows are not rows of fleet runtime.
    fn worktree_folders(&self, conn: &Connection) -> Result<Vec<FolderItem>>;

    /// The listing of the work directory `<project>/.worktrees/` of each
    /// repository of [`Outside::repositories`]: the full path of each entry
    /// (MAP.md of fleetd-p3, Q4). The scan of worktree folders reads each
    /// entry that no row names. An `Outside` with no runs has none.
    fn work_dirs(&self, _conn: &Connection) -> Result<Vec<WorkDir>> {
        Ok(Vec::new())
    }

    /// The repositories of the fleet: the project root of each run that is
    /// not `done` or `abandoned`, each once. The runs are not rows of fleet
    /// runtime. An `Outside` with no runs, as a fake of a test, has none.
    fn repositories(&self, _conn: &Connection) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// The launch spec of the attempt `attempt` in `launching`.
    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec>;

    /// What herdr and the transcript show of the worker `w` now: the
    /// activity and the pane facts of its attempt. `None` when the pass
    /// reads nothing: the attempt earns no activity in this pass.
    fn observe(&self, _w: &Worker) -> Option<Observation> {
        None
    }

    /// The roster and the pool readings of this pass (RR-11). `None` when
    /// they cannot be read: each persona has the profile of the design
    /// defaults, and no pool counts as limited.
    fn pools(&self) -> Option<PoolFacts> {
        None
    }

    /// Type the constant doorbell of the stall ladder into the pane `pane`
    /// with today's pane send of herdr.
    fn doorbell(&self, pane: &str) -> Result<()> {
        anyhow::bail!("this fleetd sends no doorbell to pane {pane}")
    }

    /// The launch of the fallback worker of the restart `r` (RR-11).
    fn restart_spec(&self, r: &Restart) -> Result<LaunchSpec> {
        anyhow::bail!("this fleetd starts no fallback worker for {}", r.task_id)
    }

    /// Each CLI of the docs sources and the path of its binary, for the
    /// scan of the CLI binaries of Phase 7. An `Outside` with no CLIs, as a
    /// fake of a test, has none.
    fn cli_binaries(&self) -> Vec<CliPath> {
        Vec::new()
    }
}

/// The roster and the pool readings of 1 pass.
pub struct PoolFacts {
    pub roster: Roster,
    pub quota: QuotaView,
}

/// The restarts of part 3, through the [`Outside`] of the pass.
struct OutsideRestarts<'a>(&'a dyn Outside);

impl Restarts for OutsideRestarts<'_> {
    fn spec(&self, r: &Restart) -> Result<LaunchSpec> {
        self.0.restart_spec(r)
    }
}

/// The work budget of the run duties in 1 pass (PL-5, A32).
#[derive(Clone, Copy)]
pub struct DutyBudget<'a> {
    /// The rows of each paged duty (`pass_budget`).
    pub rows: PassBudget,
    /// Whether the time budget of the pass is used: the duties start no
    /// new work then.
    pub time_up: &'a dyn Fn() -> bool,
}

/// What the run duties see in part 2.
pub struct DutyCtx<'a> {
    /// The generation of this fleetd.
    pub guard: Generation,
    /// The fleet clock of this pass.
    pub clock: FleetSeconds,
    /// The process and pane facts of part 1.
    pub facts: &'a Facts,
    /// The row and time budget of the run duties in this pass.
    pub budget: DutyBudget<'a>,
}

/// What the run duties see in part 3.
pub struct ActCtx<'a> {
    pub fleet_root: &'a std::path::Path,
    /// The fleet clock of this pass.
    pub clock: FleetSeconds,
}

/// The duties of the watchdog of today (FM-7), in the 3 parts of the pass.
/// Each effect of [`RunDuties::act`] is a row that [`RunDuties::run`]
/// committed, so the next pass repeats an effect that a crash after the
/// commit lost.
pub trait RunDuties {
    /// Part 1: read the facts of the duties, with no transaction, within
    /// `budget`.
    fn observe(&self, _conn: &Connection, _budget: DutyBudget<'_>) -> Result<()> {
        Ok(())
    }

    /// Part 2: write the rows of the duties in `tx`. Returns the stops to
    /// do in part 3 with the other stops of the pass.
    fn run(&self, tx: &Transaction<'_>, ctx: &DutyCtx<'_>) -> Result<Vec<Stop>>;

    /// Whether the duties launch the attempt `a` in `launching` in part 3,
    /// in place of the launch of the pass.
    fn launches(&self, _a: &AttemptRow) -> bool {
        false
    }

    /// Part 3: do the effects of the committed rows.
    fn act(&self, _conn: &Connection, _ctx: &ActCtx<'_>) -> DutyReport {
        DutyReport::default()
    }
}

/// What part 3 of the run duties did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DutyReport {
    /// The effects done, in order.
    pub done: Vec<String>,
    /// The effects that failed; the next pass tries them again.
    pub failed: Vec<String>,
}

/// No run duties.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDuties;

impl RunDuties for NoDuties {
    fn run(&self, _tx: &Transaction<'_>, _ctx: &DutyCtx<'_>) -> Result<Vec<Stop>> {
        Ok(Vec::new())
    }
}

/// The ports of the Phase 4 work of task execution
/// (`docs/tickets/fleetd-p4/46-pass-wiring-dispatch-and-verifier.md`).
#[derive(Clone, Copy)]
pub struct TaskWork<'a> {
    /// The tools of the finish steps: the git of the binds and of the
    /// tips, and the review steps of the verifier.
    pub tools: &'a Tools,
    /// The check sandbox of each persona (SR-4).
    pub sandboxes: &'a WorkerSandboxes,
    /// The spawn of each push attempt that fleetd bound.
    pub launches: &'a dyn Launches,
}

/// The settings of 1 fleetd.
#[derive(Debug, Clone)]
pub struct Settings {
    pub fleet_root: PathBuf,
    /// The generation that this fleetd took at its start.
    pub generation: Generation,
    pub config: FleetConfig,
    /// The `horch` binary of each job process: `horch job run <id>`.
    pub horch: PathBuf,
    pub faults: Faults,
}

/// The ports of a pass.
pub struct Ports<'a> {
    pub outside: &'a dyn Outside,
    pub duties: &'a dyn RunDuties,
    pub needs_you: &'a dyn NeedsYouItems,
    /// The launcher of each process.
    pub host: &'a dyn ProcessHost,
}

/// 1 effect that part 3 did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// A stop: `process <pid>`, `kill process <pid>` or `pane <id>`.
    Stop(String),
    /// The stop of the process group of the job.
    StopJob(i64),
    /// The doorbell of the stall ladder, typed into the pane.
    Doorbell(String),
    /// The launch of the attempt.
    Launch(String),
    /// The start of the process of the job.
    StartJob(i64),
    /// An effect of the run duties.
    Duty(String),
    /// A git effect of integration on a local ref.
    Git(String),
}

/// What 1 pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PassReport {
    /// The effects of part 3, in order.
    pub done: Vec<Done>,
    /// The effects that the time budget left for the next pass.
    pub left: usize,
    /// The effects that failed; the next pass tries them again.
    pub failed: Vec<String>,
    /// Whether the pass wrote its heartbeat.
    pub heartbeat: bool,
}

/// Why a pass did not run.
#[derive(Debug)]
pub enum PassError {
    /// Another fleetd holds the ledger. This fleetd exits with no effect.
    GenerationLost(DoorError),
    /// The ledger or a fact failed. Part 2 rolled back.
    Failed(anyhow::Error),
}

impl std::fmt::Display for PassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PassError::GenerationLost(e) => write!(f, "{e}"),
            PassError::Failed(e) => write!(f, "the pass failed: {e:#}"),
        }
    }
}

impl std::error::Error for PassError {}

impl From<anyhow::Error> for PassError {
    fn from(e: anyhow::Error) -> Self {
        PassError::Failed(e)
    }
}

/// 1 intended effect of part 2.
enum Effect {
    Stop(Stop),
    StopJob(jobs::Stop),
    /// The doorbell of the stall ladder into the pane.
    Doorbell(Bell),
    /// The launch of an attempt, or the restart of a task on its fallback.
    Launch(AttemptRow),
    /// The spawn of a push attempt that fleetd bound, by its id.
    PushLaunch(String),
    StartJob(JobRow),
}

/// The order of part 3 (MAP.md, "Part 3"): each stop, then each doorbell,
/// then each launch and restart, then each job start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Stop,
    Doorbell,
    Launch,
    StartJob,
}

impl Effect {
    fn order(&self) -> Rank {
        match self {
            Effect::Stop(_) | Effect::StopJob(_) => Rank::Stop,
            Effect::Doorbell(_) => Rank::Doorbell,
            Effect::Launch(_) | Effect::PushLaunch(_) => Rank::Launch,
            Effect::StartJob(_) => Rank::StartJob,
        }
    }
}

/// 1 doorbell to type: the row of its event, and the pane of the attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bell {
    attempt: String,
    event: i64,
    pane: String,
}

/// The facts of part 1.
struct Read {
    /// The ledger stamp of the pass.
    stamp: String,
    cursors: Vec<(ScanKind, Cursor)>,
    /// The facts of the reap path.
    task: Facts,
    /// The process of each `running` job, keyed by its id.
    jobs: Vec<ProcessFact>,
    /// Each attempt in `launching`, with what part 1 saw of it.
    launching: Vec<Launching>,
    /// The processes whose record of a polite stop stays: a row names
    /// each, and the scan did not see it gone, or the pass has no herdr
    /// snapshot to judge the pane of a gone one.
    may_stop: BTreeSet<(u32, u64)>,
    holds: Vec<Hold>,
    awake: AwakeReading,
    /// The worktree facts of the reconciler.
    worktrees: WorktreeFacts,
    /// The git repositories of the fleet: each gets its `worktree_list`
    /// job.
    repos: Vec<String>,
    /// The live attempts of this pass, within `pass_budget`.
    live: Vec<LiveSeen>,
    /// The cursor of [`LIVE_ATTEMPTS`] after the live attempts of the pass.
    live_cursor: Cursor,
    /// The activity of each live attempt, to write after the commit.
    activity: BTreeMap<String, Activity>,
    /// The doorbell events that part 3 typed, of each live attempt.
    sent: BTreeMap<String, BTreeSet<i64>>,
    pools: Option<PoolFacts>,
    /// The outbox files of the attempts of the outbox scan.
    outboxes: Vec<OutboxRead>,
    /// The wrapper files of the attempts of the outbox scan.
    wrappers: Vec<WrapperRead>,
    /// The job result files.
    job_files: Vec<JobFile>,
    /// Why the operator token file failed its checks; `None` when it
    /// passed them.
    token_fault: Option<String>,
    /// The tip of the task branch of each task that the verifier or a
    /// repair result reads, by task id. A task with no tip waits.
    tips: BTreeMap<String, String>,
    /// The lines of what part 1 could not do.
    failed: Vec<String>,
    /// The facts of Phase 7.
    p7: P7Read,
}

/// 1 live attempt of a task, as part 1 saw it.
struct LiveSeen {
    attempt: AttemptRow,
    worker: Worker,
    process: Seen,
    /// The age of the wrapper heartbeat (`workers.last_checkin_at`).
    heartbeat_age_s: Option<i64>,
    /// A signal of activity moved since the pass before.
    active: bool,
    pane: Option<rungs::Pane>,
}

/// 1 attempt in `launching`, as part 1 saw it.
struct Launching {
    row: AttemptRow,
    /// The process that its row names.
    row_process: Proc,
    /// Its launch, with what part 1 saw of the launched process.
    launch: Option<Launch>,
    /// The process of its launch record.
    launched: Option<Recorded>,
}

/// The items of the process scan, and which of them are job processes.
struct ProcessItems {
    items: Vec<ProcessItem>,
    /// The keys of the job processes: the ids of their rows.
    jobs: BTreeSet<String>,
}

/// The attempts to launch in part 3: the attempts in `launching` of the
/// run duties and of the restarts, and the push attempts by id.
struct ToLaunch {
    attempts: Vec<AttemptRow>,
    push: Vec<String>,
}

/// What the Phase 4 work of part 2 decided.
#[derive(Default)]
struct WorkRows {
    /// The stops of the processes of older attempts, before a launch.
    stops: Vec<Stop>,
    /// The push attempts to launch, in the order of their rows.
    launches: Vec<String>,
    /// The tasks whose check did not fail, for the review steps and the
    /// structural check of part 3.
    passed: Vec<Passed>,
    /// The lines of what failed: a bind or a repair start.
    failed: Vec<String>,
}

/// The decisions of part 2.
struct Decided {
    clock: FleetSeconds,
    effects: Vec<Effect>,
    breaches: BTreeSet<String>,
    may_stop: BTreeSet<(u32, u64)>,
    activity: BTreeMap<String, Activity>,
    sent: BTreeMap<String, BTreeSet<i64>>,
    /// The tasks whose check did not fail.
    passed: Vec<Passed>,
    /// The roster of the pass, for the numbers of rung 6 in part 3.
    roster: Option<Roster>,
    /// The lines of what failed in part 2 and kept its row unchanged.
    failed: Vec<String>,
    /// The git effects of integration.
    integration: Integrated,
}

/// The pane stops of the effects of 1 pass, as part 3 does them.
struct PaneStops {
    /// The rows with a process that is still in its grace: their panes
    /// close on a later pass, when the process is gone.
    in_grace: BTreeSet<RowRef>,
    /// The pane of each pane stop, by the process that proves it. The polite
    /// stop of that process records the pane.
    proofs: BTreeMap<(u32, u64), String>,
    /// The processes whose pane this pass closed.
    closed: BTreeSet<(u32, u64)>,
}

impl PaneStops {
    fn of(effects: &[Effect]) -> PaneStops {
        let proofs = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Stop(Stop {
                    target: Target::Pane { pane, shell },
                    ..
                }) => Some(((shell.pid, shell.pid_start?), pane.clone())),
                _ => None,
            })
            .collect();
        PaneStops {
            in_grace: BTreeSet::new(),
            proofs,
            closed: BTreeSet::new(),
        }
    }
    /// `row` has a process in its grace: its panes wait.
    fn hold(&mut self, row: &RowRef) {
        self.in_grace.insert(row.clone());
    }

    /// Whether a pane of `row` may close now: no process of the row is in
    /// its grace.
    fn may_close(&self, row: &RowRef) -> bool {
        !self.in_grace.contains(row)
    }

    /// The pane that the process proves, if a pane stop names it.
    fn pane_of(&self, pid: u32, pid_start: u64) -> Option<String> {
        self.proofs.get(&(pid, pid_start)).cloned()
    }

    /// The pane that `shell` proves is closed.
    fn closed(&mut self, shell: Recorded) {
        self.closed
            .extend(shell.pid_start.map(|start| (shell.pid, start)));
    }

    /// The processes whose pane this pass did not close: their records of
    /// the polite stop stay.
    fn pending(self) -> BTreeSet<(u32, u64)> {
        let closed = self.closed;
        self.proofs
            .into_keys()
            .filter(|k| !closed.contains(k))
            .collect()
    }
}

/// 1 fleetd, from pass to pass: what a pass keeps in memory.
pub struct Pass<'a> {
    settings: Settings,
    ports: Ports<'a>,
    now: Box<dyn FnMut() -> Instant + 'a>,
    clock: ClockStore,
    jobs: JobRunner<'a>,
    /// The launched attempt processes, to reap.
    launched: Vec<Child>,
    /// The global holds of the last pass.
    holds: Vec<Hold>,
    /// The needs-you keys of the breaches of the last pass.
    breaches: BTreeSet<String>,
    watch: Watch,
    /// The binary of this process at its start.
    exe: Exe,
    full_passes: u64,
    /// The handlers of the request files.
    requests: Handlers,
    /// The ports of the Phase 4 work of task execution; `None`: the pass
    /// does none of it.
    work: Option<TaskWork<'a>>,
}

impl<'a> Pass<'a> {
    pub fn new(settings: Settings, ports: Ports<'a>) -> Pass<'a> {
        let host = ports.host;
        Pass {
            settings,
            ports,
            now: Box::new(Instant::now),
            clock: ClockStore::new(),
            jobs: JobRunner::new(host),
            launched: Vec::new(),
            holds: Vec::new(),
            breaches: BTreeSet::new(),
            watch: Watch::new(),
            exe: effects::this_exe(),
            full_passes: 0,
            requests: Handlers::new(),
            work: None,
        }
    }

    /// Do the Phase 4 work of task execution with `work` in each pass:
    /// dispatch, the launch of push attempts, the verifier, the review
    /// steps and the repair attempts.
    pub fn with_task_work(mut self, work: TaskWork<'a>) -> Pass<'a> {
        self.work = Some(work);
        self
    }

    /// Apply each request file with `handlers`. With none, each operator
    /// request is refused as not handled.
    pub fn with_requests(mut self, handlers: Handlers) -> Pass<'a> {
        self.requests = handlers;
        self
    }

    /// Read the time budgets on `now` instead of the monotonic clock.
    pub fn with_clock(mut self, now: Box<dyn FnMut() -> Instant + 'a>) -> Pass<'a> {
        self.now = now;
        self
    }

    /// Run 1 pass on the ledger `conn`.
    pub fn run(&mut self, conn: &mut Connection) -> Result<PassReport, PassError> {
        let deadline = Deadline::pass((self.now)(), self.settings.config.pass_time_budget_s);
        self.reap();
        let old_stops = self.import_old_stops(conn);
        // A missing token file is made again; a file that exists is never
        // replaced.
        let token = token_file::ensure(&self.settings.fleet_root).map_err(|e| format!("{e:#}"));
        let (requests, request_failed) = self.request_files(conn, deadline, token.as_ref().ok())?;
        let mut read = self.read_facts(conn, deadline, requests, token.err())?;
        let mut failed = std::mem::take(&mut read.failed);
        failed.extend(request_failed);
        let job_files = read.job_files.clone();
        {
            let now = RefCell::new(&mut self.now);
            let time_up = || deadline.reached((now.borrow_mut())());
            let budget = DutyBudget {
                rows: PassBudget::new(self.settings.config.pass_budget),
                time_up: &time_up,
            };
            self.ports.duties.observe(conn, budget)?;
        }
        let decided = match self.decide(conn, read, deadline) {
            Ok(d) => d,
            Err(PassError::Failed(e)) => {
                util::record_refusal_after_rollback(conn, &e);
                return Err(PassError::Failed(e));
            }
            Err(e) => return Err(e),
        };
        self.settings.faults.abort_if(ABORT_AFTER_COMMIT);
        let mut decided = decided;
        failed.append(&mut decided.failed);
        if self.work.is_some() {
            // `task.yaml` and `START.md` of each bound attempt come before
            // its launch, also after a crash between the commit and them.
            failed.extend(write_task_files(conn, &self.settings.fleet_root));
        }
        let activity = effects::write_activity(&self.settings.fleet_root, &decided.activity);
        let passed = std::mem::take(&mut decided.passed);
        let effects = std::mem::take(&mut decided.effects);
        let sent = std::mem::take(&mut decided.sent);
        let (mut report, pending) = self.do_effects(conn, effects, sent, decided.clock, deadline);
        let git = std::mem::take(&mut decided.integration);
        self.integration_effects(conn, git, decided.clock, deadline, &mut report);
        if let Some(w) = self.work {
            let lines = self.verify_after_commit(conn, w, &decided, passed, deadline);
            report.failed.extend(lines);
        }
        self.p7_effects(conn, deadline, &mut report);
        report.failed.extend(old_stops);
        report.failed.extend(failed);
        if let Err(e) = jobs::archive_results(&self.settings.fleet_root, &job_files) {
            report
                .failed
                .push(format!("the archive of the job result files: {e:#}"));
        }
        if let Err(e) = activity {
            report.failed.push(format!("the activity record: {e:#}"));
        }
        if report.left == 0 && !deadline.reached((self.now)()) {
            let ctx = ActCtx {
                fleet_root: &self.settings.fleet_root,
                clock: decided.clock,
            };
            let duty = self.ports.duties.act(conn, &ctx);
            report.done.extend(duty.done.into_iter().map(Done::Duty));
            report.failed.extend(duty.failed);
        }
        let kept = |pid, start| {
            decided.may_stop.contains(&(pid, start)) || pending.contains(&(pid, start))
        };
        if let Err(e) = polite_stops_store::forget(conn, self.settings.generation, kept) {
            report
                .failed
                .push(format!("the rows of polite stops: {e:#}"));
        }
        self.breaches = decided.breaches;
        if self.settings.faults.has(HANG_PASS) && self.full_passes > 0 {
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        match effects::write_heartbeat(&self.settings.fleet_root, &self.heartbeat()) {
            Ok(()) => {
                report.heartbeat = true;
                effects::watchdog_ping();
            }
            Err(e) => report.failed.push(format!("the heartbeat: {e:#}")),
        }
        self.full_passes += 1;
        Ok(report)
    }

    /// The start of this fleetd after the lock and the generation (Phase
    /// 5): make the operator token, then apply the request files that wait,
    /// within the time budget of 1 pass. A token file that fails its checks
    /// is not replaced: it opens 1 needs-you item, and no operator request
    /// is applied until it is corrected. Gives the lines of what failed.
    pub fn start(&mut self, conn: &mut Connection) -> Result<Vec<String>, PassError> {
        let deadline = Deadline::pass((self.now)(), self.settings.config.pass_time_budget_s);
        let token = token_file::ensure(&self.settings.fleet_root).map_err(|e| format!("{e:#}"));
        let fault = token.as_ref().err().map(String::as_str);
        let guard = self.settings.generation;
        let root = self.settings.fleet_root.clone();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)?;
        match fleetd_row::check_generation(&tx, guard) {
            Ok(()) => {}
            Err(e @ DoorError::GenerationLost { .. }) => return Err(PassError::GenerationLost(e)),
            Err(e) => return Err(PassError::Failed(e.into())),
        }
        token_item(&tx, &root, fault)?;
        tx.commit().map_err(anyhow::Error::from)?;
        let (cursor, mut failed) = self.request_files(conn, deadline, token.as_ref().ok())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)?;
        cursors_store::save(&tx, ScanKind::Requests.as_str(), &cursor)?;
        tx.commit().map_err(anyhow::Error::from)?;
        if let Err(e) = token {
            failed.insert(0, format!("the operator token: {e}"));
        }
        Ok(failed)
    }

    /// Start the channel server of this fleetd with `ledger` as its own
    /// connection to the ledger and `handlers` as the handlers of its
    /// requests. Call it after [`Pass::start`]. The server ends when the
    /// returned value is dropped.
    pub fn serve(&self, ledger: Connection, handlers: Handlers) -> Result<ChannelServer> {
        ChannelServer::start(
            &self.settings.fleet_root,
            ledger,
            self.settings.generation,
            handlers,
        )
    }

    /// Part 1, first: apply the request files from the cursor of the
    /// requests scan until its budget ends. Each file gets its `requests`
    /// row in its own transactions, as a request of the channel does
    /// (ticket 14). `token` is the operator token, `None` when its file
    /// failed its checks. Gives the cursor after the files, and the line of
    /// a failure. A lost generation ends the pass.
    fn request_files(
        &mut self,
        conn: &mut Connection,
        deadline: Deadline,
        token: Option<&Token>,
    ) -> Result<(Cursor, Vec<String>), PassError> {
        let stamp = util::now();
        let owner = User::current().context("reading the user of fleetd")?;
        let mut cursor = cursors_store::load(conn, ScanKind::Requests.as_str(), &stamp)?;
        let start = (self.now)();
        let mut clock = ScanClock {
            deadline: deadline.scan(start, self.settings.config.scan_budget_ms),
            now: &mut *self.now,
            stamp: &stamp,
        };
        let spool = Spool {
            fleet_root: &self.settings.fleet_root,
            owner: &owner,
            token,
            max_bytes: self.settings.config.request_max_bytes,
            guard: self.settings.generation,
        };
        let failed =
            match request_spool::scan(conn, &spool, &mut cursor, &mut clock, &self.requests) {
                Ok(ScanOutcome::Read(_)) => Vec::new(),
                Ok(ScanOutcome::FolderRefused { reason }) => {
                    vec![format!("the request folder: {reason}")]
                }
                Err(e @ DoorError::GenerationLost { .. }) => {
                    return Err(PassError::GenerationLost(e))
                }
                Err(e) => vec![format!("the request files: {e}")],
            };
        Ok((cursor, failed))
    }

    /// At the first pass of this fleetd, make a row of each polite stop
    /// file of an older fleetd in `run/stops/` and remove the files. Gives
    /// the log lines of the files that failed a check.
    fn import_old_stops(&self, conn: &mut Connection) -> Vec<String> {
        if self.full_passes > 0 {
            return Vec::new();
        }
        let root = &self.settings.fleet_root;
        polite_stops_store::import_old_files(conn, self.settings.generation, root)
            .unwrap_or_else(|e| vec![format!("the old records of polite stops: {e:#}")])
    }

    /// Reap each ended process that this fleetd started, so that its pid
    /// is gone, not a zombie, for the process scan.
    fn reap(&mut self) {
        self.jobs.reap();
        self.launched
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
    }

    // ─── part 1 ─────────────────────────────────────────────────────────

    /// `requests` is the cursor of the request files after
    /// [`Pass::request_files`], and `token_fault` why the token file failed
    /// its checks.
    fn read_facts(
        &mut self,
        conn: &Connection,
        deadline: Deadline,
        requests: Cursor,
        token_fault: Option<String>,
    ) -> Result<Read> {
        let root = self.settings.fleet_root.clone();
        let stamp = util::now();
        let rows = attempts_store::rows(conn, &util::host_name())?;
        let records = launch_records(&root, &rows)?;
        let processes = process_items(&rows, &records, jobs::processes(conn)?);
        let outboxes = outbox_items(&root, &rows);
        let rows_folders = self.ports.outside.worktree_folders(conn)?;
        let work_dirs = self.ports.outside.work_dirs(conn)?;
        let trees = tree_items(&rows_folders, &work_dirs);
        let changed = self.changed_folders(&outboxes, &trees);
        let mut cursors = Vec::new();
        let mut liveness = BTreeMap::new();
        let mut folders = Vec::new();
        let mut scanned = Vec::new();
        for kind in ScanKind::ALL {
            let mut cursor = match kind {
                ScanKind::Requests => requests.clone(),
                _ => cursors_store::load(conn, kind.as_str(), &stamp)?,
            };
            let start = (self.now)();
            let mut clock = ScanClock {
                deadline: deadline.scan(start, self.settings.config.scan_budget_ms),
                now: &mut *self.now,
                stamp: &stamp,
            };
            match kind {
                ScanKind::Processes => {
                    let scan = scans::scan_processes(&processes.items, &mut cursor, &mut clock);
                    liveness.extend(scan.facts.into_iter().map(|f| (f.key, f.liveness)));
                }
                ScanKind::Outboxes => {
                    let scan = scans::scan_outboxes(&outboxes, &changed, &mut cursor, &mut clock);
                    scanned = scan.facts;
                }
                ScanKind::WorktreeFolders => {
                    let scan =
                        scans::scan_worktree_folders(&trees, &changed, &mut cursor, &mut clock);
                    folders = scan.facts;
                }
                // The request files were applied first, in
                // `request_files`.
                ScanKind::Requests => {}
            }
            cursors.push((kind, cursor));
        }
        // After the process scan: a process that is gone wrote its files
        // before it ended.
        let config = &self.settings.config;
        let owner = User::current().context("reading the user of fleetd")?;
        let (attempts, failed) = scanned_attempts(&root, &rows, &scanned);
        let limits = outbox_ingest::Limits::of(config);
        let outbox_reads = outbox_ingest::read(conn, &root, &attempts, &owner, &limits)?;
        let wrappers =
            attempt_files::read(conn, &root, &attempts, &owner, config.heartbeat_max_bytes)?;
        let job_files = jobs::read_results(conn, &root, &owner, config.job_result_max_bytes)?;
        let repos = git_repositories(self.ports.outside.repositories(conn)?);
        let worktrees = worktree_facts(
            &rows_folders,
            &work_dirs,
            folders,
            last_lists(conn, &repos)?,
        );
        let mut may_stop = may_stop(&rows, &liveness);
        let panes = self.ports.outside.panes();
        let stopped =
            polite_stops_store::stopped(conn, |pid, start| !may_stop.contains(&(pid, start)))?;
        // With no snapshot, no pane can be judged: a record whose process a
        // row with a pane names waits for a pass with one.
        if panes.is_none() {
            may_stop.extend(
                task::needs_snapshot(&rows, &stopped)
                    .into_iter()
                    .filter_map(|p| Some((p.pid, p.pid_start?))),
            );
        }
        let seen = task_processes(&rows, &liveness);
        let (live, live_cursor, activity) = self.read_live(conn, &rows, &seen, &stamp, deadline)?;
        // A result of this pass moves its task to `verifying` in part 2, and
        // the verifier of the same pass queues its check.
        let results: BTreeSet<&str> = outbox_reads
            .iter()
            .filter(|r| !r.messages.is_empty())
            .map(|r| r.attempt.task_id.as_str())
            .collect();
        let tips = match &self.work {
            Some(w) => task_tips(conn, &w.tools.git, &results)?,
            None => BTreeMap::new(),
        };
        let p7 = self.read_p7(conn, deadline, &stamp, &wrappers)?;
        let mut sent = effects::read_doorbells(&self.settings.fleet_root);
        sent.retain(|id, _| {
            rows.attempts
                .iter()
                .any(|a| &a.id == id && a.state == AttemptState::Live)
        });
        Ok(Read {
            stamp,
            cursors,
            task: Facts {
                processes: seen,
                panes,
                stopped,
            },
            jobs: job_facts(&processes.jobs, &liveness),
            launching: launching(&rows, &records, &liveness),
            may_stop,
            holds: self.health_holds(),
            awake: clock_store::awake_now()?,
            worktrees,
            repos,
            live,
            live_cursor,
            activity,
            sent,
            pools: self.ports.outside.pools(),
            outboxes: outbox_reads,
            wrappers,
            job_files,
            token_fault,
            tips,
            failed,
            p7,
        })
    }

    /// The facts of the live attempts of tasks, at most `pass_budget` of
    /// them from the cursor [`LIVE_ATTEMPTS`], until the scan budget ends:
    /// the process, the heartbeat age, the activity since the pass before
    /// and the pane. Gives the facts, the cursor after them, and the
    /// activity of each live attempt to write after the commit.
    fn read_live(
        &mut self,
        conn: &Connection,
        rows: &Rows,
        seen: &BTreeMap<Recorded, Seen>,
        stamp: &str,
        deadline: Deadline,
    ) -> Result<(Vec<LiveSeen>, Cursor, BTreeMap<String, Activity>)> {
        let root = &self.settings.fleet_root;
        let live: Vec<&AttemptRow> = rows
            .attempts
            .iter()
            .filter(|a| a.state == AttemptState::Live && a.task_id.is_some())
            .collect();
        let before = effects::read_activity(root);
        let mut after: BTreeMap<String, Activity> = before
            .iter()
            .filter(|(id, _)| live.iter().any(|a| &a.id == *id))
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect();
        let mut cursor = cursors_store::load(conn, LIVE_ATTEMPTS, stamp)?;
        let scan = deadline.scan((self.now)(), self.settings.config.scan_budget_ms);
        let budget = PassBudget::new(self.settings.config.pass_budget).rows();
        let now = util::now();
        let mut out = Vec::new();
        for a in cursor
            .turns(&live, |a| a.id.as_str())
            .into_iter()
            .take(budget)
        {
            if scan.reached((self.now)()) {
                break;
            }
            cursor.step(&a.id, stamp);
            let Some(w) = workers::find(conn, &a.worker_id)? else {
                continue;
            };
            let (l, activity) = self.live_seen(a, w, rows, seen, before.get(&a.id), &now);
            after.insert(a.id.clone(), activity);
            out.push(l);
        }
        Ok((out, cursor, after))
    }

    /// The facts of the live attempt `a` of the worker `w`, at the wall
    /// time `now`, and its activity signals to keep for the next pass.
    /// `before` is its activity of the pass before.
    fn live_seen(
        &self,
        a: &AttemptRow,
        w: Worker,
        rows: &Rows,
        seen: &BTreeMap<Recorded, Seen>,
        before: Option<&Activity>,
        now: &str,
    ) -> (LiveSeen, Activity) {
        let o = self.ports.outside.observe(&w);
        let activity = activity_of(&w, o.as_ref());
        let active = before.is_some_and(|b| activity.moved_since(b));
        let process = a
            .process
            .or_else(|| rows.workers.get(&a.worker_id).and_then(|r| r.agent))
            .map_or(Seen::Unknown, |p| {
                seen.get(&p).copied().unwrap_or(Seen::Unknown)
            });
        let heartbeat_age_s = w
            .last_checkin_at
            .as_deref()
            .and_then(|at| util::seconds_between(at, now));
        let pane = w.pane_id.clone().map(|id| rungs::Pane {
            id,
            status: o
                .as_ref()
                .and_then(|o| o.herdr_status.as_deref())
                .and_then(PaneStatus::parse),
            screen: o.and_then(|o| o.screen),
        });
        let l = LiveSeen {
            attempt: a.clone(),
            worker: w,
            process,
            heartbeat_age_s,
            active,
            pane,
        };
        (l, activity)
    }

    /// The holds of [`Outside::holds`] with no hold of the worktree disk
    /// budget: part 2 gives that hold from the rows of its transaction.
    fn health_holds(&self) -> Vec<Hold> {
        self.ports
            .outside
            .holds()
            .into_iter()
            .filter(|h| !matches!(h, Hold::WorktreeDisk { .. }))
            .collect()
    }

    /// Watch the outboxes and the worktree folders, and give the folders
    /// that changed: the scans read them first.
    fn changed_folders(&mut self, outboxes: &[FolderItem], trees: &[FolderItem]) -> Vec<PathBuf> {
        let watched: Vec<PathBuf> = outboxes
            .iter()
            .chain(trees)
            .map(|i| i.path.clone())
            .collect();
        self.watch.follow(&watched);
        let changed = self.watch.changed();
        if self.settings.faults.has(DROP_WATCH_EVENTS) {
            return Vec::new();
        }
        changed
    }

    // ─── part 2 ─────────────────────────────────────────────────────────

    fn decide(
        &mut self,
        conn: &mut Connection,
        read: Read,
        deadline: Deadline,
    ) -> Result<Decided, PassError> {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)?;
        match fleetd_row::check_generation(&tx, self.settings.generation) {
            Ok(()) => {}
            Err(e @ DoorError::GenerationLost { .. }) => return Err(PassError::GenerationLost(e)),
            Err(e) => return Err(PassError::Failed(e.into())),
        }
        let period = self.settings.config.pass_period_s;
        let tick = self.clock.tick(&tx, &read.awake, period)?;
        let clock = tick.clock;
        for (kind, cursor) in &read.cursors {
            cursors_store::save(&tx, kind.as_str(), cursor)?;
        }
        cursors_store::save(&tx, LIVE_ATTEMPTS, &read.live_cursor)?;
        // The files of the attempts and the jobs come before the leases and
        // the job observations: a result of this pass wins over an expiry
        // of this pass ("Order inside 1 pass").
        let guard = Some(self.settings.generation);
        outbox_ingest::ingest(&tx, guard, &read.outboxes)?;
        let limit_failed = context_limits(&tx, guard, &self.settings, &read.p7)?;
        let at = util::now();
        let wrappers = attempt_files::ingest(&tx, guard, &read.wrappers, clock, &at, &OsProcesses)?;
        start_running(&tx, guard, clock, &wrappers.checkins, |persona| {
            let roster = read.pools.as_ref().map(|p| &p.roster);
            seconds(profile_of(roster, persona).liveness.lease_s)
        })?;
        jobs::ingest_results(&self.job_ctx(), &tx, &read.job_files)?;
        token_item(&tx, &self.settings.fleet_root, read.token_fault.as_deref())?;
        let launches = self.observe_intents(&tx, &read.launching, clock)?;
        let mut stops = commands::mark_orphans(&tx, &read.task)?;
        let pools = read.pools.as_ref().map(|p| Pools {
            roster: &p.roster,
            quota: &p.quota,
            full_use: self.settings.config.pool_full_use,
        });
        let rules = Rules {
            tx: &tx,
            guard: Some(self.settings.generation),
            clock,
            pools: pools.as_ref(),
            config: &self.settings.config,
            needs_you: self.ports.needs_you,
            sent: &read.sent,
        };
        let mut doorbells = Vec::new();
        rules.run(&read, &mut stops, &mut doorbells)?;
        reconcile_worktrees(&self.job_ctx(), &tx, &read.worktrees, clock)?;
        let disk = disk_budget(&self.job_ctx(), &tx, clock)?;
        let mut holds = read.holds;
        holds.extend(disk.hold.map(|cause| Hold::WorktreeDisk { cause }));
        let (paced, personas) = pacing(read.pools.as_ref());
        holds.extend(paced);
        {
            let now = RefCell::new(&mut self.now);
            let time_up = || deadline.reached((now.borrow_mut())());
            let duty = DutyCtx {
                guard: self.settings.generation,
                clock,
                facts: &read.task,
                budget: DutyBudget {
                    rows: PassBudget::new(self.settings.config.pass_budget),
                    time_up: &time_up,
                },
            };
            stops.extend(self.ports.duties.run(&tx, &duty)?);
        }
        let mut work = WorkRows::default();
        if let Some(w) = &self.work {
            work = self.dispatch_rows(&tx, w, &holds, &personas, clock, &read.stamp)?;
            stops.append(&mut work.stops);
        }
        let ctx = self.job_ctx();
        let observed = jobs::observe(&ctx, &tx, &read.jobs, clock)?;
        work.failed.extend(limit_failed);
        work.failed
            .extend(self.p7_rows(&tx, &read.p7, read.pools.as_ref(), clock)?);
        if self.work.is_some() {
            let policy = |t: &tasks::Task| rules.policy(t.primary_persona.as_deref());
            let v = Verifier {
                jobs: &ctx,
                clock,
                tips: &read.tips,
                policy: &policy,
            };
            work.passed = verifier::verify(&v, &tx)?.passed;
            let r = repair::Results {
                jobs: &ctx,
                clock,
                tips: &read.tips,
                fleet_root: &self.settings.fleet_root,
            };
            repair::observe(&r, &tx)?;
        }
        let integration = match self.work {
            Some(w) => self.integrate(&tx, &w, clock, &read.stamp)?,
            None => Integrated::default(),
        };
        rules.check_i1()?;
        let breaches = self.check_invariants(&tx, &read.worktrees)?;
        self.queue_lists(&ctx, &tx, &read.repos, clock)?;
        record_hold_changes(&tx, &self.holds, &holds)?;
        let mut effects = self.intended_effects(
            &tx,
            ToLaunch {
                attempts: launches,
                push: work.launches,
            },
            stops,
            observed.stops,
            clock,
            &read.stamp,
        )?;
        effects.extend(doorbells.into_iter().map(Effect::Doorbell));
        effects.sort_by_key(Effect::order);
        self.settings.faults.abort_if(ABORT_BEFORE_COMMIT);
        tx.commit().map_err(anyhow::Error::from)?;
        self.clock.committed(tick);
        self.holds = holds;
        Ok(Decided {
            clock,
            effects,
            breaches,
            may_stop: read.may_stop,
            activity: read.activity,
            sent: read.sent,
            passed: work.passed,
            roster: read.pools.map(|p| p.roster),
            failed: work.failed,
            integration,
        })
    }

    /// The Phase 4 rows of part 2 that come before the job observations,
    /// after the stops of `mark_orphans`, the rules of the leases and the
    /// worktree reconciler, so `holds` holds the disk budget of this pass:
    /// the binds of the ready tasks, at most `pass_budget`; the hold and
    /// the needs-you item of a persona with no check sandbox; the resolver
    /// and fix attempts that are due; and the stops and launches of the
    /// push attempts in `launching` (ticket 10).
    fn dispatch_rows(
        &self,
        tx: &Transaction<'_>,
        w: &TaskWork<'_>,
        holds: &[Hold],
        paced: &BTreeMap<String, String>,
        clock: FleetSeconds,
        stamp: &str,
    ) -> Result<WorkRows> {
        let budget = PassBudget::new(self.settings.config.pass_budget).rows();
        let ctx = DispatchCtx {
            jobs: self.job_ctx(),
            clock,
            git: &w.tools.git,
        };
        let mut d = dispatch::dispatch_due(
            tx,
            &ctx,
            &dispatch_holds(holds, paced, clock),
            w.sandboxes,
            budget,
        )?;
        w.sandboxes.move_to_fallbacks(tx, &mut d.held)?;
        w.sandboxes.open_items(tx, self.ports.needs_you, &d.held)?;
        if !d.bound.is_empty() {
            let mut cursor = cursors_store::load(tx, DISPATCH, stamp)?;
            for b in &d.bound {
                cursor.step(&b.task_id, stamp);
            }
            cursors_store::save(tx, DISPATCH, &cursor)?;
        }
        repair::start_due(tx, &ctx, budget)?;
        let plan = push::plan(tx, ctx.jobs.guard, &|r| w.launches.alive(r))?;
        Ok(WorkRows {
            stops: plan.stops,
            launches: plan.launches,
            passed: Vec::new(),
            failed: d
                .failed
                .into_iter()
                .map(|(task, e)| format!("the bind of task {task}: {e}"))
                .collect(),
        })
    }

    /// Part 3 of the verifier, after the effects of the pass: the
    /// structural check and the review steps of each task of `passed`, in
    /// the time left, then the file of each committed verdict. A task that
    /// part 3 does not reach is passed again by the verifier of the next
    /// pass. Gives the lines of what failed.
    fn verify_after_commit(
        &mut self,
        conn: &mut Connection,
        w: TaskWork<'_>,
        decided: &Decided,
        passed: Vec<Passed>,
        deadline: Deadline,
    ) -> Vec<String> {
        let mut failed = Vec::new();
        if !passed.is_empty() && !deadline.reached((self.now)()) {
            if let Err(e) = self.review_steps(conn, w, decided, passed) {
                failed.push(format!("the review steps: {e:#}"));
            }
        }
        if let Err(e) = verifier::write_verdicts(conn, &self.settings.fleet_root) {
            failed.push(format!("the verdict files: {e:#}"));
        }
        failed
    }

    /// The structural check and the review steps of `passed`, in a store of
    /// the ledger of `conn` with no transaction open.
    fn review_steps(
        &self,
        conn: &Connection,
        w: TaskWork<'_>,
        decided: &Decided,
        passed: Vec<Passed>,
    ) -> Result<()> {
        let path = conn
            .path()
            .filter(|p| !p.is_empty())
            .context("the ledger has no file")?;
        let mut store = Store::open(Path::new(path))?;
        let ctx = self.job_ctx();
        let roster = decided.roster.as_ref();
        let config = &self.settings.config;
        let policy = |t: &tasks::Task| retry_policy(roster, config, t.primary_persona.as_deref());
        let r = review::Review {
            jobs: &ctx,
            clock: decided.clock,
            tools: w.tools,
            policy: &policy,
        };
        verifier::structural(&r, &mut store, &passed)?;
        verifier::review(&r, &mut store, passed)?;
        Ok(())
    }

    fn job_ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: Some(self.settings.generation),
            config: &self.settings.config,
            needs_you: self.ports.needs_you,
        }
    }

    /// Each attempt in `launching`: a live process of its attempt id ->
    /// `live`, with the process of its launch on its row; no process past
    /// the start timeout -> `ended` with `timed_out`; no launch yet -> an
    /// intended launch, returned.
    fn observe_intents(
        &self,
        tx: &Transaction<'_>,
        launching: &[Launching],
        clock: FleetSeconds,
    ) -> Result<Vec<AttemptRow>> {
        let mut launches = Vec::new();
        for l in launching {
            let a = &l.row;
            let state = door::state_of(tx, Entity::Attempt, &a.id)?;
            if state.as_deref() != Some(AttemptState::Launching.as_str()) {
                continue;
            }
            match observe_launch(l.row_process, l.launch, clock) {
                LaunchStep::Launch if self.ports.duties.launches(a) => {}
                // The launch plan of Phase 4 launches a push attempt.
                LaunchStep::Launch if via(tx, &a.id)? == PUSH => {}
                LaunchStep::Launch => launches.push(a.clone()),
                LaunchStep::Wait => {}
                LaunchStep::Live => {
                    let guard = Some(self.settings.generation);
                    commands::attempt_live(tx, guard, &a.id, l.launched)?;
                }
                LaunchStep::TimedOut => {
                    let reap = Reap {
                        attempt: a.id.clone(),
                        worker: a.worker_id.clone(),
                        outcome: TIMED_OUT.to_string(),
                        end_worker: false,
                        task: None,
                        cleanup: Default::default(),
                    };
                    commands::end_attempt(tx, &reap)?;
                }
            }
        }
        Ok(launches)
    }

    /// I1 and I4: a worker or a task with more than 1 attempt that holds
    /// it gets 1 needs-you item. I6: each breach that the worktree facts
    /// show gets 1 needs-you item. An item of the last pass whose breach
    /// ended is resolved. Returns the keys of the open breaches.
    fn check_invariants(
        &self,
        tx: &Transaction<'_>,
        worktrees: &WorktreeFacts,
    ) -> Result<BTreeSet<String>> {
        let rows = attempts_store::rows(tx, &util::host_name())?;
        let holding: Vec<Holding<'_>> = rows
            .attempts
            .iter()
            .filter(|a| a.state.holds())
            .map(|a| Holding {
                attempt: &a.id,
                worker: &a.worker_id,
                task: a.task_id.as_deref(),
            })
            .collect();
        let mut keys = BTreeSet::new();
        for b in breaches(&holding) {
            let (entity, id, rule, list) = match &b {
                Breach::WorkerAttempts { worker, attempts } => {
                    (Entity::Worker, worker, "I4", attempts)
                }
                Breach::TaskAttempts { task, attempts } => (Entity::Task, task, "I1", attempts),
            };
            let key = format!("{BREACH}:{rule}:{entity}:{id}");
            let text = format!(
                "{rule}: {entity} {id} has {} attempts that are not ended: {}. fleetd does not \
                 end an attempt that may run. End the attempts that do not run.",
                list.len(),
                list.join(", ")
            );
            let item = NewItem {
                key: &key,
                kind: BREACH,
                entity,
                entity_id: id,
                text: &text,
            };
            self.ports.needs_you.open_item(tx, &item)?;
            keys.insert(key);
        }
        for b in check_i6(tx, worktrees)? {
            let Some((id, text)) = i6_text(&b) else {
                continue;
            };
            let entity = Entity::Worktree;
            let key = format!("{BREACH}:I6:{entity}:{id}");
            let item = NewItem {
                key: &key,
                kind: BREACH,
                entity,
                entity_id: &id,
                text: &text,
            };
            self.ports.needs_you.open_item(tx, &item)?;
            keys.insert(key);
        }
        for gone in self.breaches.difference(&keys) {
            self.ports.needs_you.resolve_item(tx, gone)?;
        }
        Ok(keys)
    }

    /// 1 `worktree_list` job for each repository of `repos` in each period
    /// of `worktree_list_period_s` on the fleet clock: a repository whose
    /// run of this period exists, in any state, gets no other (RR-10, Q11).
    fn queue_lists(
        &self,
        ctx: &JobCtx<'_>,
        tx: &Transaction<'_>,
        repos: &[String],
        clock: FleetSeconds,
    ) -> Result<()> {
        let key = worktree_list_key(clock, self.settings.config.worktree_list_period_s);
        for repo in repos {
            if jobs_store::latest(tx, Kind::WorktreeList, repo, &key)?.is_some() {
                continue;
            }
            let job = JobKey {
                kind: Kind::WorktreeList,
                subject: repo,
                input_key: &key,
            };
            jobs::queue(ctx, tx, &job, Some(repo), clock)?;
        }
        Ok(())
    }

    /// The effects of the rows in an intended state, stops first. Launches
    /// and job starts each take at most `pass_budget` rows, from their
    /// cursors.
    fn intended_effects(
        &self,
        tx: &Transaction<'_>,
        launches: ToLaunch,
        stops: Vec<Stop>,
        job_stops: Vec<jobs::Stop>,
        clock: FleetSeconds,
        stamp: &str,
    ) -> Result<Vec<Effect>> {
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let mut effects: Vec<Effect> = Vec::new();
        let mut targets = Vec::new();
        for s in stops {
            if !targets.contains(&s.target) {
                targets.push(s.target.clone());
                effects.push(Effect::Stop(s));
            }
        }
        effects.extend(job_stops.into_iter().map(Effect::StopJob));
        let ids: Vec<String> = launches
            .attempts
            .iter()
            .map(|a| a.id.clone())
            .chain(launches.push.iter().cloned())
            .collect();
        for id in take(tx, LAUNCHES, &ids, budget, stamp)? {
            match launches.attempts.iter().find(|a| a.id == id) {
                Some(a) => effects.push(Effect::Launch(a.clone())),
                None => effects.push(Effect::PushLaunch(id)),
            }
        }
        let due = jobs::due_to_start(tx, clock)?;
        let ids: Vec<String> = due.iter().map(|j| job_cursor_key(j.id)).collect();
        let startable = startable(tx, &due, &ids, budget, stamp)?;
        for id in take(tx, JOB_STARTS, &startable, budget, stamp)? {
            let i = ids
                .iter()
                .position(|k| *k == id)
                .expect("a key of the list");
            effects.push(Effect::StartJob(due[i].clone()));
        }
        Ok(effects)
    }

    // ─── part 3 ─────────────────────────────────────────────────────────

    fn do_effects(
        &mut self,
        conn: &mut Connection,
        effects: Vec<Effect>,
        mut sent: BTreeMap<String, BTreeSet<i64>>,
        clock: FleetSeconds,
        deadline: Deadline,
    ) -> (PassReport, BTreeSet<(u32, u64)>) {
        let mut report = PassReport::default();
        let mut stops = PaneStops::of(&effects);
        let total = effects.len();
        for (i, e) in effects.into_iter().enumerate() {
            if deadline.reached((self.now)()) {
                report.left = total - i;
                break;
            }
            let done = match e {
                Effect::Stop(s) => self.stop(conn, &s, clock, &mut stops, &mut report),
                Effect::StopJob(s) => jobs::stop(&s).then_some(Done::StopJob(s.job_id)),
                Effect::Doorbell(bell) => self.doorbell(bell, &mut sent, &mut report),
                Effect::Launch(a) if quota::is_restart(&a) => {
                    self.restart(conn, &a, clock, &mut report)
                }
                Effect::Launch(a) => self.launch(&a, clock, &mut report),
                Effect::PushLaunch(id) => self.push_launch(conn, &id, clock, &mut report),
                Effect::StartJob(job) => self.start_job(conn, &job, clock, &mut report),
            };
            report.done.extend(done);
        }
        (report, stops.pending())
    }

    fn stop(
        &mut self,
        conn: &mut Connection,
        s: &Stop,
        clock: FleetSeconds,
        stops: &mut PaneStops,
        report: &mut PassReport,
    ) -> Option<Done> {
        match &s.target {
            Target::Process {
                pid,
                pid_start,
                scope,
            } => {
                let process = (*pid, *pid_start);
                let (done, kill) =
                    self.stop_process(conn, &s.row, process, *scope, clock, stops, report)?;
                self.mark_stop(conn, &s.row, process, kill, clock, report);
                Some(done)
            }
            Target::Pane { pane, shell } if stops.may_close(&s.row) => {
                self.close_pane(pane, *shell, stops, report)
            }
            Target::Pane { .. } => None,
        }
    }

    /// The step of the stop of `process` of `row` at `clock`: the polite
    /// stop, the wait in its grace, or the kill. Its record of the polite
    /// stop holds the pane that the process proves.
    #[allow(clippy::too_many_arguments)]
    fn stop_process(
        &self,
        conn: &mut Connection,
        row: &RowRef,
        (pid, pid_start): (u32, u64),
        scope: ProcessScope,
        clock: FleetSeconds,
        stops: &mut PaneStops,
        report: &mut PassReport,
    ) -> Option<(Done, bool)> {
        let guard = self.settings.generation;
        let failed = |e: String| format!("the stop of process {pid}: {e}");
        let polite = match polite_stops_store::get(conn, pid, pid_start) {
            Ok(p) => p,
            Err(e) => {
                stops.hold(row);
                report.failed.push(failed(format!("{e:#}")));
                return None;
            }
        };
        let grace = self.settings.config.stop_grace_s;
        let pane = stops.pane_of(pid, pid_start);
        match stop_step(polite.as_ref().map(|p| p.at), clock, grace) {
            StopStep::Polite => {
                stops.hold(row);
                // The row comes first: a restart after the signal still
                // counts its grace and still has the proof of the pane.
                let record = polite_stops_store::Polite { at: clock, pane };
                if let Err(e) = polite_stops_store::record(conn, guard, pid, pid_start, &record) {
                    report.failed.push(failed(format!("{e:#}")));
                    return None;
                }
                effects::stop_process(pid, pid_start, scope, false)
                    .then(|| (Done::Stop(format!("process {pid}")), false))
            }
            StopStep::Wait => {
                stops.hold(row);
                // A pane proven after the polite stop goes on its row, so
                // the pane closes when the process ends.
                if let Some(p) = polite.filter(|p| pane.is_some() && pane != p.pane) {
                    let record = polite_stops_store::Polite { pane, ..p };
                    if let Err(e) = polite_stops_store::record(conn, guard, pid, pid_start, &record)
                    {
                        report.failed.push(failed(format!("{e:#}")));
                    }
                }
                None
            }
            StopStep::Kill => effects::stop_process(pid, pid_start, scope, true)
                .then(|| (Done::Stop(format!("kill process {pid}")), true)),
        }
    }

    /// The sent mark of 1 stop that part 3 sent (A24): its own transaction
    /// after the signal. `kill`: the hard stop. A failed mark is no proof,
    /// so a `stalled` task that needs it stays `stalled`.
    fn mark_stop(
        &self,
        conn: &mut Connection,
        row: &RowRef,
        process: (u32, u64),
        kill: bool,
        clock: FleetSeconds,
        report: &mut PassReport,
    ) {
        let guard = Some(self.settings.generation);
        let marked = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)
            .and_then(|tx| {
                stop_sent(&tx, guard, row, process, kill, clock.0)?;
                tx.commit()?;
                Ok(())
            });
        if let Err(e) = marked {
            let pid = process.0;
            report
                .failed
                .push(format!("the sent mark of the stop of process {pid}: {e:#}"));
        }
    }

    /// Close `pane`, which `shell` proves.
    fn close_pane(
        &self,
        pane: &str,
        shell: Recorded,
        stops: &mut PaneStops,
        report: &mut PassReport,
    ) -> Option<Done> {
        match self.ports.outside.close_pane(pane) {
            Ok(()) => {
                stops.closed(shell);
                Some(Done::Stop(format!("pane {pane}")))
            }
            Err(e) => {
                report.failed.push(format!("closing pane {pane}: {e:#}"));
                None
            }
        }
    }

    fn launch(
        &mut self,
        a: &AttemptRow,
        clock: FleetSeconds,
        report: &mut PassReport,
    ) -> Option<Done> {
        let outside = self.ports.outside;
        let spec = || outside.attempt_spec(a);
        let root = &self.settings.fleet_root;
        match effects::launch_attempt(root, &a.id, clock, spec, self.ports.host) {
            Ok(Some(launched)) => {
                self.launched.push(launched.child);
                Some(Done::Launch(a.id.clone()))
            }
            Ok(None) => None,
            Err(e) => {
                report.failed.push(format!("{e:#}"));
                None
            }
        }
    }

    /// Spawn the push attempt `id` through the launcher (ticket 10): the
    /// spawn decides again on the rows of now, and writes its launch record
    /// before the process starts, so it is launched at most once.
    fn push_launch(
        &mut self,
        conn: &Connection,
        id: &str,
        clock: FleetSeconds,
        report: &mut PassReport,
    ) -> Option<Done> {
        let launches = self.work?.launches;
        let root = &self.settings.fleet_root;
        match push::launch(conn, root, clock, id, self.ports.host, launches) {
            Ok(push::Launch::Launched(started)) => {
                self.launched.push(started.child);
                Some(Done::Launch(id.to_string()))
            }
            Ok(push::Launch::Done) => None,
            Ok(push::Launch::Waiting(why)) => {
                report
                    .failed
                    .push(format!("the launch of attempt {id} waits: {why}"));
                None
            }
            Err(e) => {
                report
                    .failed
                    .push(format!("the launch of attempt {id}: {e:#}"));
                None
            }
        }
    }

    /// Type the doorbell `bell` into its pane, then mark its event as
    /// typed in `sent` and its file. A doorbell that a crash or an error
    /// lost has no mark: the next pass types it again, and the ladder does
    /// not count it until then.
    fn doorbell(
        &self,
        bell: Bell,
        sent: &mut BTreeMap<String, BTreeSet<i64>>,
        report: &mut PassReport,
    ) -> Option<Done> {
        let pane = bell.pane;
        match self.ports.outside.doorbell(&pane) {
            Ok(()) => {
                sent.entry(bell.attempt).or_default().insert(bell.event);
                if let Err(e) = effects::write_doorbells(&self.settings.fleet_root, sent) {
                    report.failed.push(format!("the doorbell marks: {e:#}"));
                }
                Some(Done::Doorbell(pane))
            }
            Err(e) => {
                report
                    .failed
                    .push(format!("the doorbell of pane {pane}: {e:#}"));
                None
            }
        }
    }

    /// Start the fallback worker of the restart attempt `a` (RR-11), once,
    /// and only when each process of an older attempt of its task is gone
    /// (FM-14).
    fn restart(
        &mut self,
        conn: &Connection,
        a: &AttemptRow,
        clock: FleetSeconds,
        report: &mut PassReport,
    ) -> Option<Done> {
        let root = &self.settings.fleet_root;
        let ports = OutsideRestarts(self.ports.outside);
        match quota::restart(conn, root, clock, &a.id, self.ports.host, &ports) {
            Ok(Restarted::Launched(l)) => {
                self.launched.push(l.child);
                Some(Done::Launch(a.id.clone()))
            }
            Ok(Restarted::Done | Restarted::Waiting { .. }) => None,
            Err(e) => {
                report.failed.push(format!("{e:#}"));
                None
            }
        }
    }

    fn start_job(
        &mut self,
        conn: &mut Connection,
        job: &JobRow,
        clock: FleetSeconds,
        report: &mut PassReport,
    ) -> Option<Done> {
        let spec = jobs::job_spec(&self.settings.horch, job.id, None);
        let ctx = JobCtx {
            guard: Some(self.settings.generation),
            config: &self.settings.config,
            needs_you: self.ports.needs_you,
        };
        match self.jobs.start(&ctx, conn, job, &spec, clock) {
            Ok(true) => Some(Done::StartJob(job.id)),
            Ok(false) => None,
            Err(e) => {
                report.failed.push(format!("{e:#}"));
                None
            }
        }
    }

    // ─── the heartbeat ──────────────────────────────────────────────────

    fn heartbeat(&self) -> Heartbeat {
        let pid = std::process::id();
        let awake = clock_store::awake_now().unwrap_or(AwakeReading {
            boot_id: String::new(),
            awake_ms: 0,
        });
        Heartbeat {
            pid,
            pid_start: horch_platform::process::start_time(pid).unwrap_or(0),
            generation: self.settings.generation,
            awake,
            at: util::now(),
            exe: self.exe.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

// ─── the rules of Phase 2 in part 2 ─────────────────────────────────────

/// The rules of Phase 2 in the transaction of part 2
/// (`docs/specs/fleetd/design.md`, "The reconcile pass" and "Order inside
/// 1 pass"). Each rule writes rows only. A stop or a doorbell is returned
/// and part 3 does it after the commit.
struct Rules<'a> {
    tx: &'a Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    pools: Option<&'a Pools<'a>>,
    config: &'a FleetConfig,
    needs_you: &'a dyn NeedsYouItems,
    /// The doorbell events that part 3 typed, of each live attempt.
    sent: &'a BTreeMap<String, BTreeSet<i64>>,
}

impl Rules<'_> {
    /// The rules in the order of the design, after `mark_orphans`:
    /// 1. `finish_exited`: rung 5, then the quota rule or rung 6;
    /// 2. `complete_cancels`;
    /// 3. `promote_pending` (I2);
    /// 4. for each live attempt: `renew_or_expire_lease`, then the stall
    ///    ladder, then the quota rule or rung 6 for each attempt it ended;
    /// 5. `settle_stalled`: rung 6 for each `stalled` task whose old
    ///    process is gone (FM-14);
    /// 6. `release_reset_pools` and `escalate_overdue`.
    ///
    /// So a result of this pass wins over a lease expiry of this pass: an
    /// attempt that step 1 ended holds no lease in step 4.
    fn run(&self, read: &Read, stops: &mut Vec<Stop>, doorbells: &mut Vec<Bell>) -> Result<()> {
        stops.extend(self.finish_exited(&read.task)?);
        cancel::complete_cancels(self.tx, &read.task)?;
        self.promote_pending()?;
        self.live_attempts(&read.live, stops, doorbells)?;
        self.settle_stalled(&read.task)?;
        self.finish_idle(&read.live)?;
        if let Some(pools) = self.pools {
            quota::release_reset_pools(self.tx, self.guard, pools)?;
        }
        retry::escalate_overdue(self.tx, self.clock, self.config.replan_limit_s)?;
        Ok(())
    }

    /// The profile of `persona` in the roster of the pass; the design
    /// defaults with no roster or no persona.
    fn profile(&self, persona: Option<&str>) -> Profile {
        profile_of(self.pools.map(|p| p.roster), persona)
    }

    fn persona_of(&self, worker: &str) -> Result<Option<String>> {
        Ok(workers::find(self.tx, worker)?.and_then(|w| w.persona))
    }

    /// The lease numbers of the worker `worker`.
    fn limits(&self, worker: &str) -> Limits {
        let persona = self.persona_of(worker).ok().flatten();
        let l = self.profile(persona.as_deref()).liveness;
        Limits {
            lease_s: seconds(l.lease_s),
            stall_after_s: seconds(l.stall_after_s),
            heartbeat_max_age_s: seconds(self.config.heartbeat_max_age_s),
        }
    }

    /// Whether the pool of `persona` is limited (MAP.md, Q9).
    fn pool_limited(&self, persona: Option<&str>) -> bool {
        let (Some(p), Some(persona)) = (self.pools, persona) else {
            return false;
        };
        p.reading(persona)
            .is_some_and(|r| limited(&r, p.full_use, p.quota.now.timestamp()))
    }

    /// After the attempt `attempt` of task `task` ended with `outcome`:
    /// the quota rule for `quota`, rung 6 for an outcome that burns an
    /// attempt. `asked` and `killed` have their own paths. `stalled` waits:
    /// `settle_stalled` does its rung 6 after the old process is stopped
    /// (FM-14).
    fn after_end(
        &self,
        attempt: &str,
        task: &str,
        persona: Option<&str>,
        outcome: Outcome,
        why: &str,
    ) -> Result<()> {
        if outcome == Outcome::Quota {
            if let Some(pools) = self.pools {
                quota::on_quota(self.tx, self.guard, attempt, pools)?;
            }
            return Ok(());
        }
        if !outcome.burns_attempt()
            || outcome == Outcome::Stalled
            || tasks::cancel_requested(self.tx, task)?.is_some()
        {
            return Ok(());
        }
        retry::burn_attempt(self.tx, task, outcome, &self.policy(persona), why)?;
        Ok(())
    }

    /// The numbers of rung 6 for a task of `persona`.
    fn policy(&self, persona: Option<&str>) -> retry::Policy {
        retry_policy(self.pools.map(|p| p.roster), self.config, persona)
    }

    /// Step 5: rung 6 for each `stalled` task whose old process the pass
    /// sees gone. A task whose old process lives stays `stalled`, so no
    /// claim starts a new attempt beside it.
    fn settle_stalled(&self, facts: &Facts) -> Result<()> {
        let bound = rungs::stop_bound_s(self.config.stop_grace_s, self.config.pass_period_s);
        let bounds = ladder::StopBounds {
            bound_s: bound,
            stop_grace_s: self.config.stop_grace_s,
        };
        let killed_at = |p: (u32, u64)| stop_sent_at(self.tx, p, true);
        for s in ladder::settle_stalled(self.tx, self.clock.0, facts, bounds, &killed_at)? {
            let persona = self.persona_of(&s.worker)?;
            let why = format!("the process of attempt {} is gone", s.attempt);
            let policy = self.policy(persona.as_deref());
            retry::burn_attempt(self.tx, &s.task, Outcome::Stalled, &policy, &why)?;
        }
        Ok(())
    }

    /// I1, process part, with rung 5: each live attempt whose process is
    /// gone ends with `asked` (an open question: the task waits in
    /// `blocked_on_user`), `quota` (a limited pool: the quota rule) or
    /// `no_result` (rung 6). An attempt whose worker row ended first keeps
    /// the path of Phase 1.
    fn finish_exited(&self, facts: &Facts) -> Result<Vec<Stop>> {
        let rows = attempts_store::rows(self.tx, &util::host_name())?;
        let mut stops = Vec::new();
        for mut reap in task::dead_attempts(&rows, facts) {
            let why = format!("the process of attempt {} is gone", reap.attempt);
            let Some((task_id, _)) = reap.task.clone() else {
                stops.extend(commands::end_attempt(self.tx, &reap)?);
                continue;
            };
            if !reap.end_worker {
                stops.extend(commands::end_attempt(self.tx, &reap)?);
                commands::schedule_retry(self.tx, &task_id, &why)?;
                continue;
            }
            let persona = self.persona_of(&reap.worker)?;
            let outcome = self.exit_outcome(
                questions_store::open_of_task(self.tx, &task_id)? > 0,
                self.pool_limited(persona.as_deref()),
            );
            reap.outcome = outcome.as_str().to_string();
            stops.extend(commands::end_attempt(self.tx, &reap)?);
            if outcome == Outcome::Asked {
                let to = TaskStatus::BlockedOnUser.as_str();
                let by = cause("rung 5: the worker exited with an open question");
                transition(
                    self.tx,
                    self.guard,
                    Entity::Task,
                    &task_id,
                    to,
                    by,
                    &NeedsYouList,
                )?;
            } else {
                self.after_end(&reap.attempt, &task_id, persona.as_deref(), outcome, &why)?;
            }
        }
        Ok(stops)
    }

    /// Rung 5 of the stall ladder: the outcome of an attempt whose process
    /// is gone, with or without an open question and a limited pool.
    fn exit_outcome(&self, open_question: bool, pool_limited: bool) -> Outcome {
        let exited = rungs::Facts {
            clock: self.clock.0,
            process: Seen::Dead,
            active_at: None,
            claimed_at: self.clock.0,
            pane: None,
            pool_limited,
            open_question,
            waiting_until: None,
            interactive: false,
            task_running: false,
            stall_after_s: 0,
            doorbells: Vec::new(),
        };
        match rungs::decide(&exited) {
            Decision::End { outcome, .. } => outcome,
            _ => Outcome::NoResult,
        }
    }

    /// I2: each `pending` task of an active run whose dependencies are all
    /// met moves to `ready`: `done`, and `landed` for a task with a branch
    /// row.
    fn promote_pending(&self) -> Result<()> {
        for run in watchdog::active_runs(self.tx)? {
            let all = tasks::of_run(self.tx, &run.run_id)?;
            let mut statuses: BTreeMap<String, String> = all
                .iter()
                .map(|t| (t.task_id.clone(), t.status.clone()))
                .collect();
            let mut deps = Vec::new();
            for t in all
                .iter()
                .filter(|t| t.status == TaskStatus::Pending.as_str())
            {
                for b in tasks::blockers(self.tx, &t.task_id)? {
                    deps.push((t.task_id.clone(), b.task_id.clone()));
                    statuses.entry(b.task_id).or_insert(b.status);
                }
            }
            let landed = integration_rows::tasks_landed(self.tx)?;
            for id in promotable(&statuses, &deps, &landed) {
                let to = TaskStatus::Ready.as_str();
                let by = cause("each task that it waits for is done and landed (I2)");
                transition(
                    self.tx,
                    self.guard,
                    Entity::Task,
                    &id,
                    to,
                    by,
                    &NeedsYouList,
                )?;
            }
        }
        Ok(())
    }

    /// Step 4 for the live attempts that part 1 saw: renew or expire each
    /// lease; an expired lease ends its attempt `stalled` with a stop of
    /// its process group (rung 3). Then the stall ladder for each attempt
    /// of a `running` task. Each attempt that ends goes on to the quota
    /// rule or rung 6. A declared wait that holds (FM-3, MAP.md Q23) is
    /// activity, for the lease and for the ladder.
    fn live_attempts(
        &self,
        live: &[LiveSeen],
        stops: &mut Vec<Stop>,
        doorbells: &mut Vec<Bell>,
    ) -> Result<()> {
        let mut out = LadderOut { stops, doorbells };
        let mut waiting_until = BTreeMap::new();
        for l in live {
            let waits = workers::waiting(self.tx, &l.worker.worker_id)?;
            waiting_until.insert(l.attempt.id.clone(), waits.iter().map(|w| w.due_at).max());
        }
        let expired = self.renew_or_expire(live, &waiting_until)?;
        self.end_expired(&expired, &mut out)?;
        let active_at: BTreeMap<String, Option<i64>> = leases_store::held(self.tx)?
            .into_iter()
            .map(|h| (h.attempt_id, h.active_at))
            .collect();
        for l in live {
            let last = active_at.get(&l.attempt.id).copied().flatten();
            self.ladder_step(l, last, waiting_until[&l.attempt.id], &mut out)?;
        }
        Ok(())
    }

    /// Renew or expire the lease of each live attempt; a wait that holds
    /// (`waiting_until`) is activity. Gives the expired leases.
    fn renew_or_expire(
        &self,
        live: &[LiveSeen],
        waiting_until: &BTreeMap<String, Option<i64>>,
    ) -> Result<Vec<lease::Expired>> {
        let observed: BTreeMap<String, lease::Observed> = live
            .iter()
            .map(|l| {
                let o = lease::Observed {
                    process: l.process,
                    heartbeat_age_s: l.heartbeat_age_s,
                    active: l.active || waiting_until[&l.attempt.id].is_some(),
                };
                (l.attempt.id.clone(), o)
            })
            .collect();
        let limits = |worker: &str| self.limits(worker);
        lease::renew_or_expire_leases(self.tx, self.guard, self.clock, &observed, &limits)
    }

    /// Rung 3 for each attempt whose lease `expired`: it ends `stalled`
    /// with a stop of its process group, then the quota rule or rung 6.
    fn end_expired(&self, expired: &[lease::Expired], out: &mut LadderOut<'_>) -> Result<()> {
        let rows = attempts_store::rows(self.tx, &util::host_name())?;
        for e in expired {
            let Some(a) = rows.attempts.iter().find(|a| a.id == e.attempt_id) else {
                continue;
            };
            if a.state == AttemptState::Ended {
                continue;
            }
            let end = Decision::End {
                outcome: Outcome::Stalled,
                stop: true,
            };
            out.take(ladder::apply(self.tx, self.guard, a, self.clock.0, &end)?);
            let persona = self.persona_of(&a.worker_id)?;
            let why = format!("the lease of attempt {} expired", a.id);
            self.after_end(
                &a.id,
                &e.task_id,
                persona.as_deref(),
                Outcome::Stalled,
                &why,
            )?;
        }
        Ok(())
    }

    /// The stall ladder for the live attempt `l` of a `running` task, whose
    /// last activity is `active_at`, and whose waits hold until
    /// `waiting_until`. An attempt that the ladder ends goes on to the
    /// quota rule or rung 6.
    fn ladder_step(
        &self,
        l: &LiveSeen,
        active_at: Option<i64>,
        waiting_until: Option<i64>,
        out: &mut LadderOut<'_>,
    ) -> Result<()> {
        let a = &l.attempt;
        let Some(task_id) = a.task_id.as_deref() else {
            return Ok(());
        };
        let state = door::state_of(self.tx, Entity::Attempt, &a.id)?;
        let task = tasks::get(self.tx, task_id)?;
        if state.as_deref() != Some(AttemptState::Live.as_str())
            || task.status != TaskStatus::Running.as_str()
        {
            return Ok(());
        }
        let bells = doorbells_of(self.tx, &a.id)?;
        let sent = self.sent.get(&a.id);
        let unsent = |b: &&(i64, i64)| !sent.is_some_and(|s| s.contains(&b.0));
        let facts = self.ladder_facts(l, &task, active_at, waiting_until)?;
        let decision = rungs::decide(&facts);
        if matches!(decision, Decision::Doorbell { .. }) && bells.iter().any(|b| unsent(&b)) {
            // A doorbell that part 3 did not type is typed again first.
            out.bells(l, bells.iter().filter(unsent));
            return Ok(());
        }
        out.take(ladder::apply(
            self.tx,
            self.guard,
            a,
            self.clock.0,
            &decision,
        )?);
        let outcome = match decision {
            Decision::End { outcome, .. } => outcome,
            Decision::NeedsYou { .. } => Outcome::Stalled,
            Decision::None => return Ok(()),
            Decision::Doorbell { .. } => {
                let bells = doorbells_of(self.tx, &a.id)?;
                out.bells(l, bells.iter().filter(unsent));
                return Ok(());
            }
        };
        let why = format!("the stall ladder ended attempt {}", a.id);
        let persona = l.worker.persona.as_deref();
        self.after_end(&a.id, task_id, persona, outcome, &why)
    }

    /// The facts of the stall ladder for the live attempt `l` of `task`,
    /// whose last activity is `active_at`, and whose declared waits count
    /// as activity until `waiting_until`. The claim sets `active_at`, so
    /// the claim is the first activity.
    fn ladder_facts(
        &self,
        l: &LiveSeen,
        task: &tasks::Task,
        active_at: Option<i64>,
        waiting_until: Option<i64>,
    ) -> Result<rungs::Facts> {
        let persona = l.worker.persona.as_deref();
        let profile = self.profile(persona);
        Ok(rungs::Facts {
            clock: self.clock.0,
            process: l.process,
            active_at,
            claimed_at: active_at.unwrap_or(self.clock.0),
            pane: l.pane.clone(),
            pool_limited: self.pool_limited(persona),
            open_question: questions_store::open_of_task(self.tx, &task.task_id)? > 0,
            waiting_until,
            // The mode of the launch, not of the profile now: a live
            // interactive attempt keeps rung 5 after an update.
            interactive: workers::attempt_mode(self.tx, &l.worker.worker_id) == Mode::Interactive,
            task_running: task.status == TaskStatus::Running.as_str(),
            stall_after_s: profile.liveness.stall_after_s,
            doorbells: doorbells_of(self.tx, &l.attempt.id)?
                .into_iter()
                .filter(|(id, _)| self.sent.get(&l.attempt.id).is_some_and(|s| s.contains(id)))
                .map(|(_, at)| at)
                .collect(),
        })
    }

    /// Rule D: a `verifying` task has no lease and no ladder (MAP.md, Q20).
    /// When each wait of its author has ended, and the checks of the author
    /// give `no_progress` for stall_after_s, 1 needs-you item names the
    /// task. The pass stops no worker and moves no task. The item resolves
    /// when the task leaves `verifying` or the author shows progress. `live`
    /// is 1 page of the live attempts: an item of a task whose author is not
    /// on the page stays, unless its task left `verifying`.
    fn finish_idle(&self, live: &[LiveSeen]) -> Result<()> {
        let now = util::now();
        let mut idle = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for l in live {
            let Some(task) = l.attempt.task_id.as_deref() else {
                continue;
            };
            seen.insert(format!("{FINISH_IDLE}:{task}"));
            let worker = &l.worker.worker_id;
            if tasks::get(self.tx, task)?.status != TaskStatus::Verifying.as_str()
                || !workers::waiting(self.tx, worker)?.is_empty()
            {
                continue;
            }
            let stall = seconds(
                self.profile(l.worker.persona.as_deref())
                    .liveness
                    .stall_after_s,
            );
            let quiet = watchdog::quiet_since(self.tx, worker)?
                .and_then(|at| util::seconds_between(&at, &now));
            if quiet.is_some_and(|s| s >= stall) {
                idle.insert(format!("{FINISH_IDLE}:{task}"), (task, l.worker.label()));
            }
        }
        for key in needs_you::open_keys(self.tx, FINISH_IDLE)? {
            let ended = if seen.contains(&key) {
                !idle.contains_key(&key)
            } else {
                let task = key.trim_start_matches(FINISH_IDLE).trim_start_matches(':');
                tasks::get(self.tx, task)?.status != TaskStatus::Verifying.as_str()
            };
            if ended {
                self.needs_you.resolve_item(self.tx, &key)?;
            }
        }
        for (key, (task, author)) in &idle {
            let text = format!(
                "{author} on task {task} shows no progress after its waits ended, and the task \
                 is verifying. fleetd does not stop it. Ask it for its status, or stop it."
            );
            let item = NewItem {
                key,
                kind: FINISH_IDLE,
                entity: Entity::Task,
                entity_id: task,
                text: &text,
            };
            self.needs_you.open_item(self.tx, &item)?;
        }
        Ok(())
    }

    /// Step 6: the lease part of I1.
    fn check_i1(&self) -> Result<()> {
        let limits = |worker: &str| self.limits(worker);
        lease::check_i1(self.tx, self.guard, self.clock, &limits)
    }
}

/// The effects of the stall ladder in 1 pass: the stops and the
/// doorbells of part 3.
struct LadderOut<'a> {
    stops: &'a mut Vec<Stop>,
    doorbells: &'a mut Vec<Bell>,
}

impl LadderOut<'_> {
    /// The stops of `effects`. A doorbell comes from its event row
    /// ([`LadderOut::bells`]), so that 1 that part 3 did not type is typed
    /// again.
    fn take(&mut self, effects: Vec<ladder::Effect>) {
        for e in effects {
            if let ladder::Effect::Stop(s) = e {
                self.stops.push(s);
            }
        }
    }

    /// The doorbell events `events` (id, fleet clock) of the live attempt
    /// `l`, to type into its pane.
    fn bells<'e>(&mut self, l: &LiveSeen, events: impl Iterator<Item = &'e (i64, i64)>) {
        let Some(pane) = &l.pane else {
            return;
        };
        for (event, _) in events {
            self.doorbells.push(Bell {
                attempt: l.attempt.id.clone(),
                event: *event,
                pane: pane.id.clone(),
            });
        }
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// A number of seconds of a profile as a value of the fleet clock.
fn seconds(s: u64) -> i64 {
    i64::try_from(s).unwrap_or(i64::MAX)
}

/// Record the sent mark of 1 stop of the process `process` (its pid and
/// its start time) of the row `row`: part 3 sent it at the fleet clock
/// `clock`. `kill`: the hard stop, else the polite stop. The row of the
/// stop came first, in the transaction of the decision. The mark comes
/// after the signal, so it is the proof that the stop ran (A24). The first
/// mark of each step stays: a second one is not written.
fn stop_sent(
    tx: &Connection,
    guard: Option<Generation>,
    row: &RowRef,
    process: (u32, u64),
    kill: bool,
    clock: i64,
) -> Result<()> {
    if stop_sent_at(tx, process, kill)?.is_some() {
        return Ok(());
    }
    let (entity, id) = match row {
        RowRef::Worker(id) => (Entity::Worker, id),
        RowRef::Attempt(id) => (Entity::Attempt, id),
    };
    let step = if kill { "hard" } else { "polite" };
    let text = format!("fleetd sent the {step} stop of process {}", process.0);
    let data = serde_json::json!({
        "fleet_clock": clock,
        "pid": process.0,
        "pid_start": process.1,
        "kill": kill,
    });
    record(
        tx,
        guard,
        EventKind::Stopped,
        entity,
        id,
        cause(&text),
        &data,
    )?;
    Ok(())
}

/// The fleet clock of the sent mark of the stop of `process` (its pid and
/// its start time). `kill`: the mark of the hard stop, else of the polite
/// stop. `None`: no mark.
fn stop_sent_at(tx: &Connection, (pid, start): (u32, u64), kill: bool) -> Result<Option<i64>> {
    let start = i64::try_from(start).unwrap_or(i64::MAX);
    Ok(tx.query_row(
        "SELECT min(json_extract(data_json, '$.fleet_clock')) FROM events
         WHERE kind = ?1 AND entity IN (?2, ?3)
           AND json_extract(data_json, '$.pid') = ?4
           AND json_extract(data_json, '$.pid_start') = ?5
           AND json_extract(data_json, '$.kill') = ?6",
        rusqlite::params![
            EventKind::Stopped.as_str(),
            Entity::Worker.as_str(),
            Entity::Attempt.as_str(),
            pid,
            start,
            kill
        ],
        |r| r.get(0),
    )?)
}

/// The event id and the fleet clock of each doorbell of the attempt
/// `attempt`, oldest first.
fn doorbells_of(tx: &Connection, attempt: &str) -> Result<Vec<(i64, i64)>> {
    let mut stmt = tx.prepare(
        "SELECT event_id, data_json FROM events WHERE kind = ?1 AND entity = ?2 AND entity_id = ?3
         ORDER BY event_id",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![
            EventKind::Doorbell.as_str(),
            Entity::Attempt.as_str(),
            attempt
        ],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (id, data) = row?;
        let at = data
            .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
            .and_then(|v| v["fleet_clock"].as_i64());
        out.extend(at.map(|at| (id, at)));
    }
    Ok(out)
}

/// The activity signals of the worker `w` in the observation `o`. Only a
/// progress that the agent recorded itself counts: a proxy checkin of a
/// check is not activity of the agent.
fn activity_of(w: &Worker, o: Option<&Observation>) -> Activity {
    let by_agent = w.last_progress_source.as_deref() == Some(ProgressSource::Agent.as_str());
    let agent = w
        .harness
        .as_deref()
        .and_then(timer::Agent::of_harness)
        .unwrap_or(timer::Agent::Any);
    Activity {
        progress_at: w.last_progress_at.clone().filter(|_| by_agent),
        transcript_bytes: o.and_then(|o| o.transcript_bytes),
        transcript_mtime: o.and_then(|o| o.transcript_mtime.clone()),
        herdr_seq: o.and_then(|o| o.herdr_seq),
        timer_s: o
            .and_then(|o| o.screen.as_deref())
            .and_then(|s| timer::working_seconds(s, agent))
            .map(seconds),
    }
}

/// The key of a recorded process in the process scan. A job process has
/// the id of its row as its key, so the 2 kinds of key never meet.
fn process_key(r: Recorded) -> String {
    match r.pid_start {
        Some(start) => format!("p:{}:{start}", r.pid),
        None => format!("p:{}:-", r.pid),
    }
}

/// The key of a job in the cursor of the job starts: its id with 20 digits,
/// so the order of the keys is the order of the ids.
fn job_cursor_key(id: i64) -> String {
    format!("{id:020}")
}

/// What the process scan saw of `r`.
fn seen(liveness: &BTreeMap<String, Liveness>, r: Option<Recorded>) -> Proc {
    let Some(r) = r else {
        return Proc::Absent;
    };
    match liveness.get(&process_key(r)) {
        Some(Liveness::Alive) => Proc::Alive,
        Some(Liveness::Dead) => Proc::Dead,
        Some(Liveness::Unknown) | None => Proc::Unknown,
    }
}

/// The processes of the reap path: each process that a row names.
fn task_processes(rows: &Rows, liveness: &BTreeMap<String, Liveness>) -> BTreeMap<Recorded, Seen> {
    rows.named_processes()
        .into_iter()
        .map(|r| (r, to_seen(seen(liveness, Some(r)))))
        .collect()
}

/// The facts of the job processes that the scan read.
fn job_facts(jobs: &BTreeSet<String>, liveness: &BTreeMap<String, Liveness>) -> Vec<ProcessFact> {
    jobs.iter()
        .filter_map(|k| {
            Some(ProcessFact {
                key: k.clone(),
                liveness: *liveness.get(k)?,
            })
        })
        .collect()
}

/// The processes that a row names, with a start time, that the scan did
/// not see gone: the record of a polite stop of each stays. A process that
/// the scan did not reach keeps its record, so its grace goes on.
fn may_stop(rows: &Rows, liveness: &BTreeMap<String, Liveness>) -> BTreeSet<(u32, u64)> {
    rows.named_processes()
        .into_iter()
        .filter(|r| seen(liveness, Some(*r)) != Proc::Dead)
        .filter_map(|r| Some((r.pid, r.pid_start?)))
        .collect()
}

/// Each attempt in `launching`, with what the scan saw of its processes.
fn launching(
    rows: &Rows,
    records: &BTreeMap<String, LaunchRecord>,
    liveness: &BTreeMap<String, Liveness>,
) -> Vec<Launching> {
    rows.attempts
        .iter()
        .filter(|a| a.state == AttemptState::Launching)
        .map(|a| {
            let record = records.get(&a.id);
            Launching {
                row: a.clone(),
                row_process: seen(liveness, a.process),
                launch: record.map(|r| Launch {
                    at: r.launched_at,
                    process: seen(liveness, r.process),
                }),
                launched: record.and_then(|r| r.process),
            }
        })
        .collect()
}

fn to_seen(p: Proc) -> Seen {
    match p {
        Proc::Alive => Seen::Alive,
        Proc::Dead => Seen::Dead,
        Proc::Absent | Proc::Unknown => Seen::Unknown,
    }
}

/// The launch record of each attempt in `launching`.
fn launch_records(root: &std::path::Path, rows: &Rows) -> Result<BTreeMap<String, LaunchRecord>> {
    let mut out = BTreeMap::new();
    for a in rows
        .attempts
        .iter()
        .filter(|a| a.state == AttemptState::Launching)
    {
        if let Some(r) = effects::read_launch(root, &a.id)? {
            out.insert(a.id.clone(), r);
        }
    }
    Ok(out)
}

/// The items of the process scan: each process that a row names, the
/// process of each attempt in `launching` and of its launch, and each job
/// process.
fn process_items(
    rows: &Rows,
    records: &BTreeMap<String, LaunchRecord>,
    jobs: Vec<ProcessItem>,
) -> ProcessItems {
    let launching = rows
        .attempts
        .iter()
        .filter(|a| a.state == AttemptState::Launching)
        .filter_map(|a| a.process);
    let launched = records.values().filter_map(|r| r.process);
    let recorded: BTreeSet<Recorded> = rows
        .named_processes()
        .into_iter()
        .chain(launching)
        .chain(launched)
        .collect();
    let mut items: Vec<ProcessItem> = recorded
        .into_iter()
        .map(|r| ProcessItem {
            key: process_key(r),
            pid: r.pid,
            pid_start: r.pid_start,
        })
        .collect();
    let job_keys = jobs.iter().map(|j| j.key.clone()).collect();
    items.extend(jobs);
    ProcessItems {
        items,
        jobs: job_keys,
    }
}

/// The outbox of each attempt of a task that is not `ended`.
fn outbox_items(root: &std::path::Path, rows: &Rows) -> Vec<FolderItem> {
    rows.attempts
        .iter()
        .filter(|a| a.state != AttemptState::Ended)
        .filter_map(|a| {
            Some(FolderItem {
                key: a.id.clone(),
                path: attempt_ref(a)?.outbox(root),
            })
        })
        .collect()
}

/// The task and the attempt of the row `a`; `None` for an attempt with no
/// task.
fn attempt_ref(a: &AttemptRow) -> Option<AttemptRef> {
    Some(AttemptRef {
        task_id: a.task_id.clone()?,
        attempt_id: a.id.clone(),
    })
}

/// The attempt of each outbox that the outbox scan read, from `rows`. A
/// live attempt whose outbox is missing gets it, with mode 0700, so that
/// an old worker writes files from then (Q3 of
/// `docs/tickets/fleetd-p5/MAP.md`). Gives the attempts, and the lines of
/// the outboxes that could not be made.
fn scanned_attempts(
    root: &Path,
    rows: &Rows,
    scanned: &[OutboxFact],
) -> (Vec<AttemptRef>, Vec<String>) {
    let mut attempts = Vec::new();
    let mut failed = Vec::new();
    for fact in scanned {
        let Some(row) = rows.attempts.iter().find(|a| a.id == fact.key) else {
            continue;
        };
        let Some(attempt) = attempt_ref(row) else {
            continue;
        };
        if row.state == AttemptState::Live && matches!(fact.listing, Listing::Missing) {
            let dir = attempt.outbox(root);
            if let Err(e) = make_outbox(&dir) {
                failed.push(format!("the outbox {}: {e}", dir.display()));
            }
        }
        attempts.push(attempt);
    }
    (attempts, failed)
}

/// Make the outbox `dir` and its missing parents with mode 0700. Where the
/// OS has no modes, the folders get the default permissions.
fn make_outbox(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    let _ = horch_platform::fs::create_dir_mode(&mut builder, OUTBOX_MODE);
    builder.create(dir)?;
    // The umask may have cleared bits of the mode.
    match horch_platform::fs::set_mode(dir, OUTBOX_MODE) {
        Err(e)
            if e.get_ref()
                .is_some_and(|inner| inner.is::<horch_platform::NotSupported>()) =>
        {
            Ok(())
        }
        r => r,
    }
}

/// The profile of `persona` in `roster`; the design defaults with no
/// roster or no persona.
fn profile_of(roster: Option<&Roster>, persona: Option<&str>) -> Profile {
    roster
        .zip(persona)
        .and_then(|(r, name)| r.get(name))
        .map(|t| t.profile())
        .unwrap_or_default()
}

/// The numbers of rung 6 for a task of `persona`.
fn retry_policy(
    roster: Option<&Roster>,
    config: &FleetConfig,
    persona: Option<&str>,
) -> retry::Policy {
    retry::Policy {
        retry: profile_of(roster, persona).retry,
        replan_limit_s: config.replan_limit_s,
    }
}

/// The holds of a dispatch from the global holds `holds` of the pass, at
/// fleet clock `clock`. A pacing hold of a pool is not here: the spawn of
/// the attempt refuses a paced pool, as today.
fn dispatch_holds(
    holds: &[Hold],
    paced: &BTreeMap<String, String>,
    clock: FleetSeconds,
) -> DispatchHolds {
    DispatchHolds {
        clock: clock.0,
        global: holds
            .iter()
            .filter_map(|h| h.global().map(|name| format!("{name}: {}", h.cause())))
            .collect(),
        personas: paced.clone(),
        ..DispatchHolds::default()
    }
}

/// The first heartbeat of the wrapper of a push attempt
/// (`docs/specs/fleetd/design.md`, the task state table, row `leased`):
/// the attempt in `launching` goes `live`, and its `leased` task goes
/// `running` and takes its lease for `lease_s` of the persona of the
/// attempt. `checkins` are the attempts whose heartbeat this pass used. A
/// repair attempt of a `done` task only goes `live`: its bind took the
/// lease.
fn start_running(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    checkins: &[String],
    lease_s: impl Fn(Option<&str>) -> i64,
) -> Result<()> {
    for id in checkins {
        let Some((Some(task_id), state, via, persona)) = attempts_store::attempt_via(tx, id)?
        else {
            continue;
        };
        if via != PUSH {
            continue;
        }
        if state == AttemptState::Launching.as_str() {
            commands::attempt_live(tx, guard, id, None)?;
        } else if state != AttemptState::Live.as_str() {
            continue;
        }
        if tasks::get(tx, &task_id)?.status != TaskStatus::Leased.as_str() {
            continue;
        }
        let text = format!("the first heartbeat of the wrapper of attempt {id}");
        let cause = Cause {
            actor: Actor::Horch,
            text: &text,
        };
        let to = TaskStatus::Running.as_str();
        match transition(tx, guard, Entity::Task, &task_id, to, cause, &NeedsYouList) {
            Ok(_) => lease::take(tx, clock, &task_id, id, lease_s(persona.as_deref()))?,
            // The door recorded the refusal: only this row fails.
            Err(DoorError::Refused { .. }) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// The tip of the task branch of each task in `verifying`, of each task of
/// `results` (a result file of this pass) and of each task whose branch
/// waits for a repair, by task id: the branch of its tree, or the run
/// branch for a task of the shared checkout. Each tip is 1 `git rev-parse`
/// of a local ref, which reads no working tree and no network (PL-5). A
/// branch that git does not have gives no tip.
fn task_tips(
    conn: &Connection,
    git: &Git,
    results: &BTreeSet<&str>,
) -> Result<BTreeMap<String, String>> {
    let mut ids: BTreeSet<String> = attempts_store::tasks_in(conn, TaskStatus::Verifying.as_str())?
        .into_iter()
        .collect();
    for state in [LandingState::Conflicted, LandingState::GateFailed] {
        let branches = integration_rows::task_branches_in(conn, state.as_str())?;
        ids.extend(branches.into_iter().map(|(_, task)| task));
    }
    ids.extend(results.iter().map(|t| t.to_string()));
    let mut tips = BTreeMap::new();
    for id in ids {
        let task = tasks::get(conn, &id)?;
        let tree = trees::of_task(conn, &id)?
            .filter(|r| task.uses_worktree() && !WORKTREES_GONE.contains(&r.status));
        let (repo, branch) = match tree {
            Some(r) => (r.repo_root, r.branch),
            None => {
                let run = runs::get(conn, &task.run_id)?;
                let Some(branch) = run.run_branch.or(run.base_branch) else {
                    continue;
                };
                (run.project_root, branch)
            }
        };
        let spec = format!("refs/heads/{branch}^{{commit}}");
        let out = git.try_run(
            Path::new(&repo),
            &["rev-parse", "--verify", "--quiet", &spec],
        )?;
        if out.ok {
            tips.insert(id, out.stdout.trim().to_string());
        }
    }
    Ok(tips)
}

/// The `via` of the attempt `id`.
fn via(conn: &Connection, id: &str) -> Result<String> {
    let row = attempts_store::attempt_via(conn, id)?;
    row.map(|(_, _, via, _)| via)
        .with_context(|| format!("the ledger has no attempt {id}"))
}

/// Write `task.yaml` and `START.md` of each push attempt in `launching`
/// that has none. Gives the lines of the writes that failed.
fn write_task_files(conn: &Connection, root: &Path) -> Vec<String> {
    match dispatch::write_missing_task_files(conn, root, &[]) {
        Ok(written) => written
            .into_iter()
            .filter_map(|(id, r)| {
                r.err()
                    .map(|e| format!("the task files of attempt {id}: {e:#}"))
            })
            .collect(),
        Err(e) => vec![format!("the task files: {e:#}")],
    }
}

/// Open the needs-you item of the operator token file of `root` with the
/// reason `fault`, or resolve it when `fault` is `None`, in `tx`.
fn token_item(tx: &Connection, root: &Path, fault: Option<&str>) -> Result<()> {
    let path = token_file::path(root).display().to_string();
    let key = format!("{TOKEN_REFUSED}:{path}");
    let Some(fault) = fault else {
        needs_you::resolve(tx, &key)?;
        return Ok(());
    };
    let text = format!(
        "The operator token file {path} fails its checks: {fault}. fleetd applies no operator \
         request until you correct it: give it to the user of fleetd with mode 0600, or remove \
         it and restart fleetd."
    );
    needs_you::open_item(
        tx,
        &NeedsYouWrite {
            key: &key,
            kind: TOKEN_REFUSED,
            entity: "fleetd",
            entity_id: &path,
            text: &text,
        },
    )
}

/// At most `budget` of `keys`, from the cursor `kind`, which moves past
/// them in `tx`.
fn take(
    tx: &Transaction<'_>,
    kind: &str,
    keys: &[String],
    budget: PassBudget,
    stamp: &str,
) -> Result<Vec<String>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut cursor = cursors_store::load(tx, kind, stamp)?;
    let chosen: Vec<String> = cursor
        .take(keys, budget, stamp)
        .into_iter()
        .map(str::to_string)
        .collect();
    cursors_store::save(tx, kind, &cursor)?;
    Ok(chosen)
}

/// The keys of the `due` jobs (with the keys `ids`) that part 3 may start,
/// in the turn order of the cursor of the job starts, at most `budget`: a
/// git writer only when no git writer of its repository runs and none is
/// taken before it in this pass (MAP.md of fleetd-p3, Q12). A writer that
/// waits keeps its turn.
fn startable(
    tx: &Transaction<'_>,
    due: &[JobRow],
    ids: &[String],
    budget: PassBudget,
    stamp: &str,
) -> Result<Vec<String>> {
    let cursor = cursors_store::load(tx, JOB_STARTS, stamp)?;
    let mut writing = BTreeSet::new();
    let mut out = Vec::new();
    for (job, id) in cursor.turns(&due.iter().zip(ids).collect::<Vec<_>>(), |(_, id)| id) {
        if out.len() == budget.rows() {
            break;
        }
        if let (true, Some(repo)) = (jobs::is_git_writer(job.kind), job.repo.as_deref()) {
            if writing.contains(repo) || jobs::git_writer_busy(tx, repo)? {
                continue;
            }
            writing.insert(repo);
        }
        out.push((*id).clone());
    }
    Ok(out)
}

/// The repositories of `repos` that are git repositories: a `.git` is at
/// the root. A folder that is not one has no worktree list.
fn git_repositories(repos: Vec<String>) -> Vec<String> {
    repos
        .into_iter()
        .filter(|r| Path::new(r).join(".git").exists())
        .collect()
}

/// The last passed `worktree_list` run of each of `repos`, read from
/// `jobs`. A run whose result does not parse gives no list.
fn last_lists(conn: &Connection, repos: &[String]) -> Result<Vec<Listed>> {
    let sql = "SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 AND state = ?3
               ORDER BY id DESC LIMIT 1";
    let mut out = Vec::new();
    for repo in repos {
        let args = (Kind::WorktreeList.as_str(), repo, State::Passed.as_str());
        let id: Option<i64> = conn.query_row(sql, args, |r| r.get(0)).optional()?;
        let Some(row) = id
            .map(|id| jobs_store::get(conn, id))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let parsed = jobs::result_data(&row).map(serde_json::from_value::<ListResult>);
        if let Some(Ok(result)) = parsed {
            out.push(Listed {
                repo: repo.clone(),
                job: row.id,
                seen_at: row.ended_at.unwrap_or(row.created_at),
                result,
            });
        }
    }
    Ok(out)
}

/// The items of the scan of worktree folders: the folder of each row, by
/// its worktree id, then each entry of `work_dirs` that no row names, by its
/// path.
fn tree_items(rows: &[FolderItem], work_dirs: &[WorkDir]) -> Vec<FolderItem> {
    let named: BTreeSet<&Path> = rows.iter().map(|r| r.path.as_path()).collect();
    let entries = work_dirs
        .iter()
        .flat_map(|d| &d.folders)
        .filter(|f| !named.contains(Path::new(f)))
        .map(|f| FolderItem {
            key: f.clone(),
            path: PathBuf::from(f),
        });
    rows.iter().cloned().chain(entries).collect()
}

/// The worktree facts of part 1: the scanned folder of each row of `rows`;
/// each scanned entry of `work_dirs` that exists, under its repository; and
/// the `lists`.
fn worktree_facts(
    rows: &[FolderItem],
    work_dirs: &[WorkDir],
    scanned: Vec<FolderFact>,
    lists: Vec<Listed>,
) -> WorktreeFacts {
    let row_keys: BTreeSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    let repo_of: BTreeMap<&str, &str> = work_dirs
        .iter()
        .flat_map(|d| d.folders.iter().map(|f| (f.as_str(), d.repo.as_str())))
        .collect();
    let mut facts = WorktreeFacts {
        lists,
        ..WorktreeFacts::default()
    };
    for fact in scanned {
        if row_keys.contains(fact.key.as_str()) {
            facts.folders.push(fact);
            continue;
        }
        let repo = repo_of.get(fact.key.as_str()).copied();
        let (Presence::Present, Some(repo)) = (&fact.presence, repo) else {
            continue;
        };
        match facts.work_dirs.iter_mut().find(|d| d.repo == repo) {
            Some(d) => d.folders.push(fact.key),
            None => facts.work_dirs.push(WorkDir {
                repo: repo.to_string(),
                folders: vec![fact.key],
            }),
        }
    }
    facts
}

/// The worktree or path, and the text, of the needs-you item of the I6
/// breach `b`. A row with no folder has the item of the reconciler that
/// escalates a vanished folder (FM-27), so it gets none here.
fn i6_text(b: &I6) -> Option<(String, String)> {
    Some(match b {
        I6::NoRow { repo, path } => (
            path.clone(),
            format!(
                "I6: the tree {path} of {repo} has no worktree row, and fleetd did not adopt it. \
                 Find why the adoption failed in the events of the pass."
            ),
        ),
        I6::NoFolder { .. } => return None,
        I6::ReservedWithNoJob { id } => (
            id.clone(),
            format!(
                "I6: the reserved worktree {id} has no worktree_add run. fleetd writes the \
                 row and its run in 1 transaction, so another writer changed the ledger."
            ),
        ),
        I6::RemovedUnsaved { id } => (
            id.clone(),
            format!(
                "I6: the worktree {id} is removed, and its work is not landed and has no \
                 rescue ref. Find its commits in the reflog of its branch."
            ),
        ),
    })
}

// ─── Phase 7 ────────────────────────────────────────────────────────────
//
// `docs/tickets/fleetd-p7/30-pass-wiring.md`. Part 1 scans the CLI
// binaries within the scan budget from its cursor, and reads the worktree
// at the end of each attempt that the wrapper ended at the context limit.
// Part 2 ingests `context_limit` before the wrapper files settle the
// worker (`finish_exited`), then, after the job observations, marks the
// health of the profiles after each ended `conformance` run, adds the
// drift idea of a failed run, and queues the docs jobs (`queue_due_jobs`).
// Part 3 starts the queued jobs with the other job starts of the pass, and
// writes the idea.md of each drift idea. Each mark and each idea is
// idempotent on the id of its run: a note on the run, in the same
// transaction, says that the pass acted on it.

/// The cursor of the scan of the CLI binaries in `pass_cursors`.
const CLI_BINARIES: &str = "cli_binaries";

/// The cause of the note on an ended `conformance` run that the pass acted
/// on: its health marks and its drift idea.
const CONFORMANCE_ACTED: &str = "conformance result acted on";

/// The facts of Phase 7 that part 1 read.
struct P7Read {
    /// The binary id of the horch binary of the job processes; `None` when
    /// it cannot be read: the pass queues no docs job then.
    horch_id: Option<String>,
    /// The CLI binaries that the scan read in this pass.
    clis: Vec<CliBinary>,
    /// The cursor of [`CLI_BINARIES`] after the scan.
    cli_cursor: Cursor,
    /// Each attempt that the wrapper ended at the context limit, with its
    /// worktree at the end.
    limits: Vec<(AttemptRef, retry::TreeAtEnd)>,
}

impl Pass<'_> {
    /// Part 1 of Phase 7: the scan of the CLI binaries, at most
    /// `pass_budget` of them from the cursor until the scan budget ends,
    /// and the worktree of each attempt of `wrappers` whose `exit.json`
    /// says the context limit.
    fn read_p7(
        &mut self,
        conn: &Connection,
        deadline: Deadline,
        stamp: &str,
        wrappers: &[WrapperRead],
    ) -> Result<P7Read> {
        let paths = self.ports.outside.cli_binaries();
        let mut cli_cursor = cursors_store::load(conn, CLI_BINARIES, stamp)?;
        let mut clock = ScanClock {
            deadline: deadline.scan((self.now)(), self.settings.config.scan_budget_ms),
            now: &mut *self.now,
            stamp,
        };
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let read = |c: &CliPath| scan_cli(&c.cli, &c.path);
        let clis = scans::scan_cli_binaries(&paths, budget, &mut cli_cursor, &mut clock, read);
        let clis = clis.facts;
        let git = self.work.map(|w| &w.tools.git);
        let tail = self.settings.config.transcript_tail_bytes;
        let mut limits = Vec::new();
        for w in wrappers.iter().filter(|w| at_context_limit(w)) {
            limits.push((w.attempt.clone(), tree_at_end(conn, git, &w.attempt, tail)?));
        }
        Ok(P7Read {
            horch_id: binary_id(&self.settings.horch),
            clis,
            cli_cursor,
            limits,
        })
    }

    /// Part 2 of Phase 7, after the job observations: the health marks and
    /// the drift idea of each ended `conformance` run, then the docs jobs
    /// that are due. Gives the lines of what failed.
    fn p7_rows(
        &self,
        tx: &Transaction<'_>,
        p7: &P7Read,
        pools: Option<&PoolFacts>,
        clock: FleetSeconds,
    ) -> Result<Vec<String>> {
        cursors_store::save(tx, CLI_BINARIES, &p7.cli_cursor)?;
        let mut failed = Vec::new();
        // With no roster, no profile is known: the runs wait for a pass
        // with one.
        if let Some(p) = pools {
            failed.extend(self.conformance_results(tx, &p.roster)?);
        }
        if let Some(horch_id) = &p7.horch_id {
            queue_docs_jobs(&self.job_ctx(), tx, clock, horch_id, &p7.clis)?;
        }
        Ok(failed)
    }

    /// The health marks of each profile of `roster` after each ended
    /// `conformance` run that the pass did not act on yet, at most
    /// `pass_budget` runs, and the drift idea of a failed run.
    fn conformance_results(&self, tx: &Transaction<'_>, roster: &Roster) -> Result<Vec<String>> {
        let guard = Some(self.settings.generation);
        let budget = PassBudget::new(self.settings.config.pass_budget).rows();
        let profiles: Vec<(&str, HarnessKind)> = roster
            .names()
            .into_iter()
            .filter_map(|name| Some((name, roster.get(name)?.agent)))
            .collect();
        let mut failed = Vec::new();
        for id in ended_not_noted(tx, Kind::Conformance, CONFORMANCE_ACTED, budget)? {
            // A failure keeps the rows of the run unchanged, and the next
            // pass tries the run again.
            match row_alone(tx, || self.act_on_conformance(tx, guard, id, &profiles))? {
                Ok(lines) => failed.extend(lines),
                Err(e) => failed.push(format!("conformance run {id}: {e}")),
            }
        }
        Ok(failed)
    }

    /// The health marks and the drift idea of the ended `conformance` run
    /// `id`, and the note that the pass acted on it. A report that cannot
    /// be read gives no mark: the note records the run, and the line says
    /// why. Gives the lines of what failed.
    fn act_on_conformance(
        &self,
        tx: &Transaction<'_>,
        guard: Option<Generation>,
        id: i64,
        profiles: &[(&str, HarnessKind)],
    ) -> Result<Vec<String>> {
        let mut failed = Vec::new();
        let mut note = serde_json::json!({});
        let row = jobs_store::get(tx, id)?.with_context(|| format!("no job {id}"))?;
        match conformance_report(&row) {
            Ok(None) => {}
            Err(e) => failed.push(format!("the report of conformance run {id}: {e:#}")),
            Ok(Some(report)) => {
                mark_after_conformance(tx, guard, &report, profiles)?;
                if report.failure().is_some() {
                    note = self.drift(tx, &report, &mut failed)?;
                }
            }
        }
        let cause = Cause {
            actor: Actor::Horch,
            text: CONFORMANCE_ACTED,
        };
        record(
            tx,
            guard,
            EventKind::Note,
            Entity::Job,
            &id.to_string(),
            cause,
            &note,
        )?;
        Ok(failed)
    }

    /// Add the drift idea of the failed run `report` to the project of the
    /// fleet: the first repository of [`Outside::repositories`] by path, so
    /// each pass chooses the same one. Gives the data of the note on the
    /// run, which names the project and the idea. With no project, no idea
    /// is added, and `failed` gets 1 line.
    fn drift(
        &self,
        tx: &Transaction<'_>,
        report: &ConformanceReport,
        failed: &mut Vec<String>,
    ) -> Result<serde_json::Value> {
        let mut repos = self.ports.outside.repositories(tx)?;
        repos.sort();
        let git = self
            .work
            .map_or_else(|| Git::new("git"), |w| w.tools.git.clone());
        let project = repos.into_iter().next().and_then(|root| {
            let branch = default_branch(&git, Path::new(&root))?;
            Some((root, branch))
        });
        let Some((root, branch)) = project else {
            failed.push(format!(
                "the drift idea of {} {}: the fleet has no project with a primary branch",
                report.cli, report.version
            ));
            return Ok(serde_json::json!({ "project": null }));
        };
        let target = intake::Target::New(Project {
            root: Path::new(&root),
            primary_branch: &branch,
        });
        let fleet_root = &self.settings.fleet_root;
        let added = add_drift_idea(tx, &self.job_ctx(), report, fleet_root, target)?;
        let idea = added.map(|a| a.file.id);
        Ok(serde_json::json!({ "project": root, "idea": idea }))
    }

    /// Part 3 of Phase 7: write the idea.md of each drift idea that has
    /// none, at most `pass_budget` of them, until the time budget ends.
    fn p7_effects(&mut self, conn: &Connection, deadline: Deadline, report: &mut PassReport) {
        let ideas = self.settings.fleet_root.join("ideas");
        let pending = match pending_idea_files(conn, &ideas) {
            Ok(p) => p,
            Err(e) => {
                report.failed.push(format!("the drift ideas: {e:#}"));
                return;
            }
        };
        let budget = PassBudget::new(self.settings.config.pass_budget).rows();
        for file in pending.iter().filter(|f| f.source == DRIFT).take(budget) {
            if deadline.reached((self.now)()) {
                report.left += 1;
                continue;
            }
            let written = User::current()
                .map_err(anyhow::Error::from)
                .and_then(|owner| write_idea_file(&ideas, owner, file));
            match written {
                Ok(_) => report
                    .done
                    .push(Done::Duty(format!("idea.md of drift idea {}", file.id))),
                Err(e) => report
                    .failed
                    .push(format!("idea.md of drift idea {}: {e:#}", file.id)),
            }
        }
    }
}

/// Part 2 of Phase 7, before the wrapper files settle the worker: each
/// attempt that the wrapper ended at the context limit ends with
/// `context_limit`, burns no attempt, and its task goes to `ready` with a
/// handoff file, or holds at a loop (FM-20, MAP.md of fleetd-p7, Q2).
///
/// A failure keeps the rows of its attempt unchanged, and gives its line:
/// the wrapper files then settle the worker as an exit with no result.
fn context_limits(
    tx: &Transaction<'_>,
    guard: Option<Generation>,
    settings: &Settings,
    p7: &P7Read,
) -> Result<Vec<String>> {
    let tail = settings.config.transcript_tail_bytes;
    let mut failed = Vec::new();
    for (a, tree) in &p7.limits {
        let moved = row_alone(tx, || {
            retry::on_context_limit(tx, guard, &settings.fleet_root, tail, a, tree)
        })?;
        if let Err(e) = moved {
            failed.push(format!(
                "the context limit of attempt {}: {e}",
                a.attempt_id
            ));
        }
    }
    Ok(failed)
}

/// Run `f` in a savepoint of `tx`: an error rolls back only the rows of
/// `f`, and gives its line.
fn row_alone<T>(tx: &Connection, f: impl FnOnce() -> Result<T>) -> Result<Result<T, String>> {
    tx.execute_batch("SAVEPOINT p7_row")?;
    match f() {
        Ok(v) => {
            tx.execute_batch("RELEASE p7_row")?;
            Ok(Ok(v))
        }
        Err(e) => {
            tx.execute_batch("ROLLBACK TO p7_row; RELEASE p7_row")?;
            Ok(Err(format!("{e:#}")))
        }
    }
}

/// Whether the wrapper says in a valid `exit.json` of `w` that it ended
/// the attempt at the context limit.
fn at_context_limit(w: &WrapperRead) -> bool {
    w.exit.as_ref().is_some_and(|f| {
        matches!(&f.found, Found::Valid { file, .. }
            if file.outcome_hint == Some(OutcomeHint::ContextLimit))
    })
}

/// The worktree of the task of `a` at the end of the attempt: its head and
/// its `git diff`, cut to `tail` bytes. With no git, or a git that fails,
/// the head is unknown and the diff is empty: an unknown head never holds
/// the loop guard.
fn tree_at_end(
    conn: &Connection,
    git: Option<&Git>,
    a: &AttemptRef,
    tail: u64,
) -> Result<retry::TreeAtEnd> {
    let Some(git) = git else {
        return Ok(retry::TreeAtEnd::default());
    };
    let task = tasks::get(conn, &a.task_id)?;
    let dir = match trees::of_task(conn, &a.task_id)? {
        Some(r) if task.uses_worktree() => r.path,
        _ => runs::get(conn, &task.run_id)?.project_root,
    };
    let dir = Path::new(&dir);
    let out = |args: &[&str]| {
        git.try_run(dir, args)
            .ok()
            .filter(|o| o.ok)
            .map(|o| o.stdout)
    };
    let head = out(&["rev-parse", "HEAD"]).map(|h| h.trim().to_string());
    let mut diff = out(&["diff"]).unwrap_or_default();
    let mut cut = usize::try_from(tail).unwrap_or(usize::MAX).min(diff.len());
    while !diff.is_char_boundary(cut) {
        cut -= 1;
    }
    diff.truncate(cut);
    Ok(retry::TreeAtEnd { head, diff })
}

/// The report of the ended `conformance` run `row`: its `conformance.json`
/// at the path of its result. `None` when the run wrote no report, as a
/// run whose version command failed.
fn conformance_report(row: &JobRow) -> Result<Option<ConformanceReport>> {
    let Some(path) =
        jobs::result_data(row).and_then(|d| d.get("path")?.as_str().map(PathBuf::from))
    else {
        return Ok(None);
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let report =
        read_conformance_json(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(Some(report))
}

/// The pacing holds of the pools of the roster in `pools`: 1 hold for each
/// paced pool, and the cause of each paced persona. A pool with no fresh
/// reading paces nothing.
fn pacing(pools: Option<&PoolFacts>) -> (Vec<Hold>, BTreeMap<String, String>) {
    let mut paced: Vec<Hold> = Vec::new();
    let mut personas = BTreeMap::new();
    let Some(p) = pools else {
        return (paced, personas);
    };
    for name in p.roster.names() {
        let Some(t) = p.roster.get(name) else {
            continue;
        };
        let model = t.model.as_deref().unwrap_or_default();
        let Some(hold) = holds::pacing(&p.quota, t.agent.as_str(), model) else {
            continue;
        };
        personas.insert(name.to_string(), hold.cause().to_string());
        let pool = |h: &Hold| match h {
            Hold::Pacing { pool, .. } => Some(pool.clone()),
            _ => None,
        };
        if !paced.iter().any(|h| pool(h) == pool(&hold)) {
            paced.push(hold);
        }
    }
    (paced, personas)
}

/// The ids of the ended runs of `kind` with no note of `cause`, oldest
/// first, at most `budget` of them.
fn ended_not_noted(tx: &Connection, kind: Kind, cause: &str, budget: usize) -> Result<Vec<i64>> {
    let sql = "SELECT j.id FROM jobs j WHERE j.kind = ?1 AND j.state IN (?2, ?3)
         AND NOT EXISTS (SELECT 1 FROM events e WHERE e.entity = ?4
             AND e.entity_id = CAST(j.id AS TEXT) AND e.kind = ?5 AND e.cause = ?6)
         ORDER BY j.id LIMIT ?7";
    let mut stmt = tx.prepare(sql)?;
    let ids = stmt.query_map(
        rusqlite::params![
            kind.as_str(),
            State::Passed.as_str(),
            State::Failed.as_str(),
            Entity::Job.as_str(),
            EventKind::Note.as_str(),
            cause,
            i64::try_from(budget).unwrap_or(i64::MAX),
        ],
        |r| r.get(0),
    )?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

// ─── the integration of Phase 4 (ticket 47) ─────────────────────────────

/// The name of the savepoint of the integration rows of part 2.
const INTEGRATION: &str = "integration";

/// The git effects that the integration rows of part 2 intend, to do after
/// the commit. Each is keyed by its row and safe to repeat: the next pass
/// reads the refs again and gives the effect again when it is not done.
#[derive(Default)]
struct Integrated {
    /// The repositories of the features: the git of part 3 reads them and
    /// writes their git folders.
    repos: Vec<PathBuf>,
    /// The rescue refs of the branches of cancelled tasks (FM-39).
    rescues: Vec<integration::landing::Effect>,
    /// The rescue refs of feature branches before a rebase (I10).
    rebase_rescues: Vec<integration::rebase::Effect>,
    /// The moves of feature refs by compare-and-set (I9).
    moves: Vec<integration::ref_move::Move>,
    /// The deletions of the candidate refs of ended bundles.
    candidates: Vec<integration::candidates::Deletion>,
    /// The deletions of branches that the guard of I12 lets go.
    branches: Vec<integration::branches::Deletion>,
    /// The deletions of `superseded` rescue refs that expired (FM-40).
    rescue_refs: Vec<rescue_refs::Deletion>,
    /// The lines of what failed.
    failed: Vec<String>,
}

impl Integrated {
    fn is_empty(&self) -> bool {
        self.rescues.is_empty()
            && self.rebase_rescues.is_empty()
            && self.moves.is_empty()
            && self.candidates.is_empty()
            && self.branches.is_empty()
            && self.rescue_refs.is_empty()
    }
}

impl Pass<'_> {
    /// Part 2 of integration, in a savepoint of `tx`: an error rolls back
    /// only the integration rows of this pass, and gives its line. The
    /// other rows of the pass commit.
    fn integrate(
        &self,
        tx: &Transaction<'_>,
        w: &TaskWork<'_>,
        clock: FleetSeconds,
        stamp: &str,
    ) -> Result<Integrated> {
        tx.execute_batch(&format!("SAVEPOINT {INTEGRATION}"))?;
        match self.integration_rows(tx, w, clock, stamp) {
            Ok(done) => {
                tx.execute_batch(&format!("RELEASE {INTEGRATION}"))?;
                Ok(done)
            }
            Err(e) => {
                tx.execute_batch(&format!("ROLLBACK TO {INTEGRATION}; RELEASE {INTEGRATION}"))?;
                Ok(Integrated {
                    failed: vec![format!("the integration rows: {e:#}")],
                    ..Integrated::default()
                })
            }
        }
    }

    /// The integration rows in the order of the design: `queue_due_jobs`
    /// (fetch and push), `land_bundles` (the task branches, the bundles,
    /// the ref moves and the candidate refs), `integrate_features` (the
    /// feature state machine, the rebases and their resolver tasks), and
    /// the checks of I11, I12 and the rescue classes from the last
    /// `worktree_list` run of each repository. I8 holds at each transition
    /// to `landed` or `merged`, I9 at each ref move, and I10 before each
    /// rebase: the use cases check them.
    fn integration_rows(
        &self,
        tx: &Transaction<'_>,
        w: &TaskWork<'_>,
        clock: FleetSeconds,
        stamp: &str,
    ) -> Result<Integrated> {
        use integration::{branches, bundles, candidates, features, landing, rebase, ref_move};
        let ctx = self.job_ctx();
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let repos = feature_repos(tx)?;
        let git = effects::integration_git(&self.settings.fleet_root, &repos);
        let gw = CliGitGateway::new(git.clone());
        let live = integration_rows::feature_ids(tx, true)?;
        let chosen = take(tx, RowKind::Features.as_str(), &live, budget, stamp)?;
        let keys: Vec<String> = integration_rows::bundles_to_follow(tx)?
            .into_iter()
            .map(id_key)
            .collect();
        let ids: Vec<i64> = take(tx, RowKind::Bundles.as_str(), &keys, budget, stamp)?
            .iter()
            .filter_map(|k| k.parse().ok())
            .collect();
        let mut out = Integrated {
            repos,
            ..Integrated::default()
        };
        for feature in &chosen {
            out.failed
                .extend(queue_remote(&ctx, tx, &gw, feature, clock)?);
        }
        let finishes = landing::RunFiles {
            evidence_root: &w.tools.evidence_root,
        };
        let l = landing::Landing {
            jobs: &ctx,
            finishes: &finishes,
        };
        out.rescues = landing::follow(&l, tx, clock, budget.rows())?;
        let b = bundles::Bundles {
            jobs: &ctx,
            git: &gw,
        };
        bundles::advance_in(&b, tx, clock, Some(&chosen), Some(&ids))?;
        let r = ref_move::RefMoves {
            jobs: &ctx,
            git: &gw,
        };
        out.moves = ref_move::advance_in(&r, tx, clock, Some(&ids))?;
        let c = candidates::Candidates {
            jobs: &ctx,
            git: &gw,
        };
        out.candidates = candidates::advance_in(&c, tx, Some(&ids))?;
        let f = features::Features {
            jobs: &ctx,
            git: &gw,
        };
        features::follow_in(&f, tx, clock, Some(&chosen))?;
        let refs = GitRebaseRefs::new(git.clone());
        let r = rebase::Rebases {
            jobs: &ctx,
            git: &gw,
            refs: &refs,
        };
        out.rebase_rescues = rebase::follow_in(&r, tx, clock, Some(&chosen))?;
        self.open_resolvers(tx, &gw, &CliStackGit::new(git), &chosen)?;
        let lists = branches::listings(tx)?;
        branches::adopt_orphans(&ctx, tx, &lists, clock)?;
        out.branches = branches::deletions(tx, &lists)?;
        self.check_i12(tx)?;
        let facts: Vec<RepoFacts> = lists
            .into_iter()
            .map(|l| RepoFacts {
                repo: l.repo,
                refs: l.rescue,
            })
            .collect();
        out.rescue_refs = rescue_refs::collect(&ctx, tx, &facts, chrono::Utc::now())?.deletions;
        Ok(out)
    }

    /// The resolver tasks of each rebase conflict of the features of
    /// `chosen`. Each takes the persona of
    /// [`integration::branches::resolver_persona`]; a feature with no task
    /// that has a persona opens none.
    fn open_resolvers(
        &self,
        tx: &Transaction<'_>,
        gw: &CliGitGateway,
        stack: &CliStackGit,
        chosen: &[String],
    ) -> Result<()> {
        use integration::branches::{self, Resolvers};
        let config = &self.settings.config;
        for (feature, branch, _) in integration_rows::live_feature_branches(tx)? {
            if !chosen.contains(&feature) {
                continue;
            }
            let Some(persona) = branches::resolver_persona(tx, &feature, &branch)? else {
                continue;
            };
            let r = Resolvers {
                git: gw,
                conflicts: stack,
                planner: StackPlanner {
                    max_lines: config.resolver_max_lines,
                    max_files: config.resolver_max_files,
                },
                profile: &persona,
                guard: Some(self.settings.generation),
            };
            let why = cause("a rebase of the feature branch conflicts");
            branches::open_rebase_resolvers_in(tx, &r, why, Some(std::slice::from_ref(&feature)))?;
        }
        Ok(())
    }

    /// I12: a task branch is a member of at most 1 bundle that is not
    /// ended. Each breach gets 1 needs-you item, and the item of a breach
    /// that ended is resolved.
    fn check_i12(&self, tx: &Transaction<'_>) -> Result<()> {
        let prefix = format!("{BREACH}:I12:{}:", Entity::TaskBranch.as_str());
        let mut open = BTreeSet::new();
        for (branch, ids) in integration_rows::branches_in_open_bundles(tx)? {
            let key = format!("{prefix}{branch}");
            let list: Vec<String> = ids.iter().map(i64::to_string).collect();
            let text = format!(
                "I12: the task branch {branch} is a member of {} bundles that are not ended: {}. \
                 fleetd does not drop a bundle for it. End the bundles that must not land.",
                ids.len(),
                list.join(", ")
            );
            let item = NewItem {
                key: &key,
                kind: BREACH,
                entity: Entity::TaskBranch,
                entity_id: &branch,
                text: &text,
            };
            self.ports.needs_you.open_item(tx, &item)?;
            open.insert(key);
        }
        for key in needs_you::open_keys(tx, BREACH)? {
            if key.starts_with(&prefix) && !open.contains(&key) {
                self.ports.needs_you.resolve_item(tx, &key)?;
            }
        }
        Ok(())
    }

    /// Part 3 of integration, after the other effects: write the rescue
    /// refs and record them in 1 transaction, then move the feature refs
    /// and delete the refs that are due. Each effect waits for the next
    /// pass when the time budget of the pass ends.
    fn integration_effects(
        &mut self,
        conn: &mut Connection,
        mut todo: Integrated,
        clock: FleetSeconds,
        deadline: Deadline,
        report: &mut PassReport,
    ) {
        use integration::{branches, candidates, landing, rebase, ref_move};
        report.failed.append(&mut todo.failed);
        if todo.is_empty() {
            return;
        }
        let git = effects::integration_git(&self.settings.fleet_root, &todo.repos);
        let gw = CliGitGateway::new(git.clone());
        let mut failed = Vec::new();
        let mut done = Vec::new();
        let mut landed = Vec::new();
        let rescues = landing::GitBranchRescues { git: &git };
        for e in &todo.rescues {
            if deadline.reached((self.now)()) {
                break;
            }
            match landing::run(e, &rescues) {
                Ok(d) => landed.push(d),
                Err(err) => failed.push(format!("the rescue ref of {e:?}: {err:#}")),
            }
        }
        let refs = GitRebaseRefs::new(git.clone());
        let mut rebased = Vec::new();
        for e in &todo.rebase_rescues {
            if deadline.reached((self.now)()) {
                break;
            }
            match rebase::run(e, &refs) {
                Ok(d) => rebased.push(d),
                Err(err) => failed.push(format!("the rescue ref of {e:?}: {err:#}")),
            }
        }
        if !landed.is_empty() || !rebased.is_empty() {
            match self.observe_rescues(conn, clock, &landed, &rebased) {
                Ok(()) => done.extend(landed.iter().map(|d| format!("{d:?}"))),
                Err(e) => failed.push(format!("the record of the rescue refs: {e:#}")),
            }
            done.extend(rebased.iter().map(|d| format!("rescue {}", d.rescue_ref)));
        }
        for mv in &todo.moves {
            if deadline.reached((self.now)()) {
                break;
            }
            match ref_move::run(conn, &gw, mv) {
                Ok(ran) => done.push(format!("move {} to {}: {ran:?}", mv.name, mv.to)),
                Err(e) => failed.push(format!("the move of {}: {e:#}", mv.name)),
            }
        }
        for d in &todo.candidates {
            if deadline.reached((self.now)()) {
                break;
            }
            match candidates::run(&gw, d) {
                Ok(ran) => done.push(format!("delete {}: {ran:?}", d.name)),
                Err(e) => failed.push(format!("the deletion of {}: {e:#}", d.name)),
            }
        }
        for d in &todo.branches {
            if deadline.reached((self.now)()) {
                break;
            }
            match branches::run(&gw, d) {
                Ok(ran) => done.push(format!("delete {}: {ran:?}", d.branch)),
                Err(e) => failed.push(format!("the deletion of {}: {e:#}", d.branch)),
            }
        }
        for d in &todo.rescue_refs {
            if deadline.reached((self.now)()) {
                break;
            }
            match rescue_refs::delete(&gw, d) {
                Ok(ran) => done.push(format!("delete {}: {ran:?}", d.name)),
                Err(e) => failed.push(format!("the deletion of {}: {e:#}", d.name)),
            }
        }
        report.done.extend(done.into_iter().map(Done::Git));
        report.failed.extend(failed);
    }

    /// Record the written rescue refs on their branch rows in 1
    /// transaction: each branch of a cancelled task moves to `abandoned`.
    fn observe_rescues(
        &self,
        conn: &mut Connection,
        clock: FleetSeconds,
        landed: &[integration::landing::Done],
        rebased: &[integration::rebase::Rescued],
    ) -> Result<()> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        fleetd_row::check_generation(&tx, self.settings.generation)?;
        let ctx = self.job_ctx();
        for d in landed {
            integration::landing::observe(&ctx, &tx, clock, d)?;
        }
        for d in rebased {
            integration::rebase::observe(&ctx, &tx, d)?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// The repository of each feature, each 1 time: the git of integration
/// reads them. A folder that is not a git repository is left out.
fn feature_repos(conn: &Connection) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    for id in integration_rows::feature_ids(conn, false)? {
        if let Some(run) = runs::feature_run(conn, &id)? {
            if !repos.contains(&run.project_root) {
                repos.push(run.project_root);
            }
        }
    }
    Ok(git_repositories(repos)
        .into_iter()
        .map(PathBuf::from)
        .collect())
}

/// `queue_due_jobs` of 1 feature: the fetch of its repository at each
/// period, and the push of its branch when it is due, each with the remote
/// of the project file at the feature tip. A feature with no branch tip
/// waits. Gives the line of a project file that does not parse.
fn queue_remote(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    gw: &CliGitGateway,
    feature: &str,
    clock: FleetSeconds,
) -> Result<Option<String>> {
    let Some(branch) = integration_rows::feature(tx, feature)?.and_then(|f| f.branch) else {
        return Ok(None);
    };
    let Some(run) = runs::feature_run(tx, feature)? else {
        return Ok(None);
    };
    let repo = PathBuf::from(&run.project_root);
    let Some(tip) = gw.tip(&repo, &format!("refs/heads/{branch}"))? else {
        return Ok(None);
    };
    let settings = match integration::features::project_settings(gw, &repo, &tip) {
        Ok(s) => s,
        Err(e) => return Ok(Some(format!("feature {feature}: {e:#}"))),
    };
    let root = &run.project_root;
    integration::remote::queue_fetch(ctx, tx, root, &settings.remote, clock)?;
    integration::remote::queue_push(ctx, tx, root, feature, &settings.remote, false, clock)?;
    Ok(None)
}
