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
//!    the transaction commits. No effect occurs inside it.
//! 3. After the commit it does the effects, stops before launches, until
//!    the time budget of the pass ends. The rest stay intended rows for the
//!    next pass. Then it writes the heartbeat and pings the watchdog.
//!
//! A crash before the commit leaves no effect. A crash after it leaves
//! intended rows, and the next pass does each effect once: a launch is
//! keyed by its attempt id, a stop by the pid and start time of its
//! process, and a job start by the id of its row.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::Result;
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
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
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
    AttemptRow, AttemptState, Facts, Reap, Recorded, RowRef, Rows, Seen, Stop, Target,
};
use crate::fleetd::worker_profiles::application::ports::ProcessHost;
use crate::runtime::fault::Faults;
use crate::taskledger::model::Actor;
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

    /// The process of the attempt `attempt` in `launching`.
    fn attempt_spec(
        &self,
        attempt: &AttemptRow,
    ) -> Result<crate::fleetd::worker_profiles::application::ports::LaunchSpec>;
}

/// What the run duties see in part 2.
pub struct DutyCtx<'a> {
    /// The generation of this fleetd.
    pub guard: Generation,
    /// The fleet clock of this pass.
    pub clock: FleetSeconds,
    /// The process and pane facts of part 1.
    pub facts: &'a Facts,
}

/// The duties of the watchdog of today, in part 2 of the pass. Ticket 22
/// implements it.
pub trait RunDuties {
    /// Write the rows of the duties in `tx`.
    fn run(&self, tx: &Connection, ctx: &DutyCtx<'_>) -> Result<()>;
}

/// No run duties.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDuties;

impl RunDuties for NoDuties {
    fn run(&self, _tx: &Connection, _ctx: &DutyCtx<'_>) -> Result<()> {
        Ok(())
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
    /// Each attempt in `launching`, with what part 1 saw of its row process
    /// and of its launch.
    launching: Vec<(AttemptRow, Proc, Option<Launch>)>,
    holds: Vec<Hold>,
    awake: AwakeReading,
}

/// The decisions of part 2.
struct Decided {
    clock: FleetSeconds,
    effects: Vec<Effect>,
    breaches: BTreeSet<String>,
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
    /// The time of the polite stop of each process, on the fleet clock.
    polite: BTreeMap<(u32, u64), FleetSeconds>,
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
            polite: BTreeMap::new(),
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
        let decided = match self.decide(conn, read) {
            Ok(d) => d,
            Err(PassError::Failed(e)) => {
                util::record_refusal_after_rollback(conn, &e);
                return Err(PassError::Failed(e));
            }
            Err(e) => return Err(e),
        };
        self.settings.faults.abort_if(ABORT_AFTER_COMMIT);
        let mut report = self.do_effects(conn, decided.effects, decided.clock, deadline);
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
        let watched: Vec<PathBuf> = outboxes
            .iter()
            .chain(&trees)
            .map(|i| i.path.clone())
            .collect();
        self.watch.follow(&watched);
        let mut changed = self.watch.changed();
        if self.settings.faults.has(DROP_WATCH_EVENTS) {
            changed.clear();
        }

        let budget_ms = self.settings.config.scan_budget_ms;
        let mut cursors = Vec::new();
        let mut liveness = BTreeMap::new();
        for kind in ScanKind::ALL {
            let mut cursor = cursors_store::load(conn, kind.as_str(), &stamp)?;
            let start = (self.now)();
            let mut clock = ScanClock {
                deadline: deadline.scan(start, budget_ms),
                now: &mut *self.now,
                stamp: &stamp,
            };
            match kind {
                ScanKind::Processes => {
                    let scan = scans::scan_processes(&processes, &mut cursor, &mut clock);
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

        let seen = |r: Option<Recorded>| match r {
            None => Proc::Absent,
            Some(r) => match liveness.get(&process_key(r)) {
                Some(Liveness::Alive) => Proc::Alive,
                Some(Liveness::Dead) => Proc::Dead,
                Some(Liveness::Unknown) | None => Proc::Unknown,
            },
        };
        let task = Facts {
            processes: rows
                .named_processes()
                .into_iter()
                .map(|r| (r, to_seen(seen(Some(r)))))
                .collect(),
            panes: self.ports.outside.panes(),
        };
        let jobs = liveness
            .iter()
            .filter(|(k, _)| !k.starts_with(PROCESS_PREFIX))
            .map(|(k, l)| ProcessFact {
                key: k.clone(),
                liveness: *l,
            })
            .collect();
        let launching = rows
            .attempts
            .iter()
            .filter(|a| a.state == AttemptState::Launching)
            .map(|a| {
                let launch = records.get(&a.id).map(|r| Launch {
                    at: r.launched_at,
                    process: seen(r.process),
                });
                (a.clone(), seen(a.process), launch)
            })
            .collect();
        Ok(Read {
            stamp,
            cursors,
            task,
            jobs,
            launching,
            holds: self.ports.outside.holds(),
            awake: clock_store::awake_now()?,
        })
    }

    // ─── part 2 ─────────────────────────────────────────────────────────

    fn decide(&mut self, conn: &mut Connection, read: Read) -> Result<Decided, PassError> {
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
        let duty = DutyCtx {
            guard: self.settings.generation,
            clock,
            facts: &read.task,
        };
        self.ports.duties.run(&tx, &duty)?;
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
    /// `live`; no process past the start timeout -> `ended` with
    /// `timed_out`; no launch yet -> an intended launch, returned.
    fn observe_intents(
        &self,
        tx: &Transaction<'_>,
        launching: &[(AttemptRow, Proc, Option<Launch>)],
        clock: FleetSeconds,
    ) -> Result<Vec<AttemptRow>> {
        let mut launches = Vec::new();
        for (a, row, launch) in launching {
            let state = door::state_of(tx, Entity::Attempt, &a.id)?;
            if state.as_deref() != Some(AttemptState::Launching.as_str()) {
                continue;
            }
            match observe_launch(*row, *launch, clock) {
                LaunchStep::Launch => launches.push(a.clone()),
                LaunchStep::Wait => {}
                LaunchStep::Live => {
                    let cause = Cause {
                        actor: Actor::Horch,
                        text: "a live process has the attempt id",
                    };
                    let guard = Some(self.settings.generation);
                    transition(
                        tx,
                        guard,
                        Entity::Attempt,
                        &a.id,
                        AttemptState::Live.as_str(),
                        cause,
                        self.ports.needs_you,
                    )?;
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
        let ids: Vec<String> = due.iter().map(|j| format!("{:020}", j.id)).collect();
        for id in take(tx, JOB_STARTS, &ids, budget, stamp)? {
            let job = due
                .iter()
                .find(|j| format!("{:020}", j.id) == id)
                .expect("an id of the list");
            effects.push(Effect::StartJob(job.clone()));
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
    ) -> PassReport {
        let mut report = PassReport::default();
        let named: BTreeSet<(u32, u64)> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Stop(Stop {
                    target: Target::Process { pid, pid_start, .. },
                    ..
                }) => Some((*pid, *pid_start)),
                _ => None,
            })
            .collect();
        // The rows with a process that is still in its grace: their panes
        // close on a later pass, when the process is gone.
        let mut in_grace: BTreeSet<RowRef> = BTreeSet::new();
        let total = effects.len();
        for (i, e) in effects.into_iter().enumerate() {
            if deadline.reached((self.now)()) {
                report.left = total - i;
                break;
            }
            let done = match e {
                Effect::Stop(s) => self.stop(&s, clock, &mut in_grace),
                Effect::StopJob(s) => jobs::stop(&s).then_some(Done::StopJob(s.job_id)),
                Effect::Launch(a) => self.launch(&a, clock, &mut report),
                Effect::StartJob(job) => self.start_job(conn, &job, clock, &mut report),
            };
            report.done.extend(done);
        }
        self.polite.retain(|k, _| named.contains(k));
        report
    }

    fn stop(
        &mut self,
        s: &Stop,
        clock: FleetSeconds,
        in_grace: &mut BTreeSet<RowRef>,
    ) -> Option<Done> {
        match &s.target {
            Target::Process {
                pid,
                pid_start,
                pgid,
            } => {
                let key = (*pid, *pid_start);
                let grace = self.settings.config.stop_grace_s;
                match stop_step(self.polite.get(&key).copied(), clock, grace) {
                    StopStep::Polite => {
                        in_grace.insert(s.row.clone());
                        self.polite.insert(key, clock);
                        effects::stop_process(*pid, *pid_start, *pgid, false)
                            .then(|| Done::Stop(format!("process {pid}")))
                    }
                    StopStep::Wait => {
                        in_grace.insert(s.row.clone());
                        None
                    }
                    StopStep::Kill => effects::stop_process(*pid, *pid_start, *pgid, true)
                        .then(|| Done::Stop(format!("kill process {pid}"))),
                }
            }
            Target::Pane(pane) if !in_grace.contains(&s.row) => {
                match self.ports.outside.close_pane(pane) {
                    Ok(()) => Some(Done::Stop(format!("pane {pane}"))),
                    Err(_) => None,
                }
            }
            Target::Pane(_) => None,
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

/// The key prefix of a recorded process in the process scan. A job process
/// has the id of its row as its key.
const PROCESS_PREFIX: &str = "p:";

fn process_key(r: Recorded) -> String {
    match r.pid_start {
        Some(start) => format!("{PROCESS_PREFIX}{}:{start}", r.pid),
        None => format!("{PROCESS_PREFIX}{}:-", r.pid),
    }
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
) -> Vec<ProcessItem> {
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
    items.extend(jobs);
    items
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
