use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
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
    id_key, Cursor, Deadline, IdeaCursor, PassBudget, RowKind, ScanKind,
};
use crate::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::health::Hold;
use crate::fleetd::fleet_runtime::domain::heartbeat::{Exe, Heartbeat};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::fleet_runtime::domain::pass::{
    breaches, observe_launch, stop_step, Breach, Holding, Launch, LaunchStep, Proc, StopStep,
    START_TIMEOUT_S, TIMED_OUT,
};
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::needs_you_folder::{self, Mirrored};
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::application::dispatcher::{Dispatch, Dispatcher, PrimaryTip};
use crate::fleetd::ideas::application::intake::{pending_idea_files, write_idea_file};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::application::plan_commit::{PlanCommit, PLAN_YAML};
use crate::fleetd::ideas::application::replan::{self, Replan};
use crate::fleetd::ideas::application::sub_orchestrators::{keep_sub_orchestrators, Keep};
use crate::fleetd::ideas::application::waiting::{keep_waits, Wait};
use crate::fleetd::ideas::application::{
    answers, intake, notes_ingest, question_flow, release, rollup,
};
use crate::fleetd::ideas::domain::needs_you::Write as NeedsYouWrite;
use crate::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use crate::fleetd::integration::adapters::exec_rebase::GitRebaseRefs;
use crate::fleetd::integration::adapters::exec_trial_merge::CliTrialMergeGit;
use crate::fleetd::integration::adapters::git::{CliGitGateway, CliStackGit};
use crate::fleetd::integration::adapters::sqlite as integration_rows;
use crate::fleetd::integration::application as integration;
use crate::fleetd::integration::application::judge_branch::{self, GitJudgeBranchRefs};
use crate::fleetd::integration::application::new_features::Project;
use crate::fleetd::integration::application::new_features::{self, BranchRef};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::application::trial_merges;
use crate::fleetd::integration::application::{merged_with_ideas, stack_edges, stacks};
use crate::fleetd::integration::domain::project::{
    missing_fast_gate_cause, ProjectSettings, MISSING_FAST_GATE_KIND,
};
use crate::fleetd::integration::domain::stack::StackPlanner;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::adapters::attempt_files::{
    self, Found, OsProcesses, WrapperRead,
};
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::dispatch::{self, BaseTips, DispatchCtx};
use crate::fleetd::task_execution::application::judge_pair::{self, Settled};
use crate::fleetd::task_execution::application::launch::{self as push, Launches};
use crate::fleetd::task_execution::application::outbox_ingest::{self, OutboxRead};
use crate::fleetd::task_execution::application::planned_tasks;
use crate::fleetd::task_execution::application::quota::{self, Pools};
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
    self, judged_task, promotable_deps, AttemptRow, AttemptState, Dep, DepKind, Facts, GroupLeft,
    InPane, ProcessScope, Reap, Recorded, RowRef, Rows, Seen, Stop, Target,
};
use crate::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use crate::fleetd::worker_profiles::application::conformance::read_conformance_json;
use crate::fleetd::worker_profiles::application::docs_jobs::{
    binary_id, exec_harness, queue_docs_jobs, scan_cli, CliBinary,
};
use crate::fleetd::worker_profiles::application::drift_idea::{add_drift_idea, KIND as DRIFT};
use crate::fleetd::worker_profiles::application::health::{
    mark_after_conformance, mark_cli_unhealthy, ConformanceRun,
};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::fleetd::worker_profiles::domain::conformance::ConformanceReport;
use crate::fleetd::worker_profiles::domain::profile::{Mode, Profile};
use crate::fleetd::worktree_custody::adapters::git::{
    worktree_list_key, CliWorktreeGit, ListResult,
};
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
use crate::taskledger::{runs, stop, tasks, timer};

pub const HANG_PASS: &str = "hang-pass";
pub const ABORT_AFTER_COMMIT: &str = "abort-after-commit";
pub const ABORT_BEFORE_COMMIT: &str = "abort-before-commit";
pub const DROP_WATCH_EVENTS: &str = "drop-watch-events";
pub const HOLD_LAUNCHING: &str = "hold-launching";

const LAUNCHES: &str = "launches";
const JOB_STARTS: &str = "job_starts";
const LIVE_ATTEMPTS: &str = "live_attempts";
const DISPATCH: &str = "dispatch";
const PUSH: &str = "push";
const BREACH: &str = "invariant_breach";
pub const TOKEN_REFUSED: &str = "operator_token_refused";
const FINISH_IDLE: &str = "finish_idle";

pub trait Outside {
    fn panes(&self) -> Option<BTreeMap<String, InPane>>;

    fn close_pane(&self, pane: &str) -> Result<()>;

    fn holds(&self) -> Vec<Hold>;

    fn worktree_folders(&self, conn: &Connection) -> Result<Vec<FolderItem>>;

    fn work_dirs(&self, _conn: &Connection) -> Result<Vec<WorkDir>> {
        Ok(Vec::new())
    }

    fn repositories(&self, _conn: &Connection) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec>;

    fn observe(&self, _w: &Worker) -> Option<Observation> {
        None
    }

    fn pools(&self) -> Option<PoolFacts> {
        None
    }

    fn doorbell(&self, pane: &str) -> Result<()> {
        anyhow::bail!("this fleetd sends no doorbell to pane {pane}")
    }

    fn cli_binaries(&self) -> Vec<CliPath> {
        Vec::new()
    }

    fn state_root(&self) -> Option<&Path> {
        None
    }
}

pub struct PoolFacts {
    pub roster: Roster,
    pub quota: QuotaView,
}

#[derive(Clone, Copy)]
pub struct DutyBudget<'a> {
    pub rows: PassBudget,
    pub time_up: &'a dyn Fn() -> bool,
}

pub struct DutyCtx<'a> {
    pub guard: Generation,
    pub clock: FleetSeconds,
    pub facts: &'a Facts,
    pub budget: DutyBudget<'a>,
}

pub struct ActCtx<'a> {
    pub fleet_root: &'a std::path::Path,
    pub clock: FleetSeconds,
}

pub trait RunDuties {
    fn observe(&self, _conn: &Connection, _budget: DutyBudget<'_>) -> Result<()> {
        Ok(())
    }

    fn run(&self, tx: &Transaction<'_>, ctx: &DutyCtx<'_>) -> Result<Vec<Stop>>;

    fn launches(&self, _a: &AttemptRow) -> bool {
        false
    }

    fn act(&self, _conn: &Connection, _ctx: &ActCtx<'_>) -> DutyReport {
        DutyReport::default()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DutyReport {
    pub done: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoDuties;

impl RunDuties for NoDuties {
    fn run(&self, _tx: &Transaction<'_>, _ctx: &DutyCtx<'_>) -> Result<Vec<Stop>> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Copy)]
pub struct TaskWork<'a> {
    pub tools: &'a Tools,
    pub sandboxes: &'a WorkerSandboxes,
    pub launches: &'a dyn Launches,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub fleet_root: PathBuf,
    pub generation: Generation,
    pub config: FleetConfig,
    pub horch: PathBuf,
    pub faults: Faults,
}

pub struct Ports<'a> {
    pub outside: &'a dyn Outside,
    pub duties: &'a dyn RunDuties,
    pub needs_you: &'a dyn NeedsYouItems,
    pub host: &'a dyn ProcessHost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Stop(String),
    StopJob(i64),
    Doorbell(String),
    Launch(String),
    StartJob(i64),
    Duty(String),
    Git(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PassReport {
    pub done: Vec<Done>,
    pub left: usize,
    pub failed: Vec<String>,
    pub heartbeat: bool,
}

#[derive(Debug)]
pub enum PassError {
    GenerationLost(DoorError),
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

enum Effect {
    Stop(Stop),
    StopJob(jobs::Stop),
    Doorbell(Bell),
    Launch(AttemptRow),
    PushLaunch(String),
    StartJob(JobRow),
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bell {
    attempt: String,
    event: i64,
    pane: String,
}

struct Read {
    stamp: String,
    cursors: Vec<(ScanKind, Cursor)>,
    task: Facts,
    jobs: Vec<ProcessFact>,
    launching: Vec<Launching>,
    may_stop: BTreeSet<(u32, u64)>,
    holds: Vec<Hold>,
    awake: AwakeReading,
    worktrees: WorktreeFacts,
    repos: Vec<String>,
    live: Vec<LiveSeen>,
    live_cursor: Cursor,
    activity: BTreeMap<String, Activity>,
    sent: BTreeMap<String, BTreeSet<i64>>,
    pools: Option<PoolFacts>,
    outboxes: Vec<OutboxRead>,
    wrappers: Vec<WrapperRead>,
    job_files: Vec<JobFile>,
    token_fault: Option<String>,
    tips: BTreeMap<String, String>,
    base_tips: BaseTips,
    failed: Vec<String>,
    p7: P7Read,
}

struct LiveSeen {
    attempt: AttemptRow,
    worker: Worker,
    process: Seen,
    heartbeat_age_s: Option<i64>,
    active: bool,
    pane: Option<rungs::Pane>,
}

struct Launching {
    row: AttemptRow,
    row_process: Proc,
    launch: Option<Launch>,
    launched: Option<Recorded>,
}

struct ProcessItems {
    items: Vec<ProcessItem>,
    jobs: BTreeSet<String>,
}

struct ToLaunch {
    attempts: Vec<AttemptRow>,
    push: Vec<String>,
}

#[derive(Default)]
struct WorkRows {
    stops: Vec<Stop>,
    launches: Vec<String>,
    passed: Vec<Passed>,
    failed: Vec<String>,
}

struct Decided {
    clock: FleetSeconds,
    effects: Vec<Effect>,
    breaches: BTreeSet<String>,
    may_stop: BTreeSet<(u32, u64)>,
    activity: BTreeMap<String, Activity>,
    sent: BTreeMap<String, BTreeSet<i64>>,
    passed: Vec<Passed>,
    roster: Option<Roster>,
    failed: Vec<String>,
    facts: Facts,
    integration: Integrated,
}

struct PaneStops {
    in_grace: BTreeSet<RowRef>,
    proofs: BTreeMap<(u32, u64), String>,
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
    fn hold(&mut self, row: &RowRef) {
        self.in_grace.insert(row.clone());
    }

    fn may_close(&self, row: &RowRef) -> bool {
        !self.in_grace.contains(row)
    }

    fn pane_of(&self, pid: u32, pid_start: u64) -> Option<String> {
        self.proofs.get(&(pid, pid_start)).cloned()
    }

    fn closed(&mut self, shell: Recorded) {
        self.closed
            .extend(shell.pid_start.map(|start| (shell.pid, start)));
    }

    fn pending(self) -> BTreeSet<(u32, u64)> {
        let closed = self.closed;
        self.proofs
            .into_keys()
            .filter(|k| !closed.contains(k))
            .collect()
    }
}

pub struct Pass<'a> {
    settings: Settings,
    ports: Ports<'a>,
    now: Box<dyn FnMut() -> Instant + 'a>,
    clock: ClockStore,
    jobs: JobRunner<'a>,
    launched: Vec<Child>,
    holds: Vec<Hold>,
    breaches: BTreeSet<String>,
    watch: Watch,
    exe: Exe,
    full_passes: u64,
    requests: Handlers,
    work: Option<TaskWork<'a>>,
    ideas: Option<IdeaWork<'a>>,
    serving: Cell<bool>,
    reproof: RefCell<Reproof>,
}

#[derive(Default)]
struct Reproof {
    read: BTreeMap<String, i64>,
    queued: BTreeSet<String>,
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
            ideas: None,
            serving: Cell::new(false),
            reproof: RefCell::new(Reproof::default()),
        }
    }

    pub fn with_task_work(mut self, work: TaskWork<'a>) -> Pass<'a> {
        self.work = Some(work);
        self
    }

    pub fn with_requests(mut self, handlers: Handlers) -> Pass<'a> {
        self.requests = handlers;
        self
    }

    pub fn with_clock(mut self, now: Box<dyn FnMut() -> Instant + 'a>) -> Pass<'a> {
        self.now = now;
        self
    }

    pub fn run(&mut self, conn: &mut Connection) -> Result<PassReport, PassError> {
        let deadline = Deadline::pass((self.now)(), self.settings.config.pass_time_budget_s);
        self.reap();
        let old_stops = self.import_old_stops(conn);
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
            failed.extend(write_task_files(
                conn,
                &self.settings.fleet_root,
                self.ports.outside.state_root(),
            ));
        }
        let activity = effects::write_activity(&self.settings.fleet_root, &decided.activity);
        let passed = std::mem::take(&mut decided.passed);
        let mut effects = std::mem::take(&mut decided.effects);
        failed.extend(self.idea_duties(conn, &decided, &mut effects, deadline)?);
        let sent = std::mem::take(&mut decided.sent);
        let (mut report, pending) = self.do_effects(conn, effects, sent, decided.clock, deadline);
        let git = std::mem::take(&mut decided.integration);
        self.integration_effects(conn, git, decided.clock, deadline, &mut report);
        if let Some(w) = self.work {
            self.verify_after_commit(conn, w, &decided, passed, deadline, &mut report);
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
        if let Err(e) = self.record_locks_left(conn) {
            report
                .failed
                .push(format!("the items of the git locks: {e:#}"));
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

    fn record_locks_left(&self, conn: &mut Connection) -> Result<()> {
        if !jobs::locks_journaled(conn)? {
            return Ok(());
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        jobs::record_locks_left(&self.job_ctx(), &tx)?;
        tx.commit()?;
        Ok(())
    }

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

    pub fn serve(&self, ledger: Connection, handlers: Handlers) -> Result<ChannelServer> {
        let server = ChannelServer::start(
            &self.settings.fleet_root,
            ledger,
            self.settings.generation,
            handlers,
        )?;
        self.serving.set(true);
        Ok(server)
    }

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

    fn import_old_stops(&self, conn: &mut Connection) -> Vec<String> {
        if self.full_passes > 0 {
            return Vec::new();
        }
        let root = &self.settings.fleet_root;
        polite_stops_store::import_old_files(conn, self.settings.generation, root)
            .unwrap_or_else(|e| vec![format!("the old records of polite stops: {e:#}")])
    }

    fn reap(&mut self) {
        self.jobs.reap();
        self.launched
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
    }

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
        let asked = attempts_store::asked_attempts(conn)?;
        let outboxes = outbox_items(&root, &rows, &asked);
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
                ScanKind::Requests => {}
            }
            cursors.push((kind, cursor));
        }
        let config = &self.settings.config;
        let owner = User::current().context("reading the user of fleetd")?;
        let (attempts, mut failed) = scanned_attempts(&root, &rows, &scanned);
        let limits = outbox_ingest::Limits::of(config);
        let outbox_reads = outbox_ingest::read(conn, &root, &attempts, &owner, &limits)?;
        let running = not_asked(&attempts, &asked);
        let wrappers =
            attempt_files::read(conn, &root, &running, &owner, config.heartbeat_max_bytes)?;
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
        let mut stopped =
            polite_stops_store::stopped(conn, |pid, start| !may_stop.contains(&(pid, start)))?;
        for s in &mut stopped {
            s.group = effects::group_left(s.process);
            if s.group == GroupLeft::Members {
                may_stop.extend(s.process.pid_start.map(|start| (s.process.pid, start)));
            }
        }
        if panes.is_none() {
            may_stop.extend(
                task::needs_snapshot(&rows, &stopped)
                    .into_iter()
                    .filter_map(|p| Some((p.pid, p.pid_start?))),
            );
        }
        let seen = task_processes(&rows, &liveness);
        let (live, live_cursor, activity) = self.read_live(conn, &rows, &seen, &stamp, deadline)?;
        let results: BTreeSet<&str> = outbox_reads
            .iter()
            .filter(|r| !r.messages.is_empty())
            .map(|r| r.attempt.task_id.as_str())
            .collect();
        let tips = match &self.work {
            Some(w) => task_tips(conn, &w.tools.git, &results)?,
            None => BTreeMap::new(),
        };
        let base_tips = match &self.work {
            Some(_) => {
                let roots: Vec<PathBuf> = repos.iter().map(PathBuf::from).collect();
                let gw = CliGitGateway::new(effects::integration_git(&root, &roots));
                dispatch::read_base_tips(conn, &gw).unwrap_or_else(|e| {
                    failed.push(format!("the base tips of the dispatch: {e:#}"));
                    BaseTips::new()
                })
            }
            None => BaseTips::new(),
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
            base_tips,
            failed,
            p7,
        })
    }

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

    fn health_holds(&self) -> Vec<Hold> {
        self.ports
            .outside
            .holds()
            .into_iter()
            .filter(|h| !matches!(h, Hold::WorktreeDisk { .. }))
            .collect()
    }

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
        let guard = Some(self.settings.generation);
        let at = util::now();
        let wrappers = attempt_files::ingest(&tx, guard, &read.wrappers, clock, &at, &OsProcesses)?;
        start_running(&tx, guard, clock, &wrappers, |persona| {
            let roster = read.pools.as_ref().map(|p| &p.roster);
            seconds(profile_of(roster, persona).liveness.lease_s)
        })?;
        let question_limit_s = self.settings.config.question_limit_s;
        outbox_ingest::ingest(&tx, guard, &read.outboxes, question_limit_s)?;
        let limit_failed = context_limits(&tx, guard, &self.settings, &read.p7)?;
        jobs::ingest_results(&self.job_ctx(), &tx, &read.job_files)?;
        token_item(&tx, &self.settings.fleet_root, read.token_fault.as_deref())?;
        let roster = read.pools.as_ref().map(|p| &p.roster);
        let launches = self.observe_intents(
            &tx,
            &read.launching,
            read.task.panes.as_ref(),
            roster,
            clock,
        )?;
        let mut stops = commands::mark_orphans(&tx, &read.task)?;
        let pools = read.pools.as_ref().map(|p| Pools {
            roster: &p.roster,
            quota: &p.quota,
            full_use: self.settings.config.pool_full_use,
        });
        let judge_git = effects::integration_git(&self.settings.fleet_root, &feature_repos(&tx)?);
        let judge_gw = CliGitGateway::new(judge_git.clone());
        let judge_refs = GitJudgeBranchRefs::new(
            judge_git,
            effects::judge_index_folder(&self.settings.fleet_root),
        );
        let rules = Rules {
            tx: &tx,
            judge_branches: JudgeGit {
                git: &judge_gw,
                refs: &judge_refs,
            },
            guard: Some(self.settings.generation),
            clock,
            pools: pools.as_ref(),
            config: &self.settings.config,
            needs_you: self.ports.needs_you,
            sent: &read.sent,
            exited: &wrappers.exited,
            personas_at_start: worker_personas(&tx)?,
        };
        let mut doorbells = Vec::new();
        rules.run(&read, &mut stops, &mut doorbells)?;
        reconcile_worktrees(&self.job_ctx(), &tx, &read.worktrees, clock)?;
        let disk = disk_budget(&self.job_ctx(), &tx, clock)?;
        let mut holds = read.holds;
        holds.extend(disk.hold.map(|cause| Hold::WorktreeDisk { cause }));
        let (paced, mut personas) = pacing(read.pools.as_ref());
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
        let ctx = self.job_ctx();
        let observed = jobs::observe(&ctx, &tx, &read.jobs, clock)?;
        let p7_failed = self.p7_rows(&tx, &read.p7, read.pools.as_ref(), clock)?;
        if let Some(p) = read.pools.as_ref() {
            for (profile, cause) in conformance_pending(&tx, &p.roster)? {
                personas.entry(profile).or_insert(cause);
            }
        }
        let mut work = WorkRows::default();
        if let Some(w) = &self.work {
            let repos = feature_repos(&tx)?;
            let gw =
                CliGitGateway::new(effects::integration_git(&self.settings.fleet_root, &repos));
            let gates = sync_gate_holds(&self.job_ctx(), &tx, &gw);
            let binds = DispatchCtx {
                jobs: self.job_ctx(),
                clock,
                git: &read.base_tips,
            };
            work = self.dispatch_rows(&tx, w, &binds, &holds, &personas, &read.stamp)?;
            if let Err(e) = gates {
                work.failed.push(format!("the gate holds: {e:#}"));
            }
            stops.append(&mut work.stops);
        }
        work.failed.extend(limit_failed);
        work.failed.extend(p7_failed);
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
            Some(_) => self.integrate(&tx, clock, &read.stamp)?,
            None => Integrated::default(),
        };
        jobs::record_locks_left(&ctx, &tx)?;
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
            facts: read.task,
            integration,
        })
    }

    fn dispatch_rows(
        &self,
        tx: &Transaction<'_>,
        w: &TaskWork<'_>,
        ctx: &DispatchCtx<'_>,
        holds: &[Hold],
        paced: &BTreeMap<String, String>,
        stamp: &str,
    ) -> Result<WorkRows> {
        let budget = PassBudget::new(self.settings.config.pass_budget).rows();
        let mut d = dispatch::dispatch_due(
            tx,
            ctx,
            &dispatch_holds(holds, paced, project_holds(tx)?, ctx.clock),
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
        repair::start_due(tx, ctx, budget)?;
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

    fn verify_after_commit(
        &mut self,
        conn: &mut Connection,
        w: TaskWork<'_>,
        decided: &Decided,
        passed: Vec<Passed>,
        deadline: Deadline,
        report: &mut PassReport,
    ) {
        if !passed.is_empty() && !deadline.reached((self.now)()) {
            match self.review_steps(conn, w, decided, passed) {
                Ok(judged) => self.judge_pair_effects(judged, report),
                Err(e) => report.failed.push(format!("the review steps: {e:#}")),
            }
        }
        if let Err(e) = verifier::write_verdicts(conn, &self.settings.fleet_root) {
            report.failed.push(format!("the verdict files: {e:#}"));
        }
    }

    fn judge_pair_effects(&self, judged: Vec<Settled>, report: &mut PassReport) {
        let root = &self.settings.fleet_root;
        let mut sent = None;
        for s in judged {
            if let Some(entry) = &s.decision {
                let written = User::current()
                    .context("reading the user of fleetd")
                    .and_then(|owner| entry.write(root, &owner));
                if let Err(e) = written {
                    report
                        .failed
                        .push(format!("the decision log of {}: {e:#}", entry.feature));
                }
            }
            if let Some(ring) = s.ring {
                let sent = sent.get_or_insert_with(|| effects::read_doorbells(root));
                let bell = Bell {
                    attempt: ring.attempt,
                    event: ring.event,
                    pane: ring.pane,
                };
                if let Some(d) = self.doorbell(bell, sent, report) {
                    report.done.push(d);
                }
            }
        }
    }

    fn review_steps(
        &self,
        conn: &Connection,
        w: TaskWork<'_>,
        decided: &Decided,
        passed: Vec<Passed>,
    ) -> Result<Vec<Settled>> {
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
        let judged = verifier::structural(&r, &mut store, &passed)?
            .into_iter()
            .filter_map(|(_, s)| match s {
                verifier::Structural::Judged(settled) => Some(settled),
                _ => None,
            })
            .collect();
        verifier::review(&r, &mut store, passed)?;
        Ok(judged)
    }

    fn job_ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: Some(self.settings.generation),
            config: &self.settings.config,
            needs_you: self.ports.needs_you,
        }
    }

    fn observe_intents(
        &self,
        tx: &Transaction<'_>,
        launching: &[Launching],
        panes: Option<&BTreeMap<String, InPane>>,
        roster: Option<&Roster>,
        clock: FleetSeconds,
    ) -> Result<Vec<AttemptRow>> {
        let mut launches = Vec::new();
        for l in launching {
            let a = &l.row;
            let state = door::state_of(tx, Entity::Attempt, &a.id)?;
            if state.as_deref() != Some(AttemptState::Launching.as_str()) {
                continue;
            }
            let mut step = observe_launch(l.row_process, l.launch, clock);
            if a.task_id.is_none() {
                let held = self.settings.faults.indexed(HOLD_LAUNCHING);
                let held = sub_orchestrator_of(a).is_some_and(|(i, _)| held.as_deref() == Some(i));
                match pane_wrapper(tx, a, panes)?.filter(|_| !held) {
                    Some(wrapper) => {
                        let guard = Some(self.settings.generation);
                        commands::attempt_live(tx, guard, &a.id, Some(wrapper))?;
                        continue;
                    }
                    None if step == LaunchStep::Live => step = LaunchStep::Wait,
                    None => {}
                }
            }
            match step {
                LaunchStep::Launch if self.ports.duties.launches(a) => {}
                LaunchStep::Launch if a.task_id.is_some() && via(tx, &a.id)? == PUSH => {}
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
                    if let Some(task) = &a.task_id {
                        self.retry_timed_out(tx, task, &a.worker_id, roster)?;
                    }
                }
            }
        }
        Ok(launches)
    }

    fn retry_timed_out(
        &self,
        tx: &Transaction<'_>,
        task: &str,
        worker: &str,
        roster: Option<&Roster>,
    ) -> Result<()> {
        let persona = workers::find(tx, worker)?.and_then(|w| w.persona);
        let policy = retry_policy(roster, &self.settings.config, persona.as_deref());
        let why = format!("no process started within {START_TIMEOUT_S} s of its launch");
        rung6_if_lost(tx, task, Outcome::TimedOut, &policy, &why)
    }

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
        for task in lost_tasks(tx)? {
            let key = format!("{BREACH}:I1:{}:{task}", Entity::Task);
            let text = format!(
                "I1: task {task} is leased or running, but each of its attempts ended and it \
                 has no lease. No pass rule moves it. Requeue or cancel the task."
            );
            let item = NewItem {
                key: &key,
                kind: BREACH,
                entity: Entity::Task,
                entity_id: &task,
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
                let done = self.close_pane(pane, *shell, stops, report)?;
                self.mark_pane(conn, &s.row, pane, report);
                Some(done)
            }
            Target::Pane { .. } => None,
        }
    }

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

    fn mark_pane(&self, conn: &mut Connection, row: &RowRef, pane: &str, report: &mut PassReport) {
        let marked = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)
            .and_then(|tx| {
                fleetd_row::check_generation(&tx, self.settings.generation)?;
                stops_row(&tx, row, stop::Sent::Pane(pane))?;
                tx.commit()?;
                Ok(())
            });
        if let Err(e) = marked {
            report
                .failed
                .push(format!("the stops row of the close of pane {pane}: {e:#}"));
        }
    }

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

struct Rules<'a> {
    tx: &'a Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    pools: Option<&'a Pools<'a>>,
    config: &'a FleetConfig,
    needs_you: &'a dyn NeedsYouItems,
    sent: &'a BTreeMap<String, BTreeSet<i64>>,
    exited: &'a [String],
    judge_branches: JudgeGit<'a>,
    personas_at_start: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Copy)]
struct JudgeGit<'a> {
    git: &'a dyn IntegrationGitGateway,
    refs: &'a dyn judge_branch::JudgeBranchRefs,
}

impl Rules<'_> {
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

    fn profile(&self, persona: Option<&str>) -> Profile {
        profile_of(self.pools.map(|p| p.roster), persona)
    }

    fn persona_of(&self, worker: &str) -> Result<Option<String>> {
        if let Some(persona) = self.personas_at_start.get(worker) {
            return Ok(persona.clone());
        }
        Ok(workers::find(self.tx, worker)?.and_then(|w| w.persona))
    }

    fn limits(&self, worker: &str) -> Limits {
        let persona = self.persona_of(worker).ok().flatten();
        let l = self.profile(persona.as_deref()).liveness;
        Limits {
            lease_s: seconds(l.lease_s),
            stall_after_s: seconds(l.stall_after_s),
            heartbeat_max_age_s: seconds(self.config.heartbeat_max_age_s),
        }
    }

    fn pool_limited(&self, persona: Option<&str>) -> bool {
        let (Some(p), Some(persona)) = (self.pools, persona) else {
            return false;
        };
        p.reading(persona)
            .is_some_and(|r| limited(&r, p.full_use, p.quota.now.timestamp()))
    }

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

    fn policy(&self, persona: Option<&str>) -> retry::Policy {
        retry_policy(self.pools.map(|p| p.roster), self.config, persona)
    }

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

    fn finish_exited(&self, facts: &Facts) -> Result<Vec<Stop>> {
        let mut rows = attempts_store::rows(self.tx, &util::host_name())?;
        keep_unproven(&mut rows, self.exited);
        exited_by_file(self.tx, self.guard, self.exited, &mut rows)?;
        let mut stops = Vec::new();
        for mut reap in task::dead_attempts(&rows, facts) {
            let why = format!("the process of attempt {} is gone", reap.attempt);
            let Some((task_id, _)) = reap.task.clone() else {
                stops.extend(commands::end_attempt(self.tx, &reap)?);
                if let Some(task_id) = task_of(&rows, &reap.attempt) {
                    let persona = self.persona_of(&reap.worker)?;
                    let policy = self.policy(persona.as_deref());
                    rung6_if_lost(self.tx, task_id, Outcome::NoResult, &policy, &why)?;
                }
                continue;
            };
            if verifying(self.tx, &task_id)? {
                stops.extend(commands::end_attempt(self.tx, &reap)?);
                continue;
            }
            if !reap.end_worker {
                stops.extend(commands::end_attempt(self.tx, &reap)?);
                commands::schedule_retry(self.tx, &task_id, &why)?;
                continue;
            }
            let persona = self.persona_of(&reap.worker)?;
            let mut outcome = self.exit_outcome(
                questions_store::open_of_task(self.tx, &task_id)? > 0,
                self.pool_limited(persona.as_deref()) || quota_hinted(self.tx, &reap.attempt)?,
            );
            if outcome == Outcome::NoResult && push::resume_refused(self.tx, &reap.attempt)? {
                outcome = Outcome::ResumeRefused;
            }
            reap.outcome = outcome.as_str().to_string();
            stops.extend(commands::end_attempt(self.tx, &reap)?);
            if outcome == Outcome::ResumeRefused {
                push::on_resume_refused(self.tx, self.guard, &task_id, &reap.attempt)?;
            } else if outcome == Outcome::Asked {
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
            let (judges, passed) = judge_deps(self.tx, &all, self.judge_branches)?;
            let deps: Vec<Dep> = deps
                .into_iter()
                .map(|(t, on)| Dep {
                    task: t,
                    on,
                    kind: DepKind::After,
                })
                .chain(judges)
                .collect();
            for id in promotable_deps(&statuses, &deps, &landed, &passed) {
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

    fn check_i1(&self) -> Result<()> {
        let limits = |worker: &str| self.limits(worker);
        let budget = PassBudget::new(self.config.pass_budget).rows();
        lease::check_i1_within(self.tx, self.guard, self.clock, &limits, budget)
    }
}

struct LadderOut<'a> {
    stops: &'a mut Vec<Stop>,
    doorbells: &'a mut Vec<Bell>,
}

impl LadderOut<'_> {
    fn take(&mut self, effects: Vec<ladder::Effect>) {
        for e in effects {
            if let ladder::Effect::Stop(s) = e {
                self.stops.push(s);
            }
        }
    }

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

fn seconds(s: u64) -> i64 {
    i64::try_from(s).unwrap_or(i64::MAX)
}

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
    let sent = stop::Sent::Signal {
        pid: process.0,
        pid_start: process.1,
        kill,
    };
    stops_row(tx, row, sent)
}

fn stops_row(tx: &Connection, row: &RowRef, sent: stop::Sent<'_>) -> Result<()> {
    let worker = match row {
        RowRef::Worker(id) => Some(id.clone()),
        RowRef::Attempt(id) => attempts_store::worker_of_attempt(tx, id)?,
    };
    match worker {
        Some(w) => stop::record_sent(tx, &w, sent),
        None => Ok(()),
    }
}

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

fn process_key(r: Recorded) -> String {
    match r.pid_start {
        Some(start) => format!("p:{}:{start}", r.pid),
        None => format!("p:{}:-", r.pid),
    }
}

fn job_cursor_key(id: i64) -> String {
    format!("{id:020}")
}

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

fn task_processes(rows: &Rows, liveness: &BTreeMap<String, Liveness>) -> BTreeMap<Recorded, Seen> {
    rows.named_processes()
        .into_iter()
        .map(|r| (r, to_seen(seen(liveness, Some(r)))))
        .collect()
}

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

fn may_stop(rows: &Rows, liveness: &BTreeMap<String, Liveness>) -> BTreeSet<(u32, u64)> {
    rows.named_processes()
        .into_iter()
        .filter(|r| seen(liveness, Some(*r)) != Proc::Dead)
        .filter_map(|r| Some((r.pid, r.pid_start?)))
        .collect()
}

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

fn pane_wrapper(
    tx: &Connection,
    a: &AttemptRow,
    panes: Option<&BTreeMap<String, InPane>>,
) -> Result<Option<Recorded>> {
    let pane = workers::find(tx, &a.worker_id)?
        .and_then(|w| w.pane_id)
        .or_else(|| a.pane.clone());
    let Some(InPane::Runs { pid, pid_start }) = pane.as_ref().and_then(|p| panes?.get(p)) else {
        return Ok(None);
    };
    let wrapper = Recorded {
        pid: *pid,
        pid_start: Some(*pid_start),
    };
    if attempts_store::process_of_other_attempt(tx, &a.id, wrapper)? {
        return Ok(None);
    }
    Ok(Some(wrapper))
}

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

fn not_asked(attempts: &[AttemptRef], asked: &BTreeSet<String>) -> Vec<AttemptRef> {
    attempts
        .iter()
        .filter(|a| !asked.contains(&a.attempt_id))
        .cloned()
        .collect()
}

fn outbox_items(root: &std::path::Path, rows: &Rows, asked: &BTreeSet<String>) -> Vec<FolderItem> {
    rows.attempts
        .iter()
        .filter(|a| a.state != AttemptState::Ended || asked.contains(&a.id))
        .filter_map(|a| {
            Some(FolderItem {
                key: a.id.clone(),
                path: attempt_ref(a)?.outbox(root),
            })
        })
        .collect()
}

fn attempt_ref(a: &AttemptRow) -> Option<AttemptRef> {
    Some(AttemptRef {
        task_id: a.task_id.clone()?,
        attempt_id: a.id.clone(),
    })
}

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

fn make_outbox(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    let _ = horch_platform::fs::create_dir_mode(&mut builder, OUTBOX_MODE);
    builder.create(dir)?;
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

fn profile_of(roster: Option<&Roster>, persona: Option<&str>) -> Profile {
    roster
        .zip(persona)
        .and_then(|(r, name)| r.get(name))
        .map(|t| t.profile())
        .unwrap_or_default()
}

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

fn dispatch_holds(
    holds: &[Hold],
    paced: &BTreeMap<String, String>,
    projects: BTreeMap<String, String>,
    clock: FleetSeconds,
) -> DispatchHolds {
    DispatchHolds {
        clock: clock.0,
        global: holds
            .iter()
            .filter_map(|h| h.global().map(|name| format!("{name}: {}", h.cause())))
            .collect(),
        personas: paced.clone(),
        projects,
        ..DispatchHolds::default()
    }
}

fn keep_unproven(rows: &mut task::Rows, exited: &[String]) {
    rows.attempts.retain(|a| {
        let unproven = a.state == AttemptState::Live
            && a.task_id.is_some()
            && a.process.is_none()
            && a.pane.is_none();
        !unproven || exited.contains(&a.id)
    });
}

fn exited_by_file(
    tx: &Connection,
    guard: Option<Generation>,
    exited: &[String],
    rows: &mut task::Rows,
) -> Result<()> {
    let mut moved = false;
    for a in rows.attempts.iter_mut().filter(|a| exited.contains(&a.id)) {
        if a.state == AttemptState::Launching {
            commands::attempt_live(tx, guard, &a.id, None)?;
            a.state = AttemptState::Live;
            moved = true;
        }
        if a.state == AttemptState::Live {
            a.process = None;
            a.pane = None;
        }
    }
    if moved {
        let fresh = attempts_store::rows(tx, &util::host_name())?;
        for (id, t) in fresh.tasks {
            rows.tasks.entry(id).or_insert(t);
        }
    }
    Ok(())
}

fn rung6_if_lost(
    tx: &Connection,
    task: &str,
    outcome: Outcome,
    policy: &retry::Policy,
    why: &str,
) -> Result<()> {
    let status = tasks::get(tx, task)?.status;
    let in_flight = [TaskStatus::Leased, TaskStatus::Running, TaskStatus::Stalled];
    if !in_flight.iter().any(|s| s.as_str() == status) {
        return Ok(());
    }
    let (_, open) = leases_store::i1_rows(tx)?;
    if !open.iter().any(|a| a.task_id == task) {
        retry::burn_attempt(tx, task, outcome, policy, why)?;
    }
    Ok(())
}

fn task_of<'a>(rows: &'a Rows, attempt: &str) -> Option<&'a str> {
    rows.attempts
        .iter()
        .find(|a| a.id == attempt)
        .and_then(|a| a.task_id.as_deref())
}

fn lost_tasks(tx: &Connection) -> Result<Vec<String>> {
    let (held, open) = leases_store::i1_rows(tx)?;
    let in_flight = [TaskStatus::Leased.as_str(), TaskStatus::Running.as_str()];
    let mut lost = Vec::new();
    for t in held {
        if !in_flight.contains(&t.status.as_str())
            || t.lease_attempt.is_some()
            || open.iter().any(|a| a.task_id == t.task_id)
            || tasks::get(tx, &t.task_id)?.attempts == 0
            || tasks::cancel_requested(tx, &t.task_id)?.is_some()
        {
            continue;
        }
        lost.push(t.task_id);
    }
    Ok(lost)
}

fn verifying(tx: &Connection, task: &str) -> Result<bool> {
    Ok(tasks::get(tx, task)?.status == TaskStatus::Verifying.as_str())
}

fn quota_hinted(tx: &Connection, attempt: &str) -> Result<bool> {
    let hint = attempt_files::QUOTA_HINT;
    door::has_fact(tx, Entity::Attempt, attempt, EventKind::Note, hint)
}

fn worker_personas(tx: &Connection) -> Result<BTreeMap<String, Option<String>>> {
    attempts_store::worker_personas(tx)
}

fn project_holds(conn: &Connection) -> Result<BTreeMap<String, String>> {
    Ok(needs_you::open_items(conn)?
        .into_iter()
        .filter(|i| i.kind == MISSING_FAST_GATE_KIND)
        .map(|i| {
            let cause = missing_fast_gate_cause(&i.entity_id);
            (i.entity_id, cause)
        })
        .collect())
}

fn start_running(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    wrappers: &attempt_files::Report,
    lease_s: impl Fn(Option<&str>) -> i64,
) -> Result<()> {
    for id in &wrappers.checkins {
        let Some((Some(task_id), state, via, persona)) = attempts_store::attempt_via(tx, id)?
        else {
            continue;
        };
        if via != PUSH {
            continue;
        }
        let wrapper = wrappers.wrappers.get(id).copied();
        if state == AttemptState::Launching.as_str() {
            commands::attempt_live(tx, guard, id, wrapper)?;
        } else if state != AttemptState::Live.as_str() {
            continue;
        } else if let Some(w) = wrapper {
            commands::attempt_wrapper(tx, id, w)?;
        }
        if wrapper.is_some() {
            commands::push_worker_working(tx, guard, id)?;
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
            Err(DoorError::Refused { .. }) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

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

fn via(conn: &Connection, id: &str) -> Result<String> {
    let row = attempts_store::attempt_via(conn, id)?;
    row.map(|(_, _, via, _)| via)
        .with_context(|| format!("the ledger has no attempt {id}"))
}

fn write_task_files(conn: &Connection, root: &Path, state_root: Option<&Path>) -> Vec<String> {
    match dispatch::write_missing_task_files_in(conn, root, state_root, &[]) {
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

fn startable(
    tx: &Transaction<'_>,
    due: &[JobRow],
    ids: &[String],
    budget: PassBudget,
    stamp: &str,
) -> Result<Vec<String>> {
    let cursor = cursors_store::load(tx, JOB_STARTS, stamp)?;
    let mut oldest: BTreeMap<&str, i64> = BTreeMap::new();
    for job in due {
        if let (true, Some(repo)) = (jobs::is_git_writer(job.kind), job.repo.as_deref()) {
            let id = oldest.entry(repo).or_insert(job.id);
            *id = (*id).min(job.id);
        }
    }
    let mut out = Vec::new();
    for (job, id) in cursor.turns(&due.iter().zip(ids).collect::<Vec<_>>(), |(_, id)| id) {
        if out.len() == budget.rows() {
            break;
        }
        if let (true, Some(repo)) = (jobs::is_git_writer(job.kind), job.repo.as_deref()) {
            if oldest.get(repo) != Some(&job.id) || jobs::git_writer_busy(tx, repo)? {
                continue;
            }
        }
        if let Some(repo) = job.repo.as_deref() {
            let removal_due = job.kind == Kind::WorktreeList
                && due
                    .iter()
                    .any(|d| d.kind == Kind::WorktreeRemove && d.repo == job.repo);
            if removal_due || jobs::excluded(tx, job.kind, repo)? {
                continue;
            }
        }
        out.push((*id).clone());
    }
    Ok(out)
}

fn git_repositories(repos: Vec<String>) -> Vec<String> {
    repos
        .into_iter()
        .filter(|r| Path::new(r).join(".git").exists())
        .collect()
}

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

pub const ABORT_AFTER_IDEA_COMMIT: &str = "abort-after-idea-commit";

pub const IDEAS_DIR: &str = "ideas";

const NEEDS_YOU_DIR: &str = "needs-you";

const ASKING_PHASE: &str = "clarifying";

const AT_PROMPT: [&str; 2] = ["idle", "done"];

const ROW_KEY_WIDTH: usize = 20;

pub trait IdeaPorts {
    fn roster(&self) -> Vec<RosterProfile>;

    fn gates(&self, conn: &Connection) -> Result<DeclaredGates>;

    fn git(&self) -> &dyn IntegrationGitGateway;

    fn primaries(&self, conn: &Connection) -> Result<Vec<Primary>>;

    fn now(&self) -> DateTime<FixedOffset>;

    fn notify(&self, _mirrored: &Mirrored, _seq: u64) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primary {
    pub repo: PathBuf,
    pub branch: String,
    pub tip: String,
}

struct IdeaWork<'a> {
    ports: &'a dyn IdeaPorts,
    dispatcher: Dispatcher,
    release_due: bool,
    recover: bool,
    last_clock: Option<FleetSeconds>,
    unwritten: Vec<answers::Recorded>,
}

#[derive(Default)]
struct Ingested {
    released: Vec<release::ReleasedApproval>,
    recorded: Vec<answers::Recorded>,
}

#[derive(Default)]
struct IdeaKept {
    effects: Vec<Effect>,
    reminded: usize,
}

impl<'a> Pass<'a> {
    pub fn with_idea_work(mut self, ports: &'a dyn IdeaPorts) -> Pass<'a> {
        self.ideas = Some(IdeaWork {
            ports,
            dispatcher: Dispatcher::new(),
            release_due: true,
            recover: true,
            last_clock: None,
            unwritten: Vec::new(),
        });
        self
    }

    fn idea_duties(
        &mut self,
        conn: &mut Connection,
        decided: &Decided,
        effects: &mut Vec<Effect>,
        deadline: Deadline,
    ) -> Result<Vec<String>, PassError> {
        let Some(mut work) = self.ideas.take() else {
            return Ok(Vec::new());
        };
        let mut failed = Vec::new();
        let out = self.idea_steps(conn, &mut work, decided, deadline, &mut failed);
        if out.is_ok() {
            work.recover = !failed.is_empty();
        }
        self.ideas = Some(work);
        effects.extend(out?);
        effects.sort_by_key(Effect::order);
        Ok(failed)
    }

    fn idea_steps(
        &mut self,
        conn: &mut Connection,
        work: &mut IdeaWork<'_>,
        decided: &Decided,
        deadline: Deadline,
        failed: &mut Vec<String>,
    ) -> Result<Vec<Effect>, PassError> {
        let root = self.settings.fleet_root.clone();
        let ideas = root.join(IDEAS_DIR);
        let owner = User::current().context("reading the user of fleetd")?;
        let stamp = util::now();
        let clock = decided.clock;
        let elapsed_s = work.last_clock.map_or(0, |c| (clock.0 - c.0).max(0));
        work.last_clock = Some(clock);
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let guard = self.settings.generation;
        let git = work.ports.git();
        retry_answers(work, &root, &owner, failed);
        let requests = request_effects(conn, &stamp, budget)?;
        let (effects_cursor, unwritten) =
            write_request_effects(&ideas, &owner, git, requests, failed);
        self.recover_idea_files(conn, work, &ideas, &owner, failed);
        let (folders_cursor, notes) = self.idea_notes(conn, &ideas, &owner, &stamp, deadline)?;
        let asking = self.asking(conn)?;
        let release_due = work.release_due;
        let ingested = idea_tx(conn, guard, failed, "the notes and the questions", |tx| {
            cursors_store::save(tx, IdeaCursor::IdeaFolders.as_str(), &folders_cursor)?;
            cursors_store::save(tx, IdeaCursor::RequestEffects.as_str(), &effects_cursor)?;
            for u in &unwritten {
                unwritten_item(tx, u)?;
            }
            let mut out = Ingested::default();
            if release_due {
                let ctx = release::Release {
                    guard: Some(guard),
                    ideas: &ideas,
                    owner: owner.clone(),
                    budget,
                };
                out.released = release::release(tx, &ctx)?.approvals;
            }
            let report = notes_ingest::ingest(tx, Some(guard), &ideas, &owner, &notes)?;
            let ctx = answers::Answering {
                guard: Some(guard),
                clock,
                root: &root,
                owner: &owner,
            };
            for note in &report.answers {
                if let answers::Answered::Recorded(r) = answers::answer_question(tx, &ctx, note)? {
                    out.recorded.push(r);
                }
            }
            answers::settle_revisions(tx)?;
            let flow = question_flow::Flow {
                guard: Some(guard),
                clock,
                elapsed_s,
                stamp: &stamp,
                budget,
            };
            question_flow::run(tx, &flow)?;
            Ok(out)
        })?;
        if let Some(i) = ingested {
            work.release_due = release_due && i.released.len() >= budget.rows();
            for a in &i.released {
                if let Err(e) = a.write(&ideas, owner.clone()) {
                    failed.push(format!("the approval.json of idea {}: {e:#}", a.idea));
                }
            }
            work.unwritten.extend(i.recorded);
            retry_answers(work, &root, &owner, failed);
        }
        if deadline.reached((self.now)()) {
            return Ok(Vec::new());
        }
        let replans =
            self.plan_and_replan(conn, work, &ideas, &owner, clock, &stamp, deadline, failed)?;
        if deadline.reached((self.now)()) {
            return Ok(Vec::new());
        }
        let job = self.job_ctx();
        let host = util::host_name();
        let kept = idea_tx(
            conn,
            guard,
            failed,
            "the roll-up, the sub-orchestrators and the waits",
            |tx| {
                if let Some(cursor) = &replans {
                    cursors_store::save(tx, IdeaCursor::Replans.as_str(), cursor)?;
                }
                rollup::roll_up_ideas(&job, tx)?;
                let keep = Keep {
                    guard: Some(guard),
                    ideas: &ideas,
                    facts: &decided.facts,
                    stamp: &stamp,
                    budget,
                };
                let subs = keep_sub_orchestrators(tx, &keep)?;
                let wait = Wait {
                    guard: Some(guard),
                    config: &self.settings.config,
                    clock,
                    now: work.ports.now(),
                    asking: &asking,
                    facts: &decided.facts,
                    ideas: &ideas,
                    owner: owner.clone(),
                    budget,
                };
                let waits = keep_waits(tx, &wait)?;
                let mut out = IdeaKept {
                    reminded: waits.reminded.len(),
                    ..IdeaKept::default()
                };
                out.effects
                    .extend(subs.stops.into_iter().chain(waits.stops).map(Effect::Stop));
                out.effects.extend(subs.started.iter().map(|s| {
                    Effect::Launch(AttemptRow {
                        id: s.attempt.clone(),
                        task_id: None,
                        worker_id: s.worker_id.clone(),
                        state: AttemptState::Launching,
                        process: None,
                        pane: s.pane.clone(),
                    })
                }));
                let bells = unsent_bells(tx, &host, &decided.sent)?;
                out.effects.extend(bells.into_iter().map(Effect::Doorbell));
                Ok(out)
            },
        )?;
        self.settings.faults.abort_if(ABORT_AFTER_IDEA_COMMIT);
        let kept = kept.unwrap_or_default();
        self.mirror_needs_you(conn, work, clock, &stamp, kept.reminded, failed)?;
        Ok(kept.effects)
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_and_replan(
        &mut self,
        conn: &Connection,
        work: &mut IdeaWork<'_>,
        ideas: &Path,
        owner: &User,
        clock: FleetSeconds,
        stamp: &str,
        deadline: Deadline,
        failed: &mut Vec<String>,
    ) -> Result<Option<Cursor>, PassError> {
        let path = conn
            .path()
            .filter(|p| !p.is_empty())
            .context("the ledger has no file")?;
        let mut store = Store::open(Path::new(path))?;
        let roster = work.ports.roster();
        let gates = work.ports.gates(store.conn()).unwrap_or_else(|e| {
            failed.push(format!("the gates of the project: {e:#}"));
            DeclaredGates::default()
        });
        let git = work.ports.git();
        let plan = || PlanCommit {
            ideas,
            owner,
            roster: &roster,
            gates: &gates,
            guard: Some(self.settings.generation),
            refusals: &NeedsYouList,
        };
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let primaries = match work.ports.primaries(store.conn()) {
            Ok(p) => p,
            Err(e) => {
                failed.push(format!("the primary tips: {e:#}"));
                Vec::new()
            }
        };
        let tips: Vec<PrimaryTip<'_>> = primaries
            .iter()
            .map(|p| PrimaryTip {
                repo: &p.repo,
                branch: &p.branch,
                tip: &p.tip,
            })
            .collect();
        let dispatch = Dispatch {
            plan: plan(),
            job: self.job_ctx(),
            git,
            primaries: &tips,
            clock,
            stamp,
            budget,
        };
        match work.dispatcher.pass(&mut store, &dispatch) {
            Ok(report) => {
                for (idea, e) in report.errors {
                    failed.push(format!("the dispatch of idea {idea}: {e}"));
                }
                for r in &report.refs {
                    if let Err(e) = new_features::create_ref(git, r) {
                        failed.push(format!("the feature branch {}: {e:#}", r.name));
                    }
                }
            }
            Err(e) => failed.push(format!("the dispatcher: {e:#}")),
        }
        if deadline.reached((self.now)()) {
            return Ok(None);
        }
        let mut keys = BTreeSet::new();
        for t in tasks::failed(store.conn())? {
            if planned_tasks::committed_task(store.conn(), &t.task_id)?.is_some() {
                keys.insert(t.task_id);
            }
        }
        keys.extend(answers::pending_replans(store.conn())?);
        let keys: Vec<String> = keys.into_iter().collect();
        let mut cursor = cursors_store::load(store.conn(), IdeaCursor::Replans.as_str(), stamp)?;
        let turns: Vec<String> = cursor
            .turns(&keys, |k| k.as_str())
            .into_iter()
            .take(budget.rows())
            .cloned()
            .collect();
        for id in turns {
            if deadline.reached((self.now)()) {
                break;
            }
            let ctx = Replan {
                plan: plan(),
                jobs: self.job_ctx(),
            };
            if let Err(e) = replan::replan_task(&mut store, &ctx, &id, clock) {
                failed.push(format!("the replan of task {id}: {e:#}"));
            }
            cursor.step(&id, stamp);
        }
        Ok(Some(cursor))
    }

    fn recover_idea_files(
        &self,
        conn: &Connection,
        work: &IdeaWork<'_>,
        ideas: &Path,
        owner: &User,
        failed: &mut Vec<String>,
    ) {
        let mut step = |what: &str, r: Result<()>| {
            if let Err(e) = r {
                failed.push(format!("{what}: {e:#}"));
            }
        };
        step(
            "the idea.md files",
            intake::pending_idea_files(conn, ideas).and_then(|files| {
                for f in &files {
                    intake::write_idea_file(ideas, owner.clone(), f)?;
                }
                Ok(())
            }),
        );
        if !work.recover {
            return;
        }
        step(
            "the approval.json files of the release",
            release::pending_approvals(conn, ideas)
                .and_then(|files| files.iter().try_for_each(|a| a.write(ideas, owner.clone()))),
        );
        let git = work.ports.git();
        step(
            "the feature branch refs",
            new_features::pending_refs(conn, git).and_then(|refs| {
                for r in &refs {
                    new_features::create_ref(git, r)?;
                }
                Ok(())
            }),
        );
    }

    fn idea_notes(
        &mut self,
        conn: &Connection,
        ideas: &Path,
        owner: &User,
        stamp: &str,
        deadline: Deadline,
    ) -> Result<(Cursor, Vec<notes_ingest::IdeaNotes>)> {
        let mut cursor = cursors_store::load(conn, IdeaCursor::IdeaFolders.as_str(), stamp)?;
        let start = (self.now)();
        let mut clock = ScanClock {
            deadline: deadline.scan(start, self.settings.config.scan_budget_ms),
            now: &mut *self.now,
            stamp,
        };
        let scan = scans::scan_idea_folders(ideas, &mut cursor, &mut clock, |idea| {
            notes_ingest::read(conn, ideas, std::slice::from_ref(idea), owner)
        })
        .with_context(|| format!("listing {}", ideas.display()))?;
        let mut notes = Vec::new();
        for read in scan.facts {
            notes.extend(read?);
        }
        Ok((cursor, notes))
    }

    fn asking(&self, conn: &Connection) -> Result<BTreeSet<String>> {
        let rows = attempts_store::rows(conn, &util::host_name())?;
        let mut asking = BTreeSet::new();
        for a in rows
            .attempts
            .iter()
            .filter(|a| a.state == AttemptState::Live)
        {
            let Some((idea, ASKING_PHASE)) = sub_orchestrator_of(a) else {
                continue;
            };
            let w = workers::get(conn, &a.worker_id)?;
            let status = self.ports.outside.observe(&w).and_then(|o| o.herdr_status);
            if status.is_some_and(|s| AT_PROMPT.contains(&s.as_str())) {
                asking.insert(idea.to_string());
            }
        }
        Ok(asking)
    }

    fn mirror_needs_you(
        &mut self,
        conn: &mut Connection,
        work: &IdeaWork<'_>,
        clock: FleetSeconds,
        stamp: &str,
        reminded: usize,
        failed: &mut Vec<String>,
    ) -> Result<(), PassError> {
        let open = needs_you::open_items(conn)?;
        let mut cursor = cursors_store::load(conn, IdeaCursor::NeedsYou.as_str(), stamp)?;
        let dir = self.settings.fleet_root.join(NEEDS_YOU_DIR);
        let budget = PassBudget::new(self.settings.config.pass_budget);
        let mirrored = match needs_you_folder::mirror(&dir, &open, &mut cursor, budget, stamp) {
            Ok(m) => m,
            Err(e) => {
                failed.push(format!("the needs-you files: {e:#}"));
                return Ok(());
            }
        };
        idea_tx(
            conn,
            self.settings.generation,
            failed,
            "the needs-you cursor",
            |tx| cursors_store::save(tx, IdeaCursor::NeedsYou.as_str(), &cursor),
        )?;
        let seq = u64::try_from(clock.0).unwrap_or(0) * 2;
        if let Err(e) = work.ports.notify(&mirrored, seq) {
            failed.push(format!("{e:#}"));
        }
        if reminded > 0 {
            let reminders = Mirrored {
                new: reminded,
                open: open.len(),
                ..Mirrored::default()
            };
            if let Err(e) = work.ports.notify(&reminders, seq + 1) {
                failed.push(format!("{e:#}"));
            }
        }
        Ok(())
    }
}

fn idea_tx<T>(
    conn: &mut Connection,
    guard: Generation,
    failed: &mut Vec<String>,
    what: &str,
    f: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<Option<T>, PassError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    match fleetd_row::check_generation(&tx, guard) {
        Ok(()) => {}
        Err(e @ DoorError::GenerationLost { .. }) => return Err(PassError::GenerationLost(e)),
        Err(e) => return Err(PassError::Failed(e.into())),
    }
    let out = f(&tx).and_then(|v| Ok(tx.commit().map(|()| v)?));
    match out {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            failed.push(format!("{what}: {e:#}"));
            Ok(None)
        }
    }
}

fn sub_orchestrator_of(a: &AttemptRow) -> Option<(&str, &str)> {
    if a.task_id.is_some() {
        return None;
    }
    let mut parts = a.id.split('/');
    let (idea, phase, n) = (parts.next()?, parts.next()?, parts.next()?);
    let ok = parts.next().is_none() && idea.starts_with("I-") && n.parse::<i64>().is_ok();
    ok.then_some((idea, phase))
}

fn unsent_bells(
    tx: &Connection,
    host: &str,
    sent: &BTreeMap<String, BTreeSet<i64>>,
) -> Result<Vec<Bell>> {
    let rows = attempts_store::rows(tx, host)?;
    let mut bells = Vec::new();
    for a in &rows.attempts {
        let (AttemptState::Live, Some(pane), Some(_)) = (a.state, &a.pane, sub_orchestrator_of(a))
        else {
            continue;
        };
        let typed = sent.get(&a.id);
        for (event, _) in doorbells_of(tx, &a.id)? {
            if !typed.is_some_and(|s| s.contains(&event)) {
                bells.push(Bell {
                    attempt: a.id.clone(),
                    event,
                    pane: pane.clone(),
                });
            }
        }
    }
    Ok(bells)
}

fn retry_answers(work: &mut IdeaWork<'_>, root: &Path, owner: &User, failed: &mut Vec<String>) {
    work.unwritten.retain(|r| match r.write(root, owner) {
        Ok(()) => false,
        Err(e) => {
            failed.push(format!("the files of the answer of {}: {e:#}", r.question));
            true
        }
    });
}

struct RequestRow {
    key: String,
    pending: bool,
    effects: Option<serde_json::Value>,
}

fn request_effects(
    conn: &Connection,
    stamp: &str,
    budget: PassBudget,
) -> Result<(Cursor, Vec<RequestRow>)> {
    let cursor = cursors_store::load(conn, IdeaCursor::RequestEffects.as_str(), stamp)?;
    let after: i64 = cursor
        .after_key
        .as_deref()
        .map_or(Ok(0), str::parse)
        .context("the cursor of the request effects is not a row number")?;
    let limit = i64::try_from(budget.rows()).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(
        "SELECT rowid, state, result_json FROM requests WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![after, limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (n, state, result) in rows {
        let effects = result
            .and_then(|r| serde_json::from_str::<serde_json::Value>(&r).ok())
            .and_then(|mut v| v.get_mut("effects").map(serde_json::Value::take))
            .filter(serde_json::Value::is_object);
        out.push(RequestRow {
            key: format!("{n:0ROW_KEY_WIDTH$}"),
            pending: state == "pending",
            effects,
        });
    }
    Ok((cursor, out))
}

struct Unwritten {
    key: String,
    idea: String,
    reason: String,
}

fn write_request_effects(
    ideas: &Path,
    owner: &User,
    git: &dyn IntegrationGitGateway,
    (mut cursor, requests): (Cursor, Vec<RequestRow>),
    failed: &mut Vec<String>,
) -> (Cursor, Vec<Unwritten>) {
    let stamp = cursor.lap_started_at.clone();
    let mut unwritten = Vec::new();
    for r in requests {
        if r.pending {
            break;
        }
        if let Some(effects) = &r.effects {
            if let Err(e) = write_idea_effects(ideas, owner, git, effects) {
                failed.push(format!("the effects of request row {}: {e:#}", r.key));
                unwritten.push(Unwritten {
                    key: r.key.clone(),
                    idea: effects["idea"].as_str().unwrap_or("-").to_string(),
                    reason: format!("{e:#}"),
                });
            }
        }
        cursor.step(&r.key, &stamp);
    }
    (cursor, unwritten)
}

pub const EFFECTS_UNWRITTEN: &str = "request_effects_unwritten";

fn unwritten_item(tx: &Connection, u: &Unwritten) -> Result<()> {
    let key = format!("{EFFECTS_UNWRITTEN}:{}", u.key.trim_start_matches('0'));
    let text = format!(
        "fleetd cannot write the files of a request of idea {}: {}. fleetd does not try again, \
         and the later requests go on. Correct the cause, then send the request again.",
        u.idea, u.reason
    );
    needs_you::open_item(
        tx,
        &NeedsYouWrite {
            key: &key,
            kind: EFFECTS_UNWRITTEN,
            entity: Entity::Idea.as_str(),
            entity_id: &u.idea,
            text: &text,
        },
    )
}

fn write_idea_effects(
    ideas: &Path,
    owner: &User,
    git: &dyn IntegrationGitGateway,
    effects: &serde_json::Value,
) -> Result<()> {
    let text = |v: &serde_json::Value, key: &str| -> Result<String> {
        v[key]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("the effect has no {key}"))
    };
    let idea = text(effects, "idea")?;
    let folder = IdeaFolder::create(ideas, &idea, owner.clone())?;
    let dir = folder.dir().to_path_buf();
    if let Some(t) = effects.get("ticket") {
        let rel = text(t, "path")?;
        let path = dir.join(&rel);
        anyhow::ensure!(
            Path::new(&rel)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "the ticket path {rel} leaves the idea folder"
        );
        if let Some(parent) = path.parent() {
            crate::fsx::ensure_private_dir(parent)?;
        }
        crate::fsx::create_immutable(&path, text(t, "text")?.as_bytes(), IDEA_FILE_MODE)?;
    }
    if let Some(plan) = effects.get("plan_yaml").and_then(serde_json::Value::as_str) {
        crate::fsx::write_atomic(&dir.join(PLAN_YAML), plan.as_bytes(), IDEA_FILE_MODE)?;
        crate::fleetd::ideas::application::direct::write_judge_ticket(&dir)?;
    }
    if let Some(a) = effects.get("approval_json") {
        folder.write_approval(&text(a, "text")?)?;
    }
    if let Some(a) = effects.get("amendment") {
        let n = a["n"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .context("the amendment has no number")?;
        folder.write_amendment(n, &text(a, "text")?)?;
    }
    if let Some(b) = effects.get("branch") {
        let r = BranchRef {
            repo: PathBuf::from(text(b, "repo")?),
            name: text(b, "name")?,
            tip: text(b, "tip")?,
        };
        new_features::create_ref(git, &r)?;
    }
    Ok(())
}

const IDEA_FILE_MODE: u32 = 0o600;

const CLI_BINARIES: &str = "cli_binaries";

const CONFORMANCE_ACTED: &str = "conformance result acted on";

struct P7Read {
    horch_id: Option<String>,
    clis: Vec<CliBinary>,
    cli_cursor: Cursor,
    limits: Vec<(AttemptRef, retry::TreeAtEnd)>,
}

impl Pass<'_> {
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

    fn p7_rows(
        &self,
        tx: &Transaction<'_>,
        p7: &P7Read,
        pools: Option<&PoolFacts>,
        clock: FleetSeconds,
    ) -> Result<Vec<String>> {
        cursors_store::save(tx, CLI_BINARIES, &p7.cli_cursor)?;
        let mut failed = Vec::new();
        if let Some(p) = pools {
            failed.extend(self.conformance_results(tx, &p.roster)?);
        }
        if let Some(horch_id) = &p7.horch_id {
            queue_docs_jobs(&self.job_ctx(), tx, clock, horch_id, &p7.clis)?;
            self.reprove(tx, clock, &p7.clis)?;
        }
        Ok(failed)
    }

    fn reprove(&self, tx: &Transaction<'_>, clock: FleetSeconds, clis: &[CliBinary]) -> Result<()> {
        if !self.serving.get() {
            return Ok(());
        }
        let mut reproof = self.reproof.borrow_mut();
        for cli in clis {
            if reproof.queued.contains(&cli.cli) {
                continue;
            }
            let Some(row) = jobs_store::last_of_word(tx, Kind::Conformance, &cli.cli)? else {
                continue;
            };
            let ended = matches!(row.state, State::Passed | State::Failed);
            if !ended || reproof.read.get(&cli.cli) == Some(&row.id) {
                continue;
            }
            reproof.read.insert(cli.cli.clone(), row.id);
            let not_proved = conformance_report(&row)
                .ok()
                .flatten()
                .is_some_and(|r| r.has_not_proved());
            if !not_proved {
                continue;
            }
            let key = JobKey {
                kind: Kind::Conformance,
                subject: &row.subject,
                input_key: &row.input_key,
            };
            jobs::queue(&self.job_ctx(), tx, &key, None, clock)?;
            reproof.queued.insert(cli.cli.clone());
        }
        Ok(())
    }

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
            match row_alone(tx, || self.act_on_conformance(tx, guard, id, &profiles))? {
                Ok(lines) => failed.extend(lines),
                Err(e) => failed.push(format!("conformance run {id}: {e}")),
            }
        }
        Ok(failed)
    }

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
        let no_report = match conformance_report(&row) {
            Ok(Some(report)) => {
                let run = ConformanceRun {
                    id,
                    subject: &row.subject,
                };
                mark_after_conformance(tx, guard, &run, &report, profiles)?;
                if report.drift().is_some() {
                    note = self.drift(tx, &report, &mut failed)?;
                }
                None
            }
            Ok(None) => Some(format!(
                "conformance run {id} of {} produced no report",
                row.subject
            )),
            Err(e) => Some(format!(
                "conformance run {id} of {} produced an unreadable or invalid report: {e:#}",
                row.subject
            )),
        };
        if let Some(reason) = no_report {
            let cli = row.subject.split_whitespace().next().unwrap_or_default();
            mark_cli_unhealthy(tx, guard, cli, &reason, profiles)?;
            note = serde_json::json!({ "unhealthy": reason });
            failed.push(reason);
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

fn at_context_limit(w: &WrapperRead) -> bool {
    w.exit.as_ref().is_some_and(|f| {
        matches!(&f.found, Found::Valid { file, .. }
            if file.outcome_hint == Some(OutcomeHint::ContextLimit))
    })
}

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

fn conformance_pending(tx: &Connection, roster: &Roster) -> Result<BTreeMap<String, String>> {
    let sql = "SELECT DISTINCT substr(subject, 1, instr(subject, ' ') - 1) FROM jobs
         WHERE kind = ?1 AND instr(subject, ' ') > 0";
    let mut stmt = tx.prepare(sql)?;
    let clis = stmt
        .query_map([Kind::Conformance.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = BTreeMap::new();
    for cli in clis {
        let Some(last) = jobs_store::last_of_word(tx, Kind::Conformance, &cli)? else {
            continue;
        };
        if conformance_acted(tx, last.id)? || !earlier_binary(tx, &cli, &last)? {
            continue;
        }
        let cause = format!("conformance of {} is pending", last.subject);
        for name in roster.names() {
            let agent = roster.get(name).map(|t| t.agent);
            if agent.is_some() && exec_harness(&cli) == agent {
                out.insert(name.to_string(), cause.clone());
            }
        }
    }
    Ok(out)
}

fn conformance_acted(tx: &Connection, id: i64) -> Result<bool> {
    let sql = "SELECT EXISTS (SELECT 1 FROM events WHERE entity = ?1
         AND entity_id = CAST(?2 AS TEXT) AND kind = ?3 AND cause = ?4)";
    let params = rusqlite::params![
        Entity::Job.as_str(),
        id,
        EventKind::Note.as_str(),
        CONFORMANCE_ACTED
    ];
    Ok(tx.query_row(sql, params, |r| r.get(0))?)
}

fn earlier_binary(tx: &Connection, cli: &str, last: &JobRow) -> Result<bool> {
    let sql = "SELECT EXISTS (SELECT 1 FROM jobs WHERE kind = ?1 AND id < ?2
         AND substr(subject, 1, length(?3) + 1) = ?3 || ' ' AND subject != ?4)";
    let params = rusqlite::params![Kind::Conformance.as_str(), last.id, cli, last.subject];
    Ok(tx.query_row(sql, params, |r| r.get(0))?)
}

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

const INTEGRATION: &str = "integration";

#[derive(Default)]
struct Integrated {
    repos: Vec<PathBuf>,
    rescues: Vec<integration::landing::Effect>,
    rebase_rescues: Vec<integration::rebase::Effect>,
    moves: Vec<integration::ref_move::Move>,
    candidates: Vec<integration::candidates::Deletion>,
    branches: Vec<integration::branches::Deletion>,
    rescue_refs: Vec<rescue_refs::Deletion>,
    moved_ideas: bool,
    judges: Vec<judge_branch::Effect>,
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
            && self.judges.is_empty()
    }
}

impl Pass<'_> {
    fn integrate(
        &self,
        tx: &Transaction<'_>,
        clock: FleetSeconds,
        stamp: &str,
    ) -> Result<Integrated> {
        tx.execute_batch(&format!("SAVEPOINT {INTEGRATION}"))?;
        match self.integration_rows(tx, clock, stamp) {
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

    fn integration_rows(
        &self,
        tx: &Transaction<'_>,
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
        let mut follow = integration_rows::bundles_to_follow(tx)?;
        follow.extend(bundles::late_builds(tx)?);
        follow.sort_unstable();
        follow.dedup();
        let keys: Vec<String> = follow.into_iter().map(id_key).collect();
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
        let m = merged_with_ideas::MergedWithIdeas {
            jobs: &ctx,
            git: &gw,
        };
        let moved = duty(tx, "merged_with_ideas", &mut out.failed, |tx| {
            merged_with_ideas::move_ideas(&m, tx, clock)
        })?;
        out.moved_ideas = moved.is_some_and(|m| !m.is_empty());
        let f = features::Features {
            jobs: &ctx,
            git: &gw,
        };
        features::follow_in(&f, tx, clock, Some(&chosen))?;
        duty(tx, "trial_merges", &mut out.failed, |tx| {
            queue_trial_merges(&ctx, tx, &gw, &git, &chosen, budget, clock)
        })?;
        duty(tx, "stacks", &mut out.failed, |tx| {
            self.stack_conflicts(tx, &git)
        })?;
        let refs = GitRebaseRefs::new(git.clone());
        let rebases = rebase::Rebases {
            jobs: &ctx,
            git: &gw,
            refs: &refs,
        };
        let edges = duty(tx, "stack_edges", &mut out.failed, |tx| {
            self.stack_edges(tx, &rebases, clock)
        })?;
        out.rebase_rescues.extend(edges.into_iter().flatten());
        let l = landing::Landing { jobs: &ctx };
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
        out.rebase_rescues
            .extend(rebase::follow_in(&rebases, tx, clock, Some(&chosen))?);
        self.open_resolvers(tx, &gw, &CliStackGit::new(git.clone()), &chosen)?;
        let judge_refs =
            GitJudgeBranchRefs::new(git, effects::judge_index_folder(&self.settings.fleet_root));
        let j = judge_branch::JudgeBranches {
            jobs: &ctx,
            git: &gw,
            refs: &judge_refs,
        };
        let judges = duty(tx, "judge_branches", &mut out.failed, |tx| {
            judge_branch::follow(&j, tx)
        })?;
        out.judges = judges.unwrap_or_default();
        let lists = branches::listings(tx)?;
        branches::adopt_orphans(&ctx, tx, &lists, clock)?;
        out.branches = branches::deletions(tx, &lists)?;
        let facts: Vec<RepoFacts> = lists
            .into_iter()
            .map(|l| RepoFacts {
                repo: l.repo,
                refs: l.rescue,
            })
            .collect();
        out.rescue_refs = rescue_refs::collect(&ctx, tx, &facts, crate::clock::now())?.deletions;
        Ok(out)
    }

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

    fn stack_conflicts(&self, tx: &Transaction<'_>, git: &CliWorktreeGit) -> Result<()> {
        let config = &self.settings.config;
        let planner = StackPlanner {
            max_lines: config.resolver_max_lines,
            max_files: config.resolver_max_files,
        };
        let stack_git = CliStackGit::new(git.clone());
        let ideas = self.settings.fleet_root.join(IDEAS_DIR);
        let why = cause("a trial merge of 2 features conflicts");
        for (row, upper) in stacks::newest_conflicts(tx, &planner)? {
            let Some(run) = runs::feature_run(tx, &upper)? else {
                continue;
            };
            let Some(profile) = feature_persona(tx, &upper)? else {
                continue;
            };
            let s = stacks::Stacking {
                repo: Path::new(&run.project_root),
                ideas: &ideas,
                profile: &profile,
                planner,
                guard: Some(self.settings.generation),
            };
            stacks::stack_conflict(tx, &s, &stack_git, &row, why)
                .with_context(|| format!("the stack of {upper}"))?;
        }
        Ok(())
    }

    fn stack_edges(
        &self,
        tx: &Transaction<'_>,
        rebases: &integration::rebase::Rebases<'_>,
        clock: FleetSeconds,
    ) -> Result<Vec<integration::rebase::Effect>> {
        let ideas = self.settings.fleet_root.join(IDEAS_DIR);
        let mut effects = Vec::new();
        for upper in stack_edges::ended_uppers(tx)? {
            let Some(profile) = feature_persona(tx, &upper)? else {
                continue;
            };
            let e = stack_edges::Edges {
                rebases,
                ideas: &ideas,
                profile: &profile,
            };
            effects.extend(stack_edges::follow_in(&e, tx, clock, Some(&upper))?);
        }
        Ok(effects)
    }

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
        self.moved_ideas_effects(conn, todo.moved_ideas, clock, deadline, report);
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
        self.judge_branch_effects(conn, &todo.judges, git, deadline, &mut done, &mut failed);
        report.done.extend(done.into_iter().map(Done::Git));
        report.failed.extend(failed);
    }

    fn judge_branch_effects(
        &mut self,
        conn: &mut Connection,
        judges: &[judge_branch::Effect],
        git: CliWorktreeGit,
        deadline: Deadline,
        done: &mut Vec<String>,
        failed: &mut Vec<String>,
    ) {
        let gw = CliGitGateway::new(git.clone());
        let refs =
            GitJudgeBranchRefs::new(git, effects::judge_index_folder(&self.settings.fleet_root));
        let mut judged = Vec::new();
        for e in judges {
            if deadline.reached((self.now)()) {
                break;
            }
            match judge_branch::run(e, &gw, &refs) {
                Ok(d) => judged.push(d),
                Err(err) => failed.push(format!("the judge branch effect {e:?}: {err:#}")),
            }
        }
        if judged.is_empty() {
            return;
        }
        match self.observe_judges(conn, &judged) {
            Ok(()) => done.extend(judged.iter().map(|d| format!("{d:?}"))),
            Err(e) => failed.push(format!("the record of the judge branches: {e:#}")),
        }
    }

    fn moved_ideas_effects(
        &mut self,
        conn: &mut Connection,
        moved: bool,
        clock: FleetSeconds,
        deadline: Deadline,
        report: &mut PassReport,
    ) {
        let pending = match merged_with_ideas::pending_moves(conn) {
            Ok(p) => p,
            Err(e) => {
                report
                    .failed
                    .push(format!("the task branches of merged features: {e:#}"));
                return;
            }
        };
        if (!moved && pending.is_empty()) || deadline.reached((self.now)()) {
            return;
        }
        match self.complete_moved_ideas(conn, clock) {
            Ok(done) => report.done.extend(done.into_iter().map(Done::Git)),
            Err(e) => report.failed.push(format!(
                "the effects of a merge with ideas in flight: {e:#}"
            )),
        }
    }

    fn complete_moved_ideas(&self, conn: &Connection, clock: FleetSeconds) -> Result<Vec<String>> {
        let repos = feature_repos(conn)?;
        let git = effects::integration_git(&self.settings.fleet_root, &repos);
        let gw = CliGitGateway::new(git.clone());
        let mut done = Vec::new();
        for r in new_features::pending_refs(conn, &gw)? {
            new_features::create_ref(&gw, &r)?;
            done.push(format!("create {}", r.name));
        }
        let path = conn
            .path()
            .filter(|p| !p.is_empty())
            .context("the ledger has no file")?;
        let mut store = Store::open(Path::new(path))?;
        let rescues = integration::landing::GitBranchRescues { git: &git };
        let ctx = self.job_ctx();
        let moved = merged_with_ideas::complete_moves(&ctx, &mut store, &gw, &rescues, clock)?;
        done.extend(
            moved
                .into_iter()
                .map(|b| format!("move {b} to its new feature")),
        );
        Ok(done)
    }

    fn observe_judges(&self, conn: &mut Connection, done: &[judge_branch::Done]) -> Result<()> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        fleetd_row::check_generation(&tx, self.settings.generation)?;
        let ctx = self.job_ctx();
        for d in done {
            judge_branch::observe(&ctx, &tx, d)?;
        }
        tx.commit()?;
        Ok(())
    }

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

fn duty<T>(
    tx: &Transaction<'_>,
    name: &str,
    failed: &mut Vec<String>,
    f: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<Option<T>> {
    tx.execute_batch(&format!("SAVEPOINT {name}"))?;
    match f(tx) {
        Ok(v) => {
            tx.execute_batch(&format!("RELEASE {name}"))?;
            Ok(Some(v))
        }
        Err(e) => {
            tx.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))?;
            failed.push(format!("the duty {name}: {e:#}"));
            Ok(None)
        }
    }
}

fn queue_trial_merges(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    gw: &CliGitGateway,
    git: &CliWorktreeGit,
    chosen: &[String],
    budget: PassBudget,
    clock: FleetSeconds,
) -> Result<()> {
    trial_merges::record_ended(ctx, tx)?;
    let reads = CliTrialMergeGit::new(git.clone());
    let mut done = BTreeSet::new();
    let mut left = budget.rows();
    for id in chosen {
        let Some(row) = integration_rows::feature(tx, id)? else {
            continue;
        };
        let Some(run) = runs::feature_run(tx, id)? else {
            continue;
        };
        let Ok(tips) = integration::features::read_tips(gw, tx, &run.project_root, &row)? else {
            continue;
        };
        if !done.insert((tips.repo.clone(), row.primary_branch.clone())) {
            continue;
        }
        let primary = trial_merges::Primary {
            repo: &tips.repo,
            branch: &row.primary_branch,
            tip: &tips.primary,
        };
        let queued = trial_merges::queue_due(ctx, tx, &reads, &primary, left, clock)?;
        left = left.saturating_sub(queued.len());
    }
    Ok(())
}

fn feature_persona(tx: &Connection, id: &str) -> Result<Option<String>> {
    let branch = integration_rows::feature(tx, id)?.and_then(|f| f.branch);
    match branch {
        Some(b) => integration::branches::resolver_persona(tx, id, &b),
        None => Ok(None),
    }
}

fn judge_deps(
    tx: &Connection,
    all: &[tasks::Task],
    branches: JudgeGit<'_>,
) -> Result<(Vec<Dep>, BTreeSet<String>)> {
    let mut deps = Vec::new();
    let mut passed = BTreeSet::new();
    for t in all
        .iter()
        .filter(|t| t.status == TaskStatus::Pending.as_str())
    {
        let Some(judged) = judged_task(&t.kind, t.parent_task_id.as_deref()) else {
            continue;
        };
        if verifier::structural_passed(tx, judged)? && judge_branch_ready(tx, t, judged, branches)?
        {
            passed.insert(judged.to_string());
        }
        deps.push(Dep {
            task: t.task_id.clone(),
            on: judged.to_string(),
            kind: DepKind::Judges,
        });
    }
    Ok((deps, passed))
}

fn judge_branch_ready(
    tx: &Connection,
    judge: &tasks::Task,
    judged: &str,
    branches: JudgeGit<'_>,
) -> Result<bool> {
    if !judge_pair::in_fix_round(tx, judged)? {
        return Ok(true);
    }
    judge_branch::holds_judged_tip(branches.git, branches.refs, tx, judge)
}

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

fn feature_settings(
    tx: &Connection,
    gw: &CliGitGateway,
    feature: &str,
) -> Result<Option<(runs::Run, std::result::Result<ProjectSettings, String>)>> {
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
    let settings = integration::features::project_settings(gw, &repo, &tip)?
        .map_err(|e| format!("feature {feature}: {e}"));
    Ok(Some((run, settings)))
}

fn sync_gate_holds(ctx: &JobCtx<'_>, tx: &Connection, gw: &CliGitGateway) -> Result<()> {
    let mut by_project: BTreeMap<String, ProjectSettings> = BTreeMap::new();
    for feature in integration_rows::feature_ids(tx, true)? {
        let Some((run, settings)) = feature_settings(tx, gw, &feature)? else {
            continue;
        };
        let Ok(s) = settings else {
            continue;
        };
        let held = by_project
            .get(&run.project_id)
            .is_some_and(|h| h.fast_gate().is_none());
        if !held {
            by_project.insert(run.project_id, s);
        }
    }
    for (project, settings) in &by_project {
        holds::sync_missing_fast_gate(tx, ctx.needs_you, project, settings)?;
    }
    Ok(())
}

fn queue_remote(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    gw: &CliGitGateway,
    feature: &str,
    clock: FleetSeconds,
) -> Result<Option<String>> {
    let Some((run, settings)) = feature_settings(tx, gw, feature)? else {
        return Ok(None);
    };
    let settings = match settings {
        Ok(s) => s,
        Err(e) => return Ok(Some(e)),
    };
    let root = &run.project_root;
    integration::remote::queue_fetch(ctx, tx, root, &settings.remote, clock)?;
    integration::remote::queue_push(ctx, tx, root, feature, &settings.remote, false, clock)?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::schema;

    const REPO: &str = "/repo";

    fn ledger() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn
    }

    fn queue(conn: &Connection, kind: Kind, subject: &str) -> i64 {
        let new = jobs_store::NewRun {
            kind,
            subject,
            repo: Some(REPO),
            input_key: "k",
            run: 1,
            abandoned_runs: 0,
            not_before: None,
            time_limit_s: 60,
        };
        jobs_store::insert(conn, &new).unwrap()
    }

    fn starts(conn: &mut Connection) -> Vec<i64> {
        let tx = conn.transaction().unwrap();
        let due = jobs::due_to_start(&tx, FleetSeconds(0)).unwrap();
        let ids: Vec<String> = due.iter().map(|j| job_cursor_key(j.id)).collect();
        let budget = PassBudget::new(10);
        let ok = startable(&tx, &due, &ids, budget, "t").unwrap();
        let started: Vec<i64> = take(&tx, JOB_STARTS, &ok, budget, "t")
            .unwrap()
            .iter()
            .map(|k| due[ids.iter().position(|i| i == k).unwrap()].id)
            .collect();
        for id in &started {
            tx.execute("UPDATE jobs SET state = 'running' WHERE id = ?1", [id])
                .unwrap();
        }
        tx.commit().unwrap();
        started
    }

    fn end(conn: &Connection, id: i64) {
        conn.execute("UPDATE jobs SET state = 'passed' WHERE id = ?1", [id])
            .unwrap();
    }

    #[test]
    fn rescue_queued_while_a_fetch_runs_starts_before_a_newer_fetch() {
        let mut conn = ledger();
        let first = queue(&conn, Kind::Fetch, "fetch-1");
        assert_eq!(starts(&mut conn), [first]);
        let rescue = queue(&conn, Kind::Rescue, "tree-1");
        let list = queue(&conn, Kind::WorktreeList, "list-1");
        assert_eq!(starts(&mut conn), [list], "the rescue waits for the fetch");
        end(&conn, first);
        let newer = queue(&conn, Kind::Fetch, "fetch-2");
        assert_eq!(starts(&mut conn), [rescue]);
        end(&conn, rescue);
        assert_eq!(starts(&mut conn), [newer]);
    }

    #[test]
    fn worktree_list_and_worktree_remove_of_1_repository_do_not_run_together() {
        let mut conn = ledger();
        let list = queue(&conn, Kind::WorktreeList, "list-1");
        assert_eq!(starts(&mut conn), [list]);
        let remove = queue(&conn, Kind::WorktreeRemove, "tree-1");
        assert_eq!(starts(&mut conn), Vec::<i64>::new(), "the removal waits");
        let newer = queue(&conn, Kind::WorktreeList, "list-2");
        end(&conn, list);
        assert_eq!(starts(&mut conn), [remove], "the newer list waits");
        assert_eq!(starts(&mut conn), Vec::<i64>::new(), "the list waits");
        end(&conn, remove);
        assert_eq!(starts(&mut conn), [newer]);
    }

    #[test]
    fn fetch_is_not_starved_by_a_row_of_writers() {
        let mut conn = ledger();
        let first = queue(&conn, Kind::WorktreeAdd, "tree-0");
        assert_eq!(starts(&mut conn), [first]);
        let fetch = queue(&conn, Kind::Fetch, "fetch-1");
        let mut newer = Vec::new();
        for i in 1..=3 {
            let list = queue(&conn, Kind::WorktreeList, &format!("list-{i}"));
            assert_eq!(starts(&mut conn), [list], "the fetch waits for the writer");
            end(&conn, list);
            newer.push(queue(&conn, Kind::WorktreeAdd, &format!("tree-{i}")));
        }
        end(&conn, first);
        assert_eq!(starts(&mut conn), [fetch]);
        let mut running = fetch;
        for writer in newer {
            end(&conn, running);
            assert_eq!(starts(&mut conn), [writer]);
            running = writer;
        }
    }
}
