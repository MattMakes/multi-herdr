//! The run of 1 reconcile pass.
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass" (RR-4, RR-7, FM-7,
//! FM-11, FM-12, PL-5). [`Pass::run`] runs 1 pass in 3 parts:
//!
//! 1. It reads the facts with no transaction, each scan within its budget
//!    from its cursor: the processes that rows name, the outboxes, the
//!    worktree folders and the folders of the work directories, the
//!    requests, the herdr panes, the health holds and the launch record of
//!    each attempt in `launching`. It reads the last passed `worktree_list`
//!    run of each repository from `jobs`. It runs no git.
//! 2. In 1 transaction it calls these steps, in this order:
//!    `check_generation`, `tick_fleet_clock`, `observe_intents`,
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
//!    next pass. When time is left, the run duties do their effects. Then
//!    it writes the heartbeat and pings the watchdog.
//!
//! A crash before the commit leaves no effect. A crash after it leaves
//! intended rows, and the next pass does each effect once: a launch is
//! keyed by its attempt id, a stop by the pid and start time of its
//! process, and a job start by the id of its row.
//!
//! 1 git writer for each repository (MAP.md of fleetd-p3, Q12): part 2
//! takes a due `worktree_add`, `worktree_remove` or `rescue` job only when
//! no such job of its repository runs or is already an effect of the pass.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::clock_store::{self, ClockStore};
use crate::fleetd::fleet_runtime::adapters::effects::{self, Activity, LaunchRecord, Watch};
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::scans::{
    self, FolderFact, FolderItem, Liveness, Presence, ProcessFact, ProcessItem, ScanClock,
};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::adapters::{cursors_store, fleetd_row};
use crate::fleetd::fleet_runtime::application::holds::record_hold_changes;
use crate::fleetd::fleet_runtime::application::jobs::{
    self, JobCtx, JobKey, JobRunner, NeedsYouItems, NewItem,
};
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline, PassBudget, ScanKind};
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
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::quota::{
    self, Pools, Restart, Restarted, Restarts,
};
use crate::fleetd::task_execution::application::stall as ladder;
use crate::fleetd::task_execution::application::{cancel, commands, lease, retry};
use crate::fleetd::task_execution::domain::lease::Limits;
use crate::fleetd::task_execution::domain::quota::limited;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::stall::{self as rungs, Decision, PaneStatus};
use crate::fleetd::task_execution::domain::task::{
    self, promotable, AttemptRow, AttemptState, Facts, InPane, ProcessScope, Reap, Recorded,
    RowRef, Rows, Seen, Stop, Target,
};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::fleetd::worker_profiles::domain::profile::{Mode, Profile};
use crate::fleetd::worktree_custody::adapters::git::{worktree_list_key, ListResult};
use crate::fleetd::worktree_custody::application::disk::disk_budget;
use crate::fleetd::worktree_custody::application::reconcile::{
    check_i6, reconcile_worktrees, Facts as WorktreeFacts, Listed, WorkDir,
};
use crate::fleetd::worktree_custody::domain::worktree::Breach as I6;
use crate::roster::Roster;
use crate::routing::quota::QuotaView;
use crate::runtime::fault::Faults;
use crate::taskledger::model::{Actor, EventKind, ProgressSource, TaskStatus};
use crate::taskledger::util;
use crate::taskledger::watchdog::{self, Observation};
use crate::taskledger::workers::{self, Worker};
use crate::taskledger::{tasks, timer};

/// After the first full pass, the next pass sleeps for ever before its
/// heartbeat (`p0_fleetd_hang.rs`).
pub const HANG_PASS: &str = "hang-pass";
/// The pass ends the process after the commit of part 2, before part 3.
pub const ABORT_AFTER_COMMIT: &str = "abort-after-commit";
/// The file watching reports nothing.
pub const DROP_WATCH_EVENTS: &str = "drop-watch-events";

/// The cursor of the launches of part 3 in `pass_cursors`.
const LAUNCHES: &str = "launches";
/// The cursor of the job starts of part 3 in `pass_cursors`.
const JOB_STARTS: &str = "job_starts";
/// The cursor of the live attempts that part 1 observes, in `pass_cursors`.
const LIVE_ATTEMPTS: &str = "live_attempts";
/// The kind of the needs-you item of a breach that no step repairs.
const BREACH: &str = "invariant_breach";
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
            Effect::Launch(_) => Rank::Launch,
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

/// The decisions of part 2.
struct Decided {
    clock: FleetSeconds,
    effects: Vec<Effect>,
    breaches: BTreeSet<String>,
    may_stop: BTreeSet<(u32, u64)>,
    activity: BTreeMap<String, Activity>,
    sent: BTreeMap<String, BTreeSet<i64>>,
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
        }
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
        let read = self.read_facts(conn, deadline)?;
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
        let activity = effects::write_activity(&self.settings.fleet_root, &decided.activity);
        let (mut report, pending) =
            self.do_effects(conn, decided.effects, decided.sent, decided.clock, deadline);
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
        match effects::forget_polite(&self.settings.fleet_root, kept) {
            Ok(unreadable) => report.failed.extend(unreadable.iter().map(|path| {
                format!(
                    "the record of a polite stop {} does not parse: it proves no pane, so \
                     fleetd kept the pane of each row that names its process, and removed \
                     the record",
                    path.display()
                )
            })),
            Err(e) => report
                .failed
                .push(format!("the records of polite stops: {e}")),
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

    /// Reap each ended process that this fleetd started, so that its pid
    /// is gone, not a zombie, for the process scan.
    fn reap(&mut self) {
        self.jobs.reap();
        self.launched
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
    }

    // ─── part 1 ─────────────────────────────────────────────────────────

    fn read_facts(&mut self, conn: &Connection, deadline: Deadline) -> Result<Read> {
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
        for kind in ScanKind::ALL {
            let mut cursor = cursors_store::load(conn, kind.as_str(), &stamp)?;
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
                    scans::scan_outboxes(&outboxes, &changed, &mut cursor, &mut clock);
                }
                ScanKind::WorktreeFolders => {
                    let scan =
                        scans::scan_worktree_folders(&trees, &changed, &mut cursor, &mut clock);
                    folders = scan.facts;
                }
                // The requests come in Phase 5; an unreadable folder keeps
                // its cursor.
                ScanKind::Requests => {
                    let _ = scans::scan_requests(&root, &mut cursor, &mut clock);
                }
            }
            cursors.push((kind, cursor));
        }
        let repos = git_repositories(self.ports.outside.repositories(conn)?);
        let worktrees = worktree_facts(
            &rows_folders,
            &work_dirs,
            folders,
            last_lists(conn, &repos)?,
        );
        let mut may_stop = may_stop(&rows, &liveness);
        let panes = self.ports.outside.panes();
        let stopped = effects::read_stopped(&root, |pid, start| !may_stop.contains(&(pid, start)))
            .context("reading the records of polite stops")?;
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
        rules.check_i1()?;
        let breaches = self.check_invariants(&tx, &read.worktrees)?;
        self.queue_lists(&ctx, &tx, &read.repos, clock)?;
        record_hold_changes(&tx, &self.holds, &holds)?;
        let mut effects =
            self.intended_effects(&tx, launches, stops, observed.stops, clock, &read.stamp)?;
        effects.extend(doorbells.into_iter().map(Effect::Doorbell));
        effects.sort_by_key(Effect::order);
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
        })
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
        launches: Vec<AttemptRow>,
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
        let ids: Vec<String> = launches.iter().map(|a| a.id.clone()).collect();
        for id in take(tx, LAUNCHES, &ids, budget, stamp)? {
            let a = launches
                .iter()
                .find(|a| a.id == id)
                .expect("an id of the list");
            effects.push(Effect::Launch(a.clone()));
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
                    self.stop_process(&s.row, process, *scope, clock, stops, report)?;
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
    fn stop_process(
        &self,
        row: &RowRef,
        (pid, pid_start): (u32, u64),
        scope: ProcessScope,
        clock: FleetSeconds,
        stops: &mut PaneStops,
        report: &mut PassReport,
    ) -> Option<(Done, bool)> {
        let root = &self.settings.fleet_root;
        let failed = |e: String| format!("the stop of process {pid}: {e}");
        let polite = match effects::read_polite(root, pid, pid_start) {
            Ok(p) => p,
            Err(e) => {
                stops.hold(row);
                report.failed.push(failed(e.to_string()));
                return None;
            }
        };
        let grace = self.settings.config.stop_grace_s;
        let pane = stops.pane_of(pid, pid_start);
        match stop_step(polite.as_ref().map(|p| p.at), clock, grace) {
            StopStep::Polite => {
                stops.hold(row);
                // The record comes first: a restart after the signal still
                // counts its grace and still has the proof of the pane.
                let record = effects::Polite { at: clock, pane };
                if let Err(e) = effects::write_polite(root, pid, pid_start, &record) {
                    report.failed.push(failed(format!("{e:#}")));
                    return None;
                }
                effects::stop_process(pid, pid_start, scope, false)
                    .then(|| (Done::Stop(format!("process {pid}")), false))
            }
            StopStep::Wait => {
                stops.hold(row);
                // A pane proven after the polite stop goes on its record, so
                // the pane closes when the process ends.
                if let Some(p) = polite.filter(|p| pane.is_some() && pane != p.pane) {
                    let record = effects::Polite { pane, ..p };
                    if let Err(e) = effects::write_polite(root, pid, pid_start, &record) {
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
                ladder::stop_sent(&tx, guard, row, process, kill, clock.0)?;
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
        self.pools
            .zip(persona)
            .and_then(|(p, name)| p.roster.get(name))
            .map(|t| t.profile())
            .unwrap_or_default()
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
        retry::Policy {
            retry: self.profile(persona).retry,
            replan_limit_s: self.config.replan_limit_s,
        }
    }

    /// Step 5: rung 6 for each `stalled` task whose old process the pass
    /// sees gone. A task whose old process lives stays `stalled`, so no
    /// claim starts a new attempt beside it.
    fn settle_stalled(&self, facts: &Facts) -> Result<()> {
        let bound = rungs::stop_bound_s(self.config.stop_grace_s, self.config.pass_period_s);
        let grace = self.config.stop_grace_s;
        for s in ladder::settle_stalled(self.tx, self.clock.0, facts, bound, grace)? {
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
    /// `done` moves to `ready`.
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
            for id in promotable(&statuses, &deps) {
                let to = TaskStatus::Ready.as_str();
                let by = cause("each task that it waits for is done (I2)");
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
            interactive: profile.mode == Mode::Interactive,
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
            let task = a.task_id.as_deref()?;
            Some(FolderItem {
                key: a.id.clone(),
                path: scans::outbox_dir(root, task, &a.id),
            })
        })
        .collect()
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
