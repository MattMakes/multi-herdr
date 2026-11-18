//! The duties of the old watchdog, done in the pass (FM-7).
//!
//! `docs/specs/fleetd/design.md`, "Supervision, liveness and
//! reconciliation" and "Migration plan" item 3. [`WatchdogDuties`] runs
//! each duty of `horch ledger watch` for each active run, in the 3 parts of
//! the pass ([`RunDuties`]):
//!
//! 1. [`RunDuties::observe`] reads the facts with no transaction: herdr and
//!    the transcript of each worker that is due for a check or has no claim,
//!    and the lock holder and the herdr workspace of each orchestrator.
//! 2. [`RunDuties::run`] writes the rows in the transaction of the pass,
//!    with the rules of [`crate::taskledger::watchdog`]: the check of each
//!    due worker ([`watchdog::check_observed`]), which reaps no worker; the
//!    end of a pre-claim worker whose supervisor is gone, with the stops of
//!    its process and pane; the flag of an unclaimed spawn; the re-wake of startable tasks; the wake for
//!    reviews that are in; and the check of the orchestrator. A respawn of
//!    the orchestrator is an attempt in `launching` with no task
//!    (`docs/tickets/fleetd-p1/MAP.md`, Q6). Each wake is an outbox row.
//! 3. [`RunDuties::act`] does the effects of the committed rows: the
//!    respawn of each orchestrator attempt in `launching` that has no launch
//!    record, then the delivery of the outbox of each run. The pass does the
//!    stops.
//!
//! Each effect comes from a committed row, never from memory. So a fleetd
//! that dies after the commit leaves the row, and the next pass does the
//! effect: the respawn from the attempt in `launching`, the stop of an
//! ended worker from `mark_orphans`, and the wake from the outbox.
//!
//! Each duty that reads rows has a work budget (PL-5, A32): it reads 1
//! page of at most `pass_budget` rows from its cursor in `pass_cursors`
//! ([`WORKER_CHECKS`], [`PRECLAIM`], [`BLOCKER_TASKS`], [`HELD_TASKS`]),
//! and the next pass goes on after the page. The cursor is saved in the
//! transaction of part 2. A duty that needs the whole run (the stuck, the
//! startable and the settled wake) asks 1 count query first, and reads the
//! rows only when the count says that the wake may be due. No duty starts
//! after the time budget of the pass.
//!
//! "3 checks of 600 s" no longer reaps a worker: the leases and the stall
//! ladder of the pass do (`docs/tickets/fleetd-p2/MAP.md`, Q14). The run
//! numbers `check_every_s` and `stall_rounds` only set when a check is due
//! and when a first stall asks for an update.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{Connection, Transaction};

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::application::pass::{
    ActCtx, DutyBudget, DutyCtx, DutyReport, RunDuties,
};
use crate::fleetd::fleet_runtime::domain::budget::{Cursor, PassBudget};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, Recorded, RowRef, Stop,
};
use crate::taskledger::model::{Actor, WorkerStatus};
use crate::taskledger::orchestrator::{self, Holder, Outcome, Probe, Respawn};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::watchdog::{self, Checked, Observation, Observer, Sighting};
use crate::taskledger::workers::{self, Preclaim, Worker};
use crate::taskledger::{outbox, tasks, util, wakes};

/// The outcome of an orchestrator attempt whose agent is gone.
const GONE: &str = "gone";

/// The cursor of the checks of the working workers in `pass_cursors`.
pub const WORKER_CHECKS: &str = "duty_worker_checks";
/// The cursor of the pre-claim sweep and the flag of an unclaimed spawn.
pub const PRECLAIM: &str = "duty_preclaim";
/// The cursor of the wakes of cancelled or parked blockers (I-05, I-06).
pub const BLOCKER_TASKS: &str = "duty_blocker_tasks";
/// The cursor of the questions of attempt-held tasks (I-07).
pub const HELD_TASKS: &str = "duty_held_tasks";

/// The statuses of a worker before its claim.
const BEFORE_CLAIM: [WorkerStatus; 3] = [
    WorkerStatus::Spawning,
    WorkerStatus::Starting,
    WorkerStatus::Claiming,
];

/// The rows of 1 page of a worker duty that part 1 read before the time
/// budget of the pass ended, and the cursor before them.
struct Page {
    workers: Vec<Worker>,
    start: Cursor,
}

/// The effects of the duties outside the ledger. The composition root of
/// `horch fleetd` gives them; a test gives fakes.
pub trait DutyEffects {
    /// Start the orchestrator of the respawn `r`
    /// (`crates/horch/src/cmd/recipes.rs`, `respawn_orchestrator`).
    fn respawn(&self, r: &Respawn) -> Result<()>;

    /// Deliver the outbox rows of `run` that are not delivered.
    fn deliver(&self, run: &Run);

    /// Retry the removal of each worktree whose cleanup failed
    /// (`taskledger::finish::land::retry_cleanup`). Returns 1 note for each
    /// worktree that it tried.
    fn retry_cleanups(&self) -> Result<Vec<String>>;
}

/// What part 1 read about the orchestrator of 1 workspace.
#[derive(Debug, Clone, Default)]
struct Seen {
    holder: Option<Holder>,
    open: Option<bool>,
}

/// The facts of part 1, and the board sighting of each run between passes.
#[derive(Default)]
struct State {
    /// The observation of each worker that is due or has no claim.
    observed: BTreeMap<String, Observation>,
    /// The orchestrator facts of each workspace.
    workspaces: BTreeMap<String, Seen>,
    /// The board that each run showed with startable tasks, and since when.
    sightings: BTreeMap<String, Option<Sighting>>,
    /// The page of [`WORKER_CHECKS`] and of [`PRECLAIM`] that part 1 read.
    pages: BTreeMap<&'static str, Page>,
    /// The runs whose orchestrator part 1 probed: part 2 checks only them.
    probed: BTreeSet<String>,
}

/// The orchestrator probe of part 2: the facts of part 1.
struct Facts<'a> {
    seen: &'a BTreeMap<String, Seen>,
    live: &'a dyn Probe,
}

impl Probe for Facts<'_> {
    fn holder(&self, workspace: &str) -> Option<Holder> {
        self.seen.get(workspace).and_then(|s| s.holder)
    }

    fn alive(&self, pid: u32, started: Option<u64>) -> bool {
        self.live.alive(pid, started)
    }

    fn workspace_open(&self, workspace: &str) -> Option<bool> {
        self.seen.get(workspace).and_then(|s| s.open)
    }
}

/// The duties of the old watchdog for each active run.
pub struct WatchdogDuties<'a> {
    observer: &'a dyn Observer,
    probe: &'a dyn Probe,
    effects: &'a dyn DutyEffects,
    /// The state folder of the mail, for the open questions of each worker.
    state_root: PathBuf,
    state: RefCell<State>,
    /// Check each working worker, due or not (`horch ledger watch --once
    /// --force`).
    force: bool,
    /// The checks of the last pass, with the run of each.
    checked: RefCell<Vec<(String, Checked)>>,
    /// The keys of the rows that each paged duty read in the last pass.
    rows: RefCell<BTreeMap<&'static str, Vec<String>>>,
}

impl<'a> WatchdogDuties<'a> {
    pub fn new(
        observer: &'a dyn Observer,
        probe: &'a dyn Probe,
        effects: &'a dyn DutyEffects,
        state_root: PathBuf,
    ) -> WatchdogDuties<'a> {
        WatchdogDuties {
            observer,
            probe,
            effects,
            state_root,
            state: RefCell::new(State::default()),
            force: false,
            checked: RefCell::new(Vec::new()),
            rows: RefCell::new(BTreeMap::new()),
        }
    }

    /// Check each working worker in each pass, due or not.
    pub fn forced(mut self) -> Self {
        self.force = true;
        self
    }

    /// The checks of the last pass, with the run id of each.
    pub fn take_checked(&self) -> Vec<(String, Checked)> {
        self.checked.take()
    }

    /// The keys of the rows that each paged duty read in the last pass,
    /// by the kind of its cursor.
    pub fn take_rows(&self) -> BTreeMap<&'static str, Vec<String>> {
        self.rows.take()
    }

    /// Read the page of the worker duty `kind`, the workers with a status
    /// in `statuses`, from its cursor. Each worker that `observe` names is
    /// observed. The page ends at the rows of `budget` or when its time is
    /// up.
    fn read_page(
        &self,
        conn: &Connection,
        kind: &'static str,
        statuses: &[WorkerStatus],
        budget: DutyBudget<'_>,
        observe: &dyn Fn(&Worker) -> Result<bool>,
        state: &mut State,
    ) -> Result<()> {
        let start = cursors_store::load(conn, kind, &util::now())?;
        let mut workers = Vec::new();
        let after = start.after_key.as_deref();
        for w in watchdog::worker_page(conn, statuses, after, budget.rows.rows())? {
            if (budget.time_up)() {
                break;
            }
            if observe(&w)? {
                let o = self.observer.observe(&w);
                state.observed.insert(w.worker_id.clone(), o);
            }
            workers.push(w);
        }
        state.pages.insert(kind, Page { workers, start });
        Ok(())
    }

    /// Check each due worker of the [`WORKER_CHECKS`] `page` that part 1
    /// observed, in the order of the cursor, until the time budget ends. A
    /// first stall or a suspect gets its wake. The cursor moves through the
    /// last worker done. No check reaps a worker.
    fn check_page(
        &self,
        tx: &Transaction<'_>,
        ctx: &DutyCtx<'_>,
        runs: &[Run],
        page: Page,
        observed: &BTreeMap<String, Observation>,
        now: &str,
    ) -> Result<()> {
        let mut cursor = page.start;
        let mut keys = Vec::new();
        let mut checked = self.checked.borrow_mut();
        for w in &page.workers {
            if (ctx.budget.time_up)() {
                break;
            }
            cursor.step(&w.worker_id, now);
            keys.push(w.worker_id.clone());
            let Some(run) = run_of(runs, w) else {
                continue;
            };
            let Some(c) = check_worker(tx, run, &w.worker_id, observed, now, self.force)? else {
                continue;
            };
            checked.push((run.run_id.clone(), c));
        }
        cursors_store::save(tx, WORKER_CHECKS, &cursor)?;
        self.rows.borrow_mut().insert(WORKER_CHECKS, keys);
        Ok(())
    }

    /// The pre-claim sweep (#190) and the flag of an unclaimed spawn
    /// (TRF-23) of each worker of the [`PRECLAIM`] `page`, in the order of
    /// the cursor, until the time budget ends. A row that no process stands
    /// for ends, and a worker that does not claim gets 1 question. The
    /// workers of 1 run that ended get 1 wake after the page. The cursor
    /// moves through the last worker done. Returns the workers that ended:
    /// their processes and panes are stopped in part 3.
    fn preclaim_page(
        &self,
        tx: &Transaction<'_>,
        ctx: &DutyCtx<'_>,
        runs: &[Run],
        page: Page,
        observed: &BTreeMap<String, Observation>,
        now: &str,
    ) -> Result<Vec<String>> {
        let status = |w: &Worker| {
            observed
                .get(&w.worker_id)
                .and_then(|o| o.herdr_status.clone())
        };
        let mut cursor = page.start;
        let mut keys = Vec::new();
        let mut ended: BTreeMap<String, Vec<Preclaim>> = BTreeMap::new();
        for w in &page.workers {
            if (ctx.budget.time_up)() {
                break;
            }
            cursor.step(&w.worker_id, now);
            keys.push(w.worker_id.clone());
            let Some(run) = run_of(runs, w) else {
                continue;
            };
            let rows: Vec<Worker> = workers::find(tx, &w.worker_id)?.into_iter().collect();
            for p in workers::sweep_preclaim_of(tx, run, rows, &workers::supervisor_alive)? {
                if p.ended.is_some() {
                    ended.entry(run.run_id.clone()).or_default().push(p);
                } else {
                    write(tx, run, wakes::preclaim_wakes(tx, run, &[p], &status))?;
                }
            }
            flag_unclaimed(tx, run, &w.worker_id, observed, now)?;
        }
        let mut out = Vec::new();
        for run in runs {
            let Some(swept) = ended.remove(&run.run_id) else {
                continue;
            };
            write(tx, run, wakes::preclaim_wakes(tx, run, &swept, &status))?;
            out.extend(swept.into_iter().map(|p| p.worker.worker_id));
        }
        cursors_store::save(tx, PRECLAIM, &cursor)?;
        self.rows.borrow_mut().insert(PRECLAIM, keys);
        Ok(out)
    }
}

/// The id of the attempt of respawn `number` of the orchestrator row
/// `worker`.
pub fn respawn_attempt_id(worker: &str, number: i64) -> String {
    format!("{worker}/R{number}")
}

/// The respawn number of the attempt `a` of an orchestrator, when `a` is
/// one.
fn respawn_number(a: &AttemptRow) -> Option<i64> {
    if a.task_id.is_some() {
        return None;
    }
    let (worker, n) = a.id.rsplit_once("/R")?;
    (worker == a.worker_id).then(|| n.parse().ok()).flatten()
}

/// The workspace of the orchestrator of `run`, when it has one.
fn orchestrator_workspace(conn: &Connection, run: &Run) -> Result<Option<String>> {
    let Some(id) = run.orchestrator_worker_id.as_deref() else {
        return Ok(None);
    };
    let ws = workers::find(conn, id)?.and_then(|w| w.workspace_id);
    Ok(ws.or_else(|| run.workspace_id.clone()))
}

impl RunDuties for WatchdogDuties<'_> {
    fn observe(&self, conn: &Connection, budget: DutyBudget<'_>) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        state.observed.clear();
        state.workspaces.clear();
        state.pages.clear();
        state.probed.clear();
        let runs = watchdog::active_runs(conn)?;
        let active: BTreeSet<String> = runs.iter().map(|r| r.run_id.clone()).collect();
        state.sightings.retain(|run, _| active.contains(run));
        let now = util::now();
        // `horch ledger watch --once --force` checks each working worker.
        let checks = DutyBudget {
            rows: if self.force {
                PassBudget::new(u64::MAX)
            } else {
                budget.rows
            },
            ..budget
        };
        let due = |w: &Worker| match run_of(&runs, w) {
            Some(run) => watchdog::is_due(conn, run, w, self.force, &now),
            None => Ok(false),
        };
        let working = [WorkerStatus::Working];
        self.read_page(conn, WORKER_CHECKS, &working, checks, &due, state)?;
        let waits = |w: &Worker| Ok(waits_for_claim(w, &now));
        self.read_page(conn, PRECLAIM, &BEFORE_CLAIM, budget, &waits, state)?;
        for run in runs.iter().take_while(|_| !(budget.time_up)()) {
            if let Some(ws) = orchestrator_workspace(conn, run)? {
                let seen = Seen {
                    holder: self.probe.holder(&ws),
                    open: self.probe.workspace_open(&ws),
                };
                state.workspaces.insert(ws, seen);
            }
            state.probed.insert(run.run_id.clone());
        }
        Ok(())
    }

    fn run(&self, tx: &Transaction<'_>, ctx: &DutyCtx<'_>) -> Result<Vec<Stop>> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        self.checked.borrow_mut().clear();
        self.rows.borrow_mut().clear();
        let time_up = ctx.budget.time_up;
        let runs = watchdog::active_runs(tx)?;
        let now = util::now();
        for run in runs.iter().take_while(|_| !time_up()) {
            if let Some(workspace) = run.workspace_id.as_deref() {
                workers::sync_questions(tx, &self.state_root, workspace)?;
            }
        }
        let mut reaped = BTreeSet::new();
        if let Some(page) = state.pages.remove(WORKER_CHECKS) {
            self.check_page(tx, ctx, &runs, page, &state.observed, &now)?;
        }
        if let Some(page) = state.pages.remove(PRECLAIM) {
            reaped.extend(self.preclaim_page(tx, ctx, &runs, page, &state.observed, &now)?);
        }
        for run in runs.iter().take_while(|_| !time_up()) {
            let seen = state.sightings.entry(run.run_id.clone()).or_default();
            run_wakes(tx, run, &now, seen)?;
        }
        // The lost wakes I-05 to I-07 (TRF-51), each from its page.
        let blockers = task_duty(
            tx,
            ctx,
            &runs,
            BLOCKER_TASKS,
            watchdog::blocker_page,
            &|run, t| {
                if watchdog::is_stalled_blocker(tx, t)? {
                    write(tx, run, wakes::blocker_wake(tx, &t.task_id)?)?;
                }
                Ok(())
            },
        )?;
        let held = task_duty(
            tx,
            ctx,
            &runs,
            HELD_TASKS,
            watchdog::held_page,
            &|run, t| write(tx, run, wakes::held_wake(tx, t)),
        )?;
        let mut rows = self.rows.borrow_mut();
        rows.insert(BLOCKER_TASKS, blockers);
        rows.insert(HELD_TASKS, held);
        let facts = Facts {
            seen: &state.workspaces,
            live: self.probe,
        };
        for run in runs.iter().take_while(|_| !time_up()) {
            if state.probed.contains(&run.run_id) {
                check_orchestrator(tx, ctx, run, &now, &facts)?;
            }
        }
        reaped_stops(tx, &reaped, ctx)
    }

    fn launches(&self, a: &AttemptRow) -> bool {
        respawn_number(a).is_some()
    }

    fn act(&self, conn: &Connection, ctx: &ActCtx<'_>) -> DutyReport {
        let mut report = DutyReport::default();
        let runs = match watchdog::active_runs(conn) {
            Ok(r) => r,
            Err(e) => {
                report.failed.push(format!("the active runs: {e:#}"));
                return report;
            }
        };
        match self.effects.retry_cleanups() {
            Ok(notes) => report.done.extend(notes),
            Err(e) => report.failed.push(format!("the cleanup retry: {e:#}")),
        }
        for run in &runs {
            match self.respawn_run(conn, ctx, run) {
                Ok(d) => report.done.extend(d),
                Err(e) => report
                    .failed
                    .push(format!("the respawn of run {}: {e:#}", run.slug)),
            }
            // The wake goes out after the respawn, so the mailbox of the
            // new orchestrator can take it; durable mail keeps it either way.
            self.effects.deliver(run);
        }
        report
    }
}

impl WatchdogDuties<'_> {
    /// Respawn each orchestrator attempt of `run` in `launching` that has no
    /// launch record. The record is written first, so an attempt is
    /// launched at most once.
    fn respawn_run(&self, conn: &Connection, ctx: &ActCtx<'_>, run: &Run) -> Result<Vec<String>> {
        let mut done = Vec::new();
        let Some(orch) = run.orchestrator_worker_id.as_deref() else {
            return Ok(done);
        };
        for a in attempts_store::attempts_of_worker(conn, orch)? {
            let Some(number) = respawn_number(&a) else {
                continue;
            };
            if a.state != AttemptState::Launching
                || effects::read_launch(ctx.fleet_root, &a.id)?.is_some()
            {
                continue;
            }
            let r = respawn_of(conn, run, &a.worker_id, number)?;
            let record = LaunchRecord {
                attempt: a.id.clone(),
                launched_at: ctx.clock,
                process: None,
            };
            effects::write_launch(ctx.fleet_root, &record)?;
            if let Err(e) = self.effects.respawn(&r) {
                orchestrator::launch_failed(conn, &r, &format!("{e:#}"))?;
                return Err(e.context(format!("attempt {}", a.id)));
            }
            done.push(format!("respawn {}", a.id));
        }
        Ok(done)
    }
}

/// The respawn of the attempt of respawn `number` of the orchestrator row
/// `worker`, from the rows that the check of the orchestrator wrote.
fn respawn_of(conn: &Connection, run: &Run, worker: &str, number: i64) -> Result<Respawn> {
    let new = workers::get(conn, worker)?;
    let previous = match new.previous_worker_id.as_deref() {
        Some(id) => workers::find(conn, id)?,
        None => None,
    };
    let workspace = new
        .workspace_id
        .clone()
        .or_else(|| run.workspace_id.clone())
        .unwrap_or_default();
    Ok(Respawn {
        run_id: run.run_id.clone(),
        slug: run.slug.clone(),
        project_root: run.project_root.clone(),
        workspace_id: workspace,
        old_pane_id: new
            .pane_id
            .clone()
            .or_else(|| previous.as_ref().and_then(|p| p.pane_id.clone())),
        old_worker_id: new
            .previous_worker_id
            .clone()
            .unwrap_or_else(|| new.worker_id.clone()),
        new_worker_id: new.worker_id.clone(),
        number,
        finish_steps: run.finish_steps.clone(),
    })
}

/// Check the worker `id` of `run` when it is due and part 1 observed it.
/// Returns the check. A check reaps no worker: the stall ladder of the
/// pass judges each live attempt (`docs/tickets/fleetd-p2/MAP.md`, Q14).
fn check_worker(
    tx: &Transaction<'_>,
    run: &Run,
    id: &str,
    observed: &BTreeMap<String, Observation>,
    now: &str,
    force: bool,
) -> Result<Option<Checked>> {
    let (Some(w), Some(o)) = (workers::find(tx, id)?, observed.get(id)) else {
        return Ok(None);
    };
    if !watchdog::is_due(tx, run, &w, force, now)? {
        return Ok(None);
    }
    let c = watchdog::check_observed(tx, run, &w, o, now)?;
    write(tx, run, wakes::verdict_wake(&c))?;
    Ok(Some(c))
}

/// End each attempt of `worker` that holds it, with `outcome`.
fn end_attempts(tx: &Connection, worker: &str, outcome: &str, text: &str) -> Result<()> {
    for a in attempts_store::attempts_of_worker(tx, worker)? {
        if !a.state.holds() {
            continue;
        }
        attempts_store::set_ended(tx, &a.id, outcome, &util::now())?;
        let by = Cause {
            actor: Actor::Watchdog,
            text,
        };
        let to = AttemptState::Ended.as_str();
        transition(tx, None, Entity::Attempt, &a.id, to, by, &NeedsYouList)?;
    }
    Ok(())
}

/// The stops of the processes and the panes of the reaped workers and of
/// their attempts: they are orphans now (I5).
fn reaped_stops(
    tx: &Connection,
    reaped: &BTreeSet<String>,
    ctx: &DutyCtx<'_>,
) -> Result<Vec<Stop>> {
    if reaped.is_empty() {
        return Ok(Vec::new());
    }
    let mut attempts = BTreeSet::new();
    for w in reaped {
        for a in attempts_store::attempts_of_worker(tx, w)? {
            attempts.insert(a.id);
        }
    }
    let stops = commands::mark_orphans(tx, ctx.facts)?;
    Ok(stops
        .into_iter()
        .filter(|s| match &s.row {
            RowRef::Worker(w) => reaped.contains(w),
            RowRef::Attempt(a) => attempts.contains(a),
        })
        .collect())
}

/// The active run of `w` in `runs`.
fn run_of<'r>(runs: &'r [Run], w: &Worker) -> Option<&'r Run> {
    runs.iter()
        .find(|r| w.run_id.as_deref() == Some(r.run_id.as_str()))
}

/// The task duty `kind` on the page that `read` gives from its cursor, in
/// the order of the cursor, until the time budget ends: `each` writes the
/// wakes of 1 task of its active run. The cursor moves through the last
/// task done. Returns the keys of the tasks done.
fn task_duty(
    tx: &Transaction<'_>,
    ctx: &DutyCtx<'_>,
    runs: &[Run],
    kind: &str,
    read: fn(&Connection, Option<&str>, usize) -> Result<Vec<tasks::Task>>,
    each: &dyn Fn(&Run, &tasks::Task) -> Result<()>,
) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    if (ctx.budget.time_up)() {
        return Ok(keys);
    }
    let now = util::now();
    let mut cursor = cursors_store::load(tx, kind, &now)?;
    for t in read(tx, cursor.after_key.as_deref(), ctx.budget.rows.rows())? {
        if (ctx.budget.time_up)() {
            break;
        }
        cursor.step(&t.task_id, &now);
        if let Some(run) = runs.iter().find(|r| r.run_id == t.run_id) {
            each(run, &t)?;
        }
        keys.push(t.task_id);
    }
    cursors_store::save(tx, kind, &cursor)?;
    Ok(keys)
}

/// The wakes of `run` that need the whole run: the stuck wake, the
/// startable re-wake (TRF-22) and the lost wakes I-03 and I-04 (TRF-51).
/// The stuck, startable and settled wakes ask 1 count query before they
/// read rows; the reviews-in wake is 1 query.
fn run_wakes(
    tx: &Transaction<'_>,
    run: &Run,
    now: &str,
    seen: &mut Option<Sighting>,
) -> Result<()> {
    write(tx, run, wakes::stuck_wake(tx, run)?)?;
    write(tx, run, watchdog::startable_rewake(tx, run, now, seen)?)?;
    write(tx, run, wakes::settled_wake(tx, run)?)?;
    write(tx, run, wakes::reviews_in_wakes(tx, run, now)?)
}

/// Flag the spawned worker `id` of `run` when it has no claim, once.
fn flag_unclaimed(
    tx: &Transaction<'_>,
    run: &Run,
    id: &str,
    observed: &BTreeMap<String, Observation>,
    now: &str,
) -> Result<()> {
    let Some(w) = workers::find(tx, id)? else {
        return Ok(());
    };
    let Some(u) = watchdog::unclaimed(tx, run, w, now)? else {
        return Ok(());
    };
    let status = observed.get(id).and_then(|o| o.herdr_status.clone());
    write(
        tx,
        run,
        watchdog::flag_unclaimed(tx, run, &u, status.as_deref())?,
    )
}

/// Write each wake of `wakes` in the outbox of `run`. A wake whose key
/// exists writes no row.
fn write(
    tx: &Transaction<'_>,
    run: &Run,
    wakes: impl IntoIterator<Item = outbox::Wake>,
) -> Result<()> {
    for wake in wakes {
        outbox::write(tx, &run.run_id, &wake)?;
    }
    Ok(())
}

/// Whether the pre-claim worker `w` waits at `now` for
/// [`watchdog::UNCLAIMED_AFTER_S`] or more: the flag of an unclaimed spawn
/// and the question of the pre-claim sweep name the herdr status of its
/// pane.
fn waits_for_claim(w: &Worker, now: &str) -> bool {
    w.spawned_at
        .as_deref()
        .or(w.started_at.as_deref())
        .and_then(|t| util::seconds_between(t, now))
        .is_some_and(|s| s >= watchdog::UNCLAIMED_AFTER_S)
}

/// The check of the orchestrator of `run`. A registered agent makes its
/// attempt `live`. A respawn ends the attempts of the rows it replaces and
/// writes the attempt of the new row in `launching`; part 3 starts it.
fn check_orchestrator(
    tx: &Transaction<'_>,
    ctx: &DutyCtx<'_>,
    run: &Run,
    now: &str,
    probe: &Facts<'_>,
) -> Result<()> {
    let checked = orchestrator::check(tx, &run.run_id, now, probe)?;
    match checked.outcome {
        Outcome::Registered(pid) => {
            let Some(orch) = runs::get(tx, &run.run_id)?.orchestrator_worker_id else {
                return Ok(());
            };
            let ws = orchestrator_workspace(tx, run)?.unwrap_or_default();
            let started = probe.holder(&ws).and_then(|h| h.started);
            for a in attempts_store::attempts_of_worker(tx, &orch)? {
                if a.state == AttemptState::Launching && respawn_number(&a).is_some() {
                    let p = Recorded {
                        pid,
                        pid_start: started,
                    };
                    commands::attempt_live(tx, Some(ctx.guard), &a.id, Some(p))?;
                }
            }
        }
        Outcome::Respawned(r) => {
            orchestrator::inherit_harness(tx, &r)?;
            let gone = format!("the orchestrator respawn {} replaces it", r.number);
            end_attempts(tx, &r.old_worker_id, GONE, &gone)?;
            end_attempts(tx, &r.new_worker_id, GONE, &gone)?;
            let a = AttemptRow {
                id: respawn_attempt_id(&r.new_worker_id, r.number),
                task_id: None,
                worker_id: r.new_worker_id.clone(),
                state: AttemptState::Launching,
                process: None,
                pane: None,
            };
            attempts_store::insert_attempt(tx, &a, now)?;
            let text = format!("respawn {} of the orchestrator of run {}", r.number, r.slug);
            let by = Cause {
                actor: Actor::Watchdog,
                text: &text,
            };
            created(tx, Some(ctx.guard), Entity::Attempt, &a.id, by)?;
        }
        Outcome::NotRespawned(why) => {
            if let Some(orch) = run.orchestrator_worker_id.as_deref() {
                end_attempts(tx, orch, GONE, &format!("no respawn: {why}"))?;
            }
        }
        Outcome::Nothing | Outcome::Waiting(_) => {}
    }
    Ok(())
}

/// The observer of a real machine: herdr for the pane, the disk for the
/// transcript.
pub struct HerdrObserver {
    pub herdr: crate::workspace::herdr::Herdr,
}

impl Observer for HerdrObserver {
    fn observe(&self, w: &Worker) -> Observation {
        let pane = w
            .pane_id
            .as_deref()
            .and_then(|p| self.herdr.pane_get(p).ok());
        let screen = w
            .pane_id
            .as_deref()
            .and_then(|p| self.herdr.pane_read(p, "visible").ok());
        let (transcript_bytes, transcript_mtime) = transcript_facts(w);
        Observation {
            agent_alive: agent_alive(w),
            herdr_status: pane.as_ref().and_then(|p| p.agent_status.clone()),
            herdr_seq: pane.as_ref().and_then(|p| p.state_change_seq),
            transcript_bytes,
            transcript_mtime,
            screen,
        }
    }
}

/// Whether the agent PID of `w` runs with its recorded start time. An old
/// record has no start time: the PID alone counts.
fn agent_alive(w: &Worker) -> bool {
    match (w.agent_pid, w.agent_started) {
        (Some(p), Some(s)) => crate::procid::is_same(p as u32, s as u64),
        (Some(p), None) => crate::procid::alive(p as u32, None),
        _ => false,
    }
}

/// The size and the write time of the transcript of `w`, when it can be read.
fn transcript_facts(w: &Worker) -> (Option<i64>, Option<String>) {
    let meta = w
        .transcript_path
        .as_deref()
        .and_then(|t| std::fs::metadata(t).ok());
    (
        meta.as_ref().map(|m| m.len() as i64),
        meta.and_then(|m| m.modified().ok())
            .map(|t| crate::clock::stamp(chrono::DateTime::<chrono::Utc>::from(t))),
    )
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    //! Each test uses a temp ledger and a temp fleet root, and runs whole
    //! passes with these duties and fake herdr facts. The crash test runs
    //! the pass in a child process: this test binary again, with the test
    //! [`child_pass`] and the variable [`CHILD`].

    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use rusqlite::{params, Connection};

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::scans::FolderItem;
    use crate::fleetd::fleet_runtime::application::pass::{
        Done, Outside, Pass, PassReport, Ports, Settings, ABORT_AFTER_COMMIT,
    };
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::fleet_runtime::domain::health::Hold;
    use crate::fleetd::fleet_runtime::domain::ledger::Generation;
    use crate::fleetd::worker_profiles::application::ports::{
        LaunchError, LaunchSpec, Launched, ProcessHost,
    };
    use crate::runtime::fault::Faults;
    use crate::taskledger::model::{OutboxEvent, TaskKind, WorkerKind};
    use crate::taskledger::runs::NewRun;
    use crate::taskledger::tasks::{self, ClaimOutcome, NewTask};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::{schema, util};

    /// The variable that makes [`child_pass`] run: the folder of the world.
    const CHILD: &str = "WATCHDOG_DUTIES_TEST_CHILD";

    /// A temp ledger at generation 1, a fleet root and 1 run.
    struct World {
        dir: PathBuf,
        _temp: Option<tempfile::TempDir>,
        conn: Connection,
        run: Run,
    }

    fn open(db: &Path) -> Connection {
        let conn = Connection::open(db).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        conn
    }

    fn world() -> World {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_path_buf();
        let mut conn = open(&dir.join("ledger.sqlite3"));
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
            params![std::process::id(), util::now()],
        )
        .unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            workspace_id: Some("w1".into()),
            ..Default::default()
        };
        let run = runs::create(&conn, &dir, &new_run).unwrap();
        std::fs::create_dir_all(dir.join("fleet")).unwrap();
        World {
            dir,
            _temp: Some(temp),
            conn,
            run,
        }
    }

    fn reopen(dir: &Path) -> World {
        let conn = open(&dir.join("ledger.sqlite3"));
        let run_id: String = conn
            .query_row("SELECT run_id FROM runs LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let run = runs::get(&conn, &run_id).unwrap();
        World {
            dir: dir.to_path_buf(),
            _temp: None,
            conn,
            run,
        }
    }

    /// The ledger stamp `s` seconds ago.
    fn ago(s: i64) -> String {
        crate::clock::stamp(chrono::Utc::now() - chrono::Duration::seconds(s))
    }

    impl World {
        fn root(&self) -> PathBuf {
            self.dir.join("fleet")
        }

        fn settings(&self, faults: Faults) -> Settings {
            Settings {
                fleet_root: self.root(),
                generation: Generation(1),
                config: FleetConfig::default(),
                horch: PathBuf::from("/bin/false"),
                faults,
            }
        }

        /// A worker of the run, in state `status`, with the agent `agent`.
        fn worker(&self, kind: WorkerKind, status: &str, agent: Option<Recorded>) -> String {
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(kind),
                persona: "dev".into(),
                workspace_id: Some("w1".into()),
                ..Default::default()
            };
            let id = workers::insert(&self.conn, &new).unwrap();
            self.conn
                .execute(
                    "UPDATE workers SET status = ?2, agent_pid = ?3, agent_started = ?4
                     WHERE worker_id = ?1",
                    params![
                        id,
                        status,
                        agent.map(|a| a.pid),
                        agent.and_then(|a| a.pid_start).map(|s| s as i64)
                    ],
                )
                .unwrap();
            id
        }

        /// A task that a working worker with the agent `agent` claimed an
        /// hour ago, with no progress since. Returns the task and the
        /// worker.
        fn stalled_task(&self, agent: Recorded) -> (String, String) {
            let new = NewTask::work("a", "tickets/a.md", "dev");
            let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
            let worker = self.worker(WorkerKind::Worker, "spawning", Some(agent));
            match tasks::claim(&self.conn, &worker).unwrap() {
                ClaimOutcome::Claimed(_) => {}
                other => panic!("no claim: {other:?}"),
            }
            let hour = ago(3600);
            self.conn
                .execute(
                    "UPDATE workers SET status = 'working', claimed_at = ?2,
                         last_checkin_at = ?2, last_progress_at = ?2
                     WHERE worker_id = ?1",
                    params![worker, hour],
                )
                .unwrap();
            self.conn
                .execute(
                    "UPDATE runs SET stall_rounds = 1 WHERE run_id = ?1",
                    [&self.run.run_id],
                )
                .unwrap();
            (task.task_id, worker)
        }

        /// The orchestrator of the run, whose agent `agent` is gone.
        fn gone_orchestrator(&self) -> String {
            let id = self.worker(WorkerKind::Orchestrator, "working", Some(Proc::dead()));
            runs::set_orchestrator(&self.conn, &self.run.run_id, &id).unwrap();
            id
        }

        fn count(&self, sql: &str) -> i64 {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }

        fn outbox(&self, event: OutboxEvent) -> i64 {
            self.conn
                .query_row(
                    "SELECT count(*) FROM outbox WHERE event = ?1",
                    [event.as_str()],
                    |r| r.get(0),
                )
                .unwrap()
        }

        fn status(&self, table: &str, id_col: &str, id: &str) -> String {
            self.conn
                .query_row(
                    &format!("SELECT status FROM {table} WHERE {id_col} = ?1"),
                    [id],
                    |r| r.get(0),
                )
                .unwrap()
        }

        /// The respawn attempts of the orchestrator rows: (id, state).
        fn respawn_attempts(&self) -> Vec<(String, String)> {
            let mut st = self
                .conn
                .prepare("SELECT id, state FROM attempts WHERE task_id IS NULL AND id LIKE '%/R%' ORDER BY id")
                .unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        }
    }

    /// The outside of the pass: no herdr snapshot, no health hold and no
    /// launch of its own.
    struct NoOutside;

    impl Outside for NoOutside {
        fn panes(
            &self,
        ) -> Option<BTreeMap<String, crate::fleetd::task_execution::domain::task::InPane>> {
            None
        }

        fn close_pane(&self, _pane: &str) -> Result<()> {
            Ok(())
        }

        fn holds(&self) -> Vec<Hold> {
            Vec::new()
        }

        fn worktree_folders(&self, _conn: &Connection) -> Result<Vec<FolderItem>> {
            Ok(Vec::new())
        }

        fn attempt_spec(&self, a: &AttemptRow) -> Result<LaunchSpec> {
            anyhow::bail!("the pass launched attempt {}", a.id)
        }
    }

    /// A launcher that refuses each launch: the duties launch nothing
    /// through the pass.
    struct NoHost;

    impl ProcessHost for NoHost {
        fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
            panic!("the pass launched {:?}", spec.program)
        }
    }

    /// herdr and the transcript: an agent that is alive and shows nothing.
    struct Quiet;

    impl Observer for Quiet {
        fn observe(&self, _w: &workers::Worker) -> Observation {
            Observation {
                agent_alive: true,
                herdr_status: Some("idle".into()),
                ..Default::default()
            }
        }
    }

    /// The orchestrator facts: no lock holder, and the workspace is open.
    struct NoHolder;

    impl Probe for NoHolder {
        fn holder(&self, _workspace: &str) -> Option<Holder> {
            None
        }

        fn alive(&self, pid: u32, started: Option<u64>) -> bool {
            match started {
                Some(s) => crate::procid::is_same(pid, s),
                None => crate::procid::alive(pid, None),
            }
        }

        fn workspace_open(&self, _workspace: &str) -> Option<bool> {
            Some(true)
        }
    }

    /// The effects: each respawn appends 1 line to `respawns.txt` with the
    /// state of its attempt, read on a second connection; each delivery
    /// is counted.
    struct Record {
        dir: PathBuf,
        delivered: RefCell<usize>,
        cleanups: RefCell<usize>,
    }

    impl Record {
        fn new(w: &World) -> Record {
            Record {
                dir: w.dir.clone(),
                delivered: RefCell::new(0),
                cleanups: RefCell::new(0),
            }
        }

        fn respawns(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("respawns.txt"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl DutyEffects for Record {
        fn respawn(&self, r: &Respawn) -> Result<()> {
            let other = open(&self.dir.join("ledger.sqlite3"));
            let id = respawn_attempt_id(&r.new_worker_id, r.number);
            let state: String = other
                .query_row("SELECT state FROM attempts WHERE id = ?1", [&id], |row| {
                    row.get(0)
                })
                .unwrap_or_else(|_| "no row".into());
            let line = format!("{id} {state}\n");
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("respawns.txt"))?
                .write_all(line.as_bytes())?;
            Ok(())
        }

        fn deliver(&self, _run: &Run) {
            *self.delivered.borrow_mut() += 1;
        }

        fn retry_cleanups(&self) -> Result<Vec<String>> {
            *self.cleanups.borrow_mut() += 1;
            Ok(Vec::new())
        }
    }

    /// 1 pass with these duties.
    fn pass(w: &mut World, duties: &WatchdogDuties<'_>, faults: Faults) -> PassReport {
        let ports = Ports {
            outside: &NoOutside,
            duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        Pass::new(w.settings(faults), ports)
            .run(&mut w.conn)
            .unwrap()
    }

    struct Proc(Child);

    impl Proc {
        fn sleep() -> Proc {
            Proc(Command::new("sleep").arg("60").spawn().unwrap())
        }

        fn recorded(&self) -> Recorded {
            let pid = self.0.id();
            Recorded {
                pid,
                pid_start: horch_platform::process::start_time(pid),
            }
        }

        /// A process that ended and was reaped.
        fn dead() -> Recorded {
            let mut p = Proc::sleep();
            let r = p.recorded();
            p.0.kill().unwrap();
            p.0.wait().unwrap();
            r
        }

        /// Wait until it exits, at most 10 s.
        fn wait_exit(&mut self) -> bool {
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end {
                if let Ok(Some(_)) = self.0.try_wait() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        }
    }

    impl Drop for Proc {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // ─── the tests ──────────────────────────────────────────────────────

    #[test]
    fn pass_flags_an_unclaimed_spawn_once() {
        let mut w = world();
        let worker = w.worker(WorkerKind::Worker, "spawning", None);
        w.conn
            .execute(
                "UPDATE workers SET spawned_at = ?2, last_checkin_at = ?3 WHERE worker_id = ?1",
                params![worker, ago(600), util::now()],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        let flags = w.count("SELECT count(*) FROM events WHERE text LIKE 'No claim:%'");
        assert_eq!(flags, 1, "1 flag for 1 unclaimed spawn");
        assert_eq!(w.outbox(OutboxEvent::UnclaimedSpawn), 1);
        let text: String = w
            .conn
            .query_row("SELECT text FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert!(text.contains("herdr status idle"), "{text}");
        assert_eq!(
            *effects.delivered.borrow(),
            2,
            "each pass delivers the outbox"
        );
    }

    #[test]
    fn pass_checks_a_worker_past_its_stall_rounds_and_reaps_none() {
        let mut w = world();
        let agent = Proc::sleep();
        let (task, worker) = w.stalled_task(agent.recorded());
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        let report = pass(&mut w, &duties, Faults::default());

        let checked = duties.take_checked();
        assert_eq!(checked.len(), 1, "the due worker gets its check");
        assert_eq!(checked[0].1.verdict, "no_progress");
        assert_eq!(w.status("workers", "worker_id", &worker), "working");
        assert_eq!(w.status("tasks", "task_id", &task), "running");
        assert_eq!(w.outbox(OutboxEvent::Reaped), 0);
        assert_eq!(
            w.count("SELECT count(*) FROM events WHERE kind = 'reaped'"),
            0
        );
        let pid = agent.0.id();
        assert!(
            !report.done.contains(&Done::Stop(format!("process {pid}"))),
            "{report:?}"
        );
        assert!(crate::procid::alive(pid, None), "the agent runs on");
    }

    #[test]
    fn pass_writes_a_launching_attempt_for_a_gone_orchestrator_and_respawns_it_after_the_commit() {
        let mut w = world();
        let old = w.gone_orchestrator();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        let report = pass(&mut w, &duties, Faults::default());

        let new = runs::get(&w.conn, &w.run.run_id)
            .unwrap()
            .orchestrator_worker_id
            .unwrap();
        assert_ne!(new, old, "a new orchestrator row takes its place");
        let id = respawn_attempt_id(&new, 1);
        assert_eq!(w.respawn_attempts(), vec![(id.clone(), "launching".into())]);
        assert_eq!(
            effects.respawns(),
            vec![format!("{id} launching")],
            "the respawn sees the committed attempt"
        );
        assert!(report.done.contains(&Done::Duty(format!("respawn {id}"))));
        assert!(report.failed.is_empty(), "{report:?}");

        pass(&mut w, &duties, Faults::default());
        assert_eq!(effects.respawns().len(), 1, "1 respawn for 1 attempt");
    }

    #[test]
    fn pass_rewakes_startable_tasks_once_for_each_board_state() {
        let mut w = world();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());
        let startable = |w: &World, title: &str| {
            let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
            tasks::add(&w.conn, &w.run, &new, Actor::Orchestrator).unwrap();
            // The first startable wake of this board state, 200 s ago.
            let ready: Vec<String> = tasks::startable(&w.conn, &w.run.run_id)
                .unwrap()
                .iter()
                .map(|t| t.short())
                .collect();
            let board = outbox::board(&w.conn, &w.run.run_id, true).unwrap();
            let first =
                outbox::Wake::new(OutboxEvent::Startable, ready.join(","), "first").on_board(board);
            let tx = w.conn.unchecked_transaction().unwrap();
            let id = outbox::write(&tx, &w.run.run_id, &first).unwrap().unwrap();
            tx.execute(
                "UPDATE outbox SET created_at = ?2, delivered_at = ?2 WHERE id = ?1",
                params![id, ago(200)],
            )
            .unwrap();
            tx.commit().unwrap();
        };
        let rewakes = |w: &World| {
            w.count("SELECT count(*) FROM outbox WHERE text LIKE '%no worker is on its way%'")
        };

        startable(&w, "a");
        pass(&mut w, &duties, Faults::default());
        assert_eq!(rewakes(&w), 1, "the board waited 200 s: 1 re-wake");
        pass(&mut w, &duties, Faults::default());
        assert_eq!(rewakes(&w), 1, "the same board state: no second re-wake");

        startable(&w, "b");
        pass(&mut w, &duties, Faults::default());
        assert_eq!(rewakes(&w), 2, "a new board state: 1 more re-wake");
    }

    /// The number of outbox rows whose text has `part`.
    fn wakes_with(w: &World, part: &str) -> i64 {
        w.conn
            .query_row(
                "SELECT count(*) FROM outbox WHERE instr(text, ?1) > 0",
                [part],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn pass_ends_a_preclaim_worker_whose_supervisor_is_gone_and_stops_it() {
        let mut w = world();
        let mut agent = Proc::sleep();
        let worker = w.worker(WorkerKind::Worker, "claiming", Some(agent.recorded()));
        let gone = Proc::dead();
        w.conn
            .execute(
                "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3,
                     last_checkin_at = ?4 WHERE worker_id = ?1",
                params![
                    worker,
                    gone.pid,
                    gone.pid_start.map(|s| s as i64),
                    util::now()
                ],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "no_task");
        assert_eq!(wakes_with(&w, "ended before they claimed a task"), 1);
        assert!(agent.wait_exit(), "part 3 stops the agent of the ended row");
    }

    #[test]
    fn pass_asks_once_about_a_claiming_worker_that_the_sweep_and_the_flag_both_see() {
        let mut w = world();
        let worker = w.worker(WorkerKind::Worker, "claiming", None);
        w.conn
            .execute(
                "UPDATE workers SET spawned_at = ?2, started_at = ?2, last_checkin_at = ?3
                 WHERE worker_id = ?1",
                params![worker, ago(900), util::now()],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "claiming");
        assert_eq!(w.outbox(OutboxEvent::UnclaimedSpawn), 1, "1 question");
        assert_eq!(wakes_with(&w, "herdr status idle"), 1);
    }

    #[test]
    fn pass_wakes_a_stuck_run_once_and_asks_about_a_held_task() {
        let mut w = world();
        let new = NewTask::work("a", "tickets/a.md", "dev");
        let task = tasks::add(&w.conn, &w.run, &new, Actor::Orchestrator).unwrap();
        w.conn
            .execute(
                "UPDATE tasks SET status = 'escalated', held_reason = '3 attempts failed'
                 WHERE task_id = ?1",
                [&task.task_id],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(wakes_with(&w, "the run is stuck"), 1, "1 stuck wake");
        assert_eq!(
            wakes_with(&w, "is held: 3 attempts failed"),
            1,
            "1 held question"
        );
    }

    #[test]
    fn pass_writes_the_settled_wake_once() {
        let mut w = world();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.outbox(OutboxEvent::Settled), 1);
    }

    #[test]
    fn pass_wakes_once_about_a_cancelled_task_that_open_tasks_wait_for() {
        let mut w = world();
        let first = NewTask::work("a", "tickets/a.md", "dev");
        let first = tasks::add(&w.conn, &w.run, &first, Actor::Orchestrator).unwrap();
        let mut second = NewTask::work("b", "tickets/b.md", "dev");
        second.blocked_by = vec![first.task_id.clone()];
        tasks::add(&w.conn, &w.run, &second, Actor::Orchestrator).unwrap();
        tasks::cancel(&w.conn, &first, "drop").unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.outbox(OutboxEvent::Cancelled), 1);
    }

    #[test]
    fn pass_asks_a_worker_on_its_first_stall_for_an_update_once() {
        let mut w = world();
        let agent = Proc::sleep();
        let (task, worker) = w.stalled_task(agent.recorded());
        w.conn
            .execute(
                "UPDATE runs SET stall_rounds = 3 WHERE run_id = ?1",
                [&w.run.run_id],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(
            wakes_with(&w, "shows no progress"),
            1,
            "1 wake on the first stall, none on the second"
        );
        assert_eq!(w.status("workers", "worker_id", &worker), "working");
        assert_eq!(w.status("tasks", "task_id", &task), "running");
    }

    #[test]
    fn pass_writes_the_reviews_in_wake_once() {
        let mut w = world();
        let author = w.worker(WorkerKind::Worker, "working", None);
        let new = NewTask::work("a", "tickets/a.md", "dev");
        let parent = tasks::add(&w.conn, &w.run, &new, Actor::Orchestrator).unwrap();
        w.conn
            .execute(
                "UPDATE tasks SET status = 'running', current_worker_id = ?2 WHERE task_id = ?1",
                [&parent.task_id, &author],
            )
            .unwrap();
        w.conn
            .execute(
                "INSERT INTO task_steps (task_id, worker_id, seq, name, status)
                 VALUES (?1, ?2, 2, 'code-sweep', 'running')",
                [&parent.task_id, &author],
            )
            .unwrap();
        let mut review = NewTask::work("review", "review.md", "rev");
        review.kind = TaskKind::Review;
        review.parent_task_id = Some(parent.task_id.clone());
        let review = tasks::add(&w.conn, &w.run, &review, Actor::Orchestrator).unwrap();
        w.conn
            .execute(
                "UPDATE tasks SET status = 'done', finished_at = ?2 WHERE task_id = ?1",
                params![review.task_id, ago(3600)],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.outbox(OutboxEvent::ReviewsIn), 1);
    }

    #[test]
    fn pass_syncs_open_questions_so_a_waiting_worker_is_not_reaped() {
        let mut w = world();
        let agent = Proc::sleep();
        let (_, worker) = w.stalled_task(agent.recorded());
        w.conn
            .execute(
                "UPDATE workers SET role = 'worker-1' WHERE worker_id = ?1",
                [&worker],
            )
            .unwrap();
        let mail = crate::messaging::mail::Mail::under(&w.dir, "w1");
        let id = mail.enqueue("worker-1", "QUESTION: choose?").unwrap();
        mail.ack(&id).unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "working");
        let waits = w.count("SELECT count(*) FROM worker_waits WHERE kind = 'question'");
        assert_eq!(waits, 1);
    }

    #[test]
    fn pass_retries_the_worktree_cleanups_once_each_pass() {
        let mut w = world();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(*effects.cleanups.borrow(), 2);
    }

    /// Not a test of its own: the pass of a child process of a test. It
    /// runs only with [`CHILD`], and the fault ends it after the commit.
    #[test]
    fn child_pass() {
        let Some(dir) = std::env::var_os(CHILD) else {
            return;
        };
        let mut w = reopen(Path::new(&dir));
        let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());
        pass(&mut w, &duties, faults);
    }

    /// Run [`child_pass`] in a child process that ends after the commit.
    fn crash_after_commit(w: &World) {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fleetd::fleet_runtime::adapters::watchdog_duties::tests::child_pass",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, &w.dir)
            .env("HORCH_FAULT", ABORT_AFTER_COMMIT)
            .status()
            .unwrap();
        assert!(
            !status.success(),
            "the fault ends the child after the commit"
        );
    }

    #[test]
    fn a_crash_between_the_commit_and_the_effects_is_repaired_by_the_next_pass() {
        let mut w = world();
        w.gone_orchestrator();

        crash_after_commit(&w);

        let attempts = w.respawn_attempts();
        assert_eq!(attempts.len(), 1, "{attempts:?}");
        assert_eq!(attempts[0].1, "launching");
        let effects = Record::new(&w);
        assert!(
            effects.respawns().is_empty(),
            "the crash came before part 3"
        );

        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(
            effects.respawns(),
            vec![format!("{} launching", attempts[0].0)]
        );
        pass(&mut w, &duties, Faults::default());
        assert_eq!(effects.respawns().len(), 1, "1 respawn for 1 attempt");
    }

    // ─── the tests of the old `horch ledger watch` pass (ticket 25) ─────

    /// 1 respawn: its number and the harness and model of its row.
    type Try = (i64, Option<String>, Option<String>);

    /// The effects of a respawn that reads the harness and the model of
    /// the new orchestrator row, and fails while `fail` is set.
    struct Launch {
        dir: PathBuf,
        fail: RefCell<bool>,
        /// Each respawn.
        tries: RefCell<Vec<Try>>,
    }

    impl Launch {
        fn new(w: &World, fail: bool) -> Launch {
            Launch {
                dir: w.dir.clone(),
                fail: RefCell::new(fail),
                tries: RefCell::new(Vec::new()),
            }
        }

        fn numbers(&self) -> Vec<i64> {
            self.tries.borrow().iter().map(|t| t.0).collect()
        }
    }

    impl DutyEffects for Launch {
        fn respawn(&self, r: &Respawn) -> Result<()> {
            let other = open(&self.dir.join("ledger.sqlite3"));
            let row = workers::get(&other, &r.new_worker_id)?;
            self.tries
                .borrow_mut()
                .push((r.number, row.harness, row.model));
            if *self.fail.borrow() {
                anyhow::bail!("herdr is down");
            }
            Ok(())
        }

        fn deliver(&self, _run: &Run) {}

        fn retry_cleanups(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    /// The orchestrator facts: the live process `holder` holds the lock of
    /// the workspace.
    struct Held(Recorded);

    impl Probe for Held {
        fn holder(&self, _workspace: &str) -> Option<Holder> {
            Some(Holder {
                pid: self.0.pid,
                started: self.0.pid_start,
            })
        }

        fn alive(&self, pid: u32, started: Option<u64>) -> bool {
            NoHolder.alive(pid, started)
        }

        fn workspace_open(&self, _workspace: &str) -> Option<bool> {
            Some(true)
        }
    }

    impl World {
        /// The current orchestrator row of the run.
        fn orchestrator(&self) -> Worker {
            let id = runs::get(&self.conn, &self.run.run_id)
                .unwrap()
                .orchestrator_worker_id
                .unwrap();
            workers::get(&self.conn, &id).unwrap()
        }

        /// Move the last respawn 10 min and 1 s back: the next respawn is
        /// due (TRF-40).
        fn respawn_due(&self) {
            self.conn
                .execute(
                    "UPDATE runs SET last_respawn_at = ?2 WHERE run_id = ?1",
                    params![self.run.run_id, ago(601)],
                )
                .unwrap();
        }

        fn respawns(&self) -> i64 {
            orchestrator::guard(&self.conn, &self.run.run_id)
                .unwrap()
                .respawns
        }

        /// The texts of the outbox rows of `event`.
        fn rows(&self, event: OutboxEvent) -> Vec<String> {
            let mut st = self
                .conn
                .prepare("SELECT text FROM outbox WHERE event = ?1 ORDER BY rowid")
                .unwrap();
            st.query_map([event.as_str()], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        }
    }

    /// Report 28 D-c (TRF-45): the respawned orchestrator row records the
    /// harness and the model of the row that it replaces before its launch
    /// runs.
    #[test]
    fn pass_respawn_keeps_the_harness_and_the_model_of_the_old_orchestrator() {
        let mut w = world();
        let old = w.gone_orchestrator();
        w.conn
            .execute(
                "UPDATE workers SET harness = 'codex', model = 'gpt-5.6-sol' WHERE worker_id = ?1",
                [&old],
            )
            .unwrap();
        let launch = Launch::new(&w, false);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &launch, w.dir.clone());

        pass(&mut w, &duties, Faults::default());

        let new = w.orchestrator();
        assert_ne!(new.worker_id, old);
        assert_eq!(new.harness.as_deref(), Some("codex"));
        assert_eq!(new.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(
            *launch.tries.borrow(),
            vec![(1, Some("codex".into()), Some("gpt-5.6-sol".into()))],
            "the launch sees the harness and the model"
        );
    }

    /// TRF-38: each failed launch is recorded and counts 1 respawn, so a
    /// launch that always fails cannot loop.
    #[test]
    fn pass_records_a_failed_respawn_launch_and_counts_it() {
        let mut w = world();
        w.gone_orchestrator();
        let launch = Launch::new(&w, true);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &launch, w.dir.clone());

        for _ in 0..4 {
            pass(&mut w, &duties, Faults::default());
            // Within 10 min no respawn follows.
            pass(&mut w, &duties, Faults::default());
            w.respawn_due();
        }

        assert_eq!(launch.numbers(), vec![1, 2, 3]);
        let last = w.orchestrator();
        assert_eq!(last.status, "launch_failed");
        assert!(
            last.end_reason.unwrap().contains("used its 3 respawns"),
            "the last respawn ends the pending row"
        );
        let failed = w.count(
            "SELECT count(*) FROM events
             WHERE text LIKE 'Orchestrator respawn % did not start: herdr is down'",
        );
        assert_eq!(failed, 3);
        assert_eq!(w.respawns(), 3);
    }

    /// TRF-38: a failed launch keeps the pending row and the remaining
    /// respawns; the next respawn launches it.
    #[test]
    fn pass_keeps_the_remaining_respawns_after_a_failed_launch() {
        let mut w = world();
        w.gone_orchestrator();
        let launch = Launch::new(&w, true);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &launch, w.dir.clone());

        pass(&mut w, &duties, Faults::default());
        let pending = w.orchestrator();
        assert_eq!(pending.status, "spawning", "a failed launch stays pending");
        assert_eq!(w.respawns(), 1);

        *launch.fail.borrow_mut() = false;
        pass(&mut w, &duties, Faults::default());
        assert_eq!(launch.numbers(), vec![1], "no respawn within 10 min");
        w.respawn_due();
        pass(&mut w, &duties, Faults::default());
        assert_eq!(launch.numbers(), vec![1, 2]);

        // The launched orchestrator takes the lock of the workspace.
        let agent = Proc::sleep();
        let held = Held(agent.recorded());
        let duties = WatchdogDuties::new(&Quiet, &held, &launch, w.dir.clone());
        pass(&mut w, &duties, Faults::default());
        let live = w.orchestrator();
        assert_eq!(live.worker_id, pending.worker_id);
        assert_eq!(live.status, "working");
        assert_eq!(w.respawns(), 2, "1 respawn of 3 remains");
    }

    /// TRF-51, I-06: T-1 was parked unlanded, and the wake to the tasks
    /// that wait for it was lost. The pass writes it once, and the direct
    /// path after it writes no second row.
    #[test]
    fn pass_writes_a_lost_parked_blocker_wake_once() {
        let mut w = world();
        let a = tasks::add(
            &w.conn,
            &w.run,
            &NewTask::work("a", "a.md", "dev"),
            Actor::Orchestrator,
        )
        .unwrap();
        let mut b = NewTask::work("b", "b.md", "dev");
        b.blocked_by = vec!["T-1".into()];
        tasks::add(&w.conn, &w.run, &b, Actor::Orchestrator).unwrap();
        w.conn
            .execute(
                "UPDATE tasks SET status = 'escalated', held_reason = 'unlanded', attempts = 1
                 WHERE task_id = ?1",
                [&a.task_id],
            )
            .unwrap();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects, w.dir.clone());

        // No worker is live, so the run is stuck too: that wake is apart.
        let parked = |w: &World| -> Vec<String> {
            let rows = w.rows(OutboxEvent::Held);
            rows.into_iter()
                .filter(|r| r.contains("T-2 waits:"))
                .collect()
        };
        pass(&mut w, &duties, Faults::default());
        let rows = parked(&w);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(
            rows[0].contains("T-2 waits: T-1 is parked unlanded"),
            "{rows:?}"
        );

        pass(&mut w, &duties, Faults::default());
        // The direct path (`taskledger_wake::stalled_dependents`) writes
        // the same keyed row.
        let t = tasks::get(&w.conn, &a.task_id).unwrap();
        let waiting = tasks::dependents(&w.conn, &a.task_id).unwrap();
        let wake = wakes::dependents_wake(&t, &waiting).expect("the wake");
        let tx = w.conn.transaction().unwrap();
        outbox::write(&tx, &w.run.run_id, &wake).unwrap();
        tx.commit().unwrap();
        assert_eq!(parked(&w).len(), 1);
    }

    /// herdr and the transcript as [`Quiet`]; it counts each worker it
    /// observes.
    #[derive(Default)]
    struct Counting(RefCell<BTreeMap<String, usize>>);

    impl Observer for Counting {
        fn observe(&self, w: &workers::Worker) -> Observation {
            *self.0.borrow_mut().entry(w.worker_id.clone()).or_default() += 1;
            Quiet.observe(w)
        }
    }

    /// A big run: `working` tasks, each `running` with 1 worker that claimed
    /// it an hour ago and 1 live attempt of this test process; `claiming`
    /// workers that wait for a claim and are not stale; and `held` tasks
    /// that the attempt limit holds.
    fn seed_big_run(w: &World, working: usize, claiming: usize, held: usize) {
        let pid = std::process::id();
        let start = crate::procid::start_time(pid).expect("the start time of this process") as i64;
        let now = util::now();
        let hour = ago(3600);
        let host = util::host_name();
        let run = &w.run.run_id;
        let tx = w.conn.unchecked_transaction().unwrap();
        // FR-12: each task belongs to an idea.
        let idea = crate::taskledger::schema::run_idea(&tx, run, &now, true)
            .unwrap()
            .unwrap();
        {
            let mut task = tx
                .prepare(
                    "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                         claimable_by, status, attempts, current_worker_id, held_reason,
                         created_at, updated_at, idea_id)
                     VALUES (?1, ?2, ?3, ?1, 'tickets/big.md', 'dev', 'dev', ?4, 1, ?5, ?6, ?7, ?7, ?8)",
                )
                .unwrap();
            let mut worker = tx
                .prepare(
                    "INSERT INTO workers (worker_id, run_id, kind, persona, host, workspace_id,
                         agent_pid, agent_started, task_id, attempt, status, spawned_at,
                         started_at, claimed_at, last_checkin_at, last_progress_at)
                     VALUES (?1, ?2, 'worker', 'dev', ?3, 'w1', ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10,
                         ?11, ?10)",
                )
                .unwrap();
            let mut attempt = tx
                .prepare(
                    "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6)",
                )
                .unwrap();
            for n in 1..=working {
                let (t, wk) = (format!("B-{n:05}"), format!("BW-{n:05}"));
                let worker_of = Some(wk.as_str());
                let held: Option<&str> = None;
                task.execute(params![
                    t, run, n as i64, "running", worker_of, held, hour, idea
                ])
                .unwrap();
                let claimed = Some(hour.as_str());
                worker
                    .execute(params![
                        wk, run, host, pid, start, t, 1, "working", hour, claimed, hour
                    ])
                    .unwrap();
                attempt
                    .execute(params![format!("{t}/A1"), t, wk, pid, start, hour])
                    .unwrap();
            }
            for n in 1..=claiming {
                let wk = format!("BC-{n:05}");
                let none: Option<&str> = None;
                let no_attempt: Option<i64> = None;
                worker
                    .execute(params![
                        wk, run, host, pid, start, none, no_attempt, "claiming", now, none, now
                    ])
                    .unwrap();
            }
            for n in 1..=held {
                let t = format!("BH-{n:05}");
                let seq = (working + n) as i64;
                let none: Option<&str> = None;
                task.execute(params![
                    t,
                    run,
                    seq,
                    "escalated",
                    none,
                    Some("attempts"),
                    hour,
                    idea
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }

    /// PL-5, A32: each paged duty of part 2 reads at most `pass_budget` rows
    /// in 1 pass, and the passes of 1 lap reach each row exactly 1 time, in
    /// the order of the cursor of the duty.
    #[test]
    fn each_paged_duty_reads_at_most_the_pass_budget_and_a_lap_reaches_each_row_once() {
        const WORKING: usize = 10_000;
        const CLAIMING: usize = 3_000;
        const HELD: usize = 3_000;
        const BUDGET: usize = 2_500;
        const PASSES: usize = WORKING / BUDGET;
        let mut w = world();
        seed_big_run(&w, WORKING, CLAIMING, HELD);
        let observer = Counting::default();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&observer, &NoHolder, &effects, w.dir.clone());
        let mut settings = w.settings(Faults::default());
        settings.config.pass_budget = BUDGET as u64;
        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        // The clock of the pass stands still: the row budget alone ends
        // each page, on a slow host too. The time budget has its own tests.
        let start = Instant::now();
        let mut pass = Pass::new(settings, ports).with_clock(Box::new(move || start));

        let mut reached: BTreeMap<&'static str, BTreeMap<String, usize>> = BTreeMap::new();
        for n in 1..=PASSES {
            let report = pass.run(&mut w.conn).unwrap();
            assert!(report.heartbeat, "pass {n}: {report:?}");
            let rows = duties.take_rows();
            for (duty, keys) in &rows {
                assert!(
                    keys.len() <= BUDGET,
                    "pass {n}: the duty {duty} read {} rows",
                    keys.len()
                );
                for k in keys {
                    *reached
                        .entry(duty)
                        .or_default()
                        .entry(k.clone())
                        .or_default() += 1;
                }
            }
            assert_eq!(rows[WORKER_CHECKS].len(), BUDGET, "pass {n}");
            let observed: usize = observer.0.borrow().values().sum();
            assert_eq!(observed, n * BUDGET, "pass {n}: herdr observes 1 page");
        }

        // 1 lap of the checks: each working worker once.
        let checks = &reached[WORKER_CHECKS];
        assert_eq!(checks.len(), WORKING);
        assert!(
            checks.values().all(|&c| c == 1),
            "a check row is read twice"
        );
        assert!(observer.0.borrow().values().all(|&c| c == 1));
        let (rows, workers): (i64, i64) = w
            .conn
            .query_row(
                "SELECT count(*), count(DISTINCT worker_id) FROM checks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((rows, workers), (WORKING as i64, WORKING as i64));
        // 10,000 reads of 3,000 claiming workers and of 3,000 held tasks:
        // the cursor wraps, and each row is reached 3 or 4 times, so no row
        // is starved.
        for (duty, n) in [(PRECLAIM, CLAIMING), (HELD_TASKS, HELD)] {
            let keys = &reached[duty];
            assert_eq!(keys.len(), n, "{duty}");
            let fair = (PASSES * BUDGET / n)..=(PASSES * BUDGET).div_ceil(n);
            assert!(keys.values().all(|c| fair.contains(c)), "{duty}: {keys:?}");
        }
        // Each held task has its 1 question, once.
        assert_eq!(wakes_with(&w, "is held: attempts"), HELD as i64);
    }

    /// herdr and the transcript as [`Quiet`]; the observation number `at`
    /// moves the clock of the pass 1 hour on, past its time budget.
    struct SlowAt {
        at: usize,
        seen: RefCell<usize>,
        clock: std::rc::Rc<std::cell::Cell<Instant>>,
    }

    impl Observer for SlowAt {
        fn observe(&self, w: &workers::Worker) -> Observation {
            *self.seen.borrow_mut() += 1;
            if *self.seen.borrow() == self.at {
                self.clock.set(self.clock.get() + Duration::from_secs(3600));
            }
            Quiet.observe(w)
        }
    }

    /// PL-5: the time budget ends in the middle of a page. Part 1 observes
    /// no more workers, part 2 checks none, and the cursor stays before the
    /// first worker not done, so the next pass checks each worker once.
    #[test]
    fn a_page_cut_by_the_time_budget_keeps_its_cursor_before_the_first_row_not_done() {
        let mut w = world();
        seed_big_run(&w, 50, 0, 0);
        let clock = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
        let observer = SlowAt {
            at: 10,
            seen: RefCell::new(0),
            clock: clock.clone(),
        };
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&observer, &NoHolder, &effects, w.dir.clone());
        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        let now = clock.clone();
        let mut pass =
            Pass::new(w.settings(Faults::default()), ports).with_clock(Box::new(move || now.get()));

        let report = pass.run(&mut w.conn).unwrap();
        assert!(report.heartbeat, "{report:?}");
        assert_eq!(
            *observer.seen.borrow(),
            10,
            "part 1 stops at the time budget"
        );
        assert!(duties.take_rows()[WORKER_CHECKS].is_empty());
        assert_eq!(w.count("SELECT count(*) FROM checks"), 0);

        pass.run(&mut w.conn).unwrap();
        assert_eq!(duties.take_rows()[WORKER_CHECKS].len(), 50);
        let (rows, workers): (i64, i64) = w
            .conn
            .query_row(
                "SELECT count(*), count(DISTINCT worker_id) FROM checks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((rows, workers), (50, 50));
    }

    /// PL-5: when the time budget of the pass is used, part 2 starts no
    /// new duty row, and the pass still writes its heartbeat. The next pass
    /// does the rows.
    #[test]
    fn a_pass_past_its_time_budget_starts_no_duty_row_and_writes_its_heartbeat() {
        let mut w = world();
        seed_big_run(&w, 50, 0, 0);
        let observer = Counting::default();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&observer, &NoHolder, &effects, w.dir.clone());
        let mut settings = w.settings(Faults::default());
        settings.config.pass_time_budget_s = 0;
        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        let report = Pass::new(settings, ports).run(&mut w.conn).unwrap();

        assert!(report.heartbeat, "{report:?}");
        assert!(duties.take_rows().values().all(Vec::is_empty));
        assert_eq!(w.count("SELECT count(*) FROM checks"), 0);

        pass(&mut w, &duties, Faults::default());
        assert_eq!(w.count("SELECT count(*) FROM checks"), 50);
    }

    /// PL-5: the time budget ends the held-task duty after 4 rows of a page
    /// of 6. The next pass starts at row 5, so 1 lap reaches each held task
    /// once, and each held task has its 1 question, once.
    #[test]
    fn a_held_page_cut_by_the_time_budget_goes_on_at_the_first_task_not_done() {
        const HELD: usize = 10;
        let w = world();
        seed_big_run(&w, 0, 0, HELD);
        let guard = w.settings(Faults::default()).generation;
        let facts = crate::fleetd::task_execution::domain::task::Facts::default();
        let duty = |rows: u64, cut_after: usize| -> Vec<String> {
            let up = std::cell::Cell::new(false);
            let done = std::cell::Cell::new(0);
            let time_up = || up.get();
            let ctx = DutyCtx {
                guard,
                clock: crate::fleetd::fleet_runtime::domain::clock::FleetSeconds(0),
                facts: &facts,
                budget: DutyBudget {
                    rows: PassBudget::new(rows),
                    time_up: &time_up,
                },
            };
            let tx = w.conn.unchecked_transaction().unwrap();
            let runs = watchdog::active_runs(&tx).unwrap();
            let keys = task_duty(
                &tx,
                &ctx,
                &runs,
                HELD_TASKS,
                watchdog::held_page,
                &|run, t| {
                    done.set(done.get() + 1);
                    up.set(done.get() == cut_after);
                    write(&tx, run, wakes::held_wake(&tx, t))
                },
            )
            .unwrap();
            tx.commit().unwrap();
            keys
        };
        let all: Vec<String> = watchdog::held_page(&w.conn, None, HELD)
            .unwrap()
            .into_iter()
            .map(|t| t.task_id)
            .collect();
        assert_eq!(all.len(), HELD);

        assert_eq!(
            duty(6, 4),
            all[..4],
            "the budget ends the page after 4 rows"
        );
        assert_eq!(duty(6, 0), all[4..], "the next pass starts at row 5");
        assert_eq!(wakes_with(&w, "is held: attempts"), HELD as i64);
        assert_eq!(duty(6, 0), all[..6], "the next lap starts at row 1");
        assert_eq!(wakes_with(&w, "is held: attempts"), HELD as i64);
    }
}
