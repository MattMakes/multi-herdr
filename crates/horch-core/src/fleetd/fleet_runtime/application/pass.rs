//! The run of 1 reconcile pass.
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass" (RR-4, RR-7, FM-7,
//! FM-11, FM-12, PL-5). [`Pass::run`] runs 1 pass in 3 parts:
//!
//! 1. It reads the facts with no transaction, each scan within its budget
//!    from its cursor: the processes that rows name, the outboxes, the
//!    worktree folders, the requests, the herdr panes, the health holds and
//!    the launch record of each attempt in `launching`.
//! 2. In 1 transaction it calls the steps of Phase 1 in the order of the
//!    design: `check_generation`, `tick_fleet_clock`, `observe_intents`,
//!    `mark_orphans`, `finish_exited`, the run duties ([`RunDuties`]),
//!    `observe_jobs`, `check_invariants`, `record_hold_changes` and
//!    `intended_effects`. Each effect is a row in an intended state when
//!    the transaction commits. No effect occurs inside it. The run duties
//!    have a [`DutyBudget`]: at most `pass_budget` rows of each paged duty,
//!    and no new duty work after the time budget of the pass.
//! 3. After the commit it does the effects, stops before launches, until
//!    the time budget of the pass ends. The rest stay intended rows for the
//!    next pass. When time is left, the run duties do their effects. Then
//!    it writes the heartbeat and pings the watchdog.
//!
//! A crash before the commit leaves no effect. A crash after it leaves
//! intended rows, and the next pass does each effect once: a launch is
//! keyed by its attempt id, a stop by the pid and start time of its
//! process, and a job start by the id of its row.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::clock_store::{self, ClockStore};
use crate::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord, Watch};
use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::adapters::scans::{
    self, FolderItem, Liveness, ProcessFact, ProcessItem, ScanClock,
};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::adapters::{cursors_store, fleetd_row};
use crate::fleetd::fleet_runtime::application::holds::record_hold_changes;
use crate::fleetd::fleet_runtime::application::jobs::{
    self, JobCtx, JobRunner, NeedsYouItems, NewItem,
};
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline, PassBudget, ScanKind};
use crate::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::health::Hold;
use crate::fleetd::fleet_runtime::domain::heartbeat::{Exe, Heartbeat};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::fleet_runtime::domain::pass::{
    breaches, observe_launch, stop_step, Breach, EffectKind, Holding, Launch, LaunchStep, Proc,
    StopStep, TIMED_OUT,
};
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, Facts, PaneProof, Reap, Recorded, RowRef, Rows, Seen, Stop, Target,
};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::runtime::fault::Faults;
use crate::taskledger::util;

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
/// The kind of the needs-you item of a breach that no step repairs.
const BREACH: &str = "invariant_breach";

/// What a pass reads and does outside the ledger and the process table.
/// The composition root of `horch fleetd` gives it; a test gives fakes.
pub trait Outside {
    /// The panes of the herdr snapshot, each with the process in it when
    /// herdr reports one. `None` when herdr gives no snapshot.
    fn panes(&self) -> Option<BTreeMap<String, Option<Recorded>>>;

    /// Close the exact pane `pane`.
    fn close_pane(&self, pane: &str) -> Result<()>;

    /// The health holds now (OR-4, OR-5).
    fn holds(&self) -> Vec<Hold>;

    /// The folder of each worktree that is not cleaned, for the scan of
    /// worktree folders. The rows are not rows of fleet runtime.
    fn worktree_folders(&self, conn: &Connection) -> Result<Vec<FolderItem>>;

    /// The launch spec of the attempt `attempt` in `launching`.
    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec>;
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
    Launch(AttemptRow),
    StartJob(JobRow),
}

impl Effect {
    fn kind(&self) -> EffectKind {
        match self {
            Effect::Stop(_) | Effect::StopJob(_) => EffectKind::Stop,
            Effect::Launch(_) => EffectKind::Launch,
            Effect::StartJob(_) => EffectKind::StartJob,
        }
    }
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
        let (mut report, pending) = self.do_effects(conn, decided.effects, decided.clock, deadline);
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
        let trees = self.ports.outside.worktree_folders(conn)?;
        let changed = self.changed_folders(&outboxes, &trees);
        let mut cursors = Vec::new();
        let mut liveness = BTreeMap::new();
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
                    scans::scan_worktree_folders(&trees, &changed, &mut cursor, &mut clock);
                }
                // The requests come in Phase 5; an unreadable folder keeps
                // its cursor.
                ScanKind::Requests => {
                    let _ = scans::scan_requests(&root, &mut cursor, &mut clock);
                }
            }
            cursors.push((kind, cursor));
        }
        let mut may_stop = may_stop(&rows, &liveness);
        let panes = self.ports.outside.panes();
        let stopped = effects::read_stopped(&root, |pid, start| !may_stop.contains(&(pid, start)))
            .context("reading the records of polite stops")?;
        // With no snapshot, no pane can be judged: a record that names a
        // pane, or that does not parse, waits for a pass with one.
        if panes.is_none() {
            may_stop.extend(
                stopped
                    .iter()
                    .filter(|s| s.proof != PaneProof::None)
                    .filter_map(|s| Some((s.process.pid, s.process.pid_start?))),
            );
        }
        Ok(Read {
            stamp,
            cursors,
            task: Facts {
                processes: task_processes(&rows, &liveness),
                panes,
                stopped,
            },
            jobs: job_facts(&processes.jobs, &liveness),
            launching: launching(&rows, &records, &liveness),
            may_stop,
            holds: self.ports.outside.holds(),
            awake: clock_store::awake_now()?,
        })
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
        let launches = self.observe_intents(&tx, &read.launching, clock)?;
        let mut stops = commands::mark_orphans(&tx, &read.task)?;
        stops.extend(commands::finish_exited(&tx, &read.task)?);
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
        let breaches = self.check_invariants(&tx)?;
        record_hold_changes(&tx, &self.holds, &read.holds)?;
        let effects =
            self.intended_effects(&tx, launches, stops, observed.stops, clock, &read.stamp)?;
        tx.commit().map_err(anyhow::Error::from)?;
        self.clock.committed(tick);
        self.holds = read.holds;
        Ok(Decided {
            clock,
            effects,
            breaches,
            may_stop: read.may_stop,
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
    /// it gets 1 needs-you item. An item of the last pass whose breach
    /// ended is resolved. Returns the keys of the open breaches.
    fn check_invariants(&self, tx: &Transaction<'_>) -> Result<BTreeSet<String>> {
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
        for gone in self.breaches.difference(&keys) {
            self.ports.needs_you.resolve_item(tx, gone)?;
        }
        Ok(keys)
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
        for id in take(tx, JOB_STARTS, &ids, budget, stamp)? {
            let i = ids
                .iter()
                .position(|k| *k == id)
                .expect("a key of the list");
            effects.push(Effect::StartJob(due[i].clone()));
        }
        effects.sort_by_key(Effect::kind);
        Ok(effects)
    }

    // ─── part 3 ─────────────────────────────────────────────────────────

    fn do_effects(
        &mut self,
        conn: &mut Connection,
        effects: Vec<Effect>,
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
                Effect::Stop(s) => self.stop(&s, clock, &mut stops, &mut report),
                Effect::StopJob(s) => jobs::stop(&s).then_some(Done::StopJob(s.job_id)),
                Effect::Launch(a) => self.launch(&a, clock, &mut report),
                Effect::StartJob(job) => self.start_job(conn, &job, clock, &mut report),
            };
            report.done.extend(done);
        }
        let pending = stops
            .proofs
            .into_keys()
            .filter(|k| !stops.closed.contains(k));
        (report, pending.collect())
    }

    fn stop(
        &mut self,
        s: &Stop,
        clock: FleetSeconds,
        stops: &mut PaneStops,
        report: &mut PassReport,
    ) -> Option<Done> {
        let root = &self.settings.fleet_root;
        match &s.target {
            Target::Process {
                pid,
                pid_start,
                scope,
            } => {
                let polite = match effects::read_polite(root, *pid, *pid_start) {
                    Ok(p) => p,
                    Err(e) => {
                        stops.in_grace.insert(s.row.clone());
                        report
                            .failed
                            .push(format!("the stop of process {pid}: {e}"));
                        return None;
                    }
                };
                let grace = self.settings.config.stop_grace_s;
                let pane = stops.proofs.get(&(*pid, *pid_start)).cloned();
                match stop_step(polite.as_ref().map(|p| p.at), clock, grace) {
                    StopStep::Polite => {
                        stops.in_grace.insert(s.row.clone());
                        // The record comes first: a restart after the
                        // signal still counts its grace and still has the
                        // proof of the pane.
                        let record = effects::Polite { at: clock, pane };
                        if let Err(e) = effects::write_polite(root, *pid, *pid_start, &record) {
                            report
                                .failed
                                .push(format!("the stop of process {pid}: {e:#}"));
                            return None;
                        }
                        effects::stop_process(*pid, *pid_start, *scope, false)
                            .then(|| Done::Stop(format!("process {pid}")))
                    }
                    StopStep::Wait => {
                        stops.in_grace.insert(s.row.clone());
                        // A pane proven after the polite stop goes on its
                        // record, so the pane closes when the process ends.
                        if let Some(p) = polite.filter(|p| pane.is_some() && pane != p.pane) {
                            let record = effects::Polite { pane, ..p };
                            if let Err(e) = effects::write_polite(root, *pid, *pid_start, &record) {
                                report
                                    .failed
                                    .push(format!("the stop of process {pid}: {e:#}"));
                            }
                        }
                        None
                    }
                    StopStep::Kill => effects::stop_process(*pid, *pid_start, *scope, true)
                        .then(|| Done::Stop(format!("kill process {pid}"))),
                }
            }
            Target::Pane { pane, shell } if !stops.in_grace.contains(&s.row) => {
                match self.ports.outside.close_pane(pane) {
                    Ok(()) => {
                        stops
                            .closed
                            .extend(shell.pid_start.map(|start| (shell.pid, start)));
                        Some(Done::Stop(format!("pane {pane}")))
                    }
                    Err(e) => {
                        report.failed.push(format!("closing pane {pane}: {e:#}"));
                        None
                    }
                }
            }
            Target::Pane { .. } => None,
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
