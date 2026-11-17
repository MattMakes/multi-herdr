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
//!    due worker ([`watchdog::check_observed`]); the reap of a worker past
//!    its stall rounds ([`watchdog::reap_stalled`]), which ends its attempt
//!    `stalled` and returns the stops of its process and pane; the flag of
//!    an unclaimed spawn; the re-wake of startable tasks; the wake for
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
//! effect: the respawn from the attempt in `launching`, the stop of a
//! reaped worker from `mark_orphans`, and the wake from the outbox.
//!
//! The stall reap keeps the numbers of today (`check_every_s` and
//! `stall_rounds` of the run) until Phase 2 replaces it with leases.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use rusqlite::{Connection, Transaction};

use crate::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::application::pass::{ActCtx, DutyCtx, DutyReport, RunDuties};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, Recorded, RowRef, Stop,
};
use crate::taskledger::model::Actor;
use crate::taskledger::orchestrator::{self, Holder, Outcome, Probe, Respawn};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::watchdog::{self, Observation, Observer, Sighting};
use crate::taskledger::{outbox, util, workers};

/// The outcome of the attempt of a worker that the stall reap ends.
pub const STALLED: &str = "stalled";

/// The outcome of an orchestrator attempt whose agent is gone.
const GONE: &str = "gone";

/// The effects of the duties outside the ledger. The composition root of
/// `horch fleetd` gives them; a test gives fakes.
pub trait DutyEffects {
    /// Start the orchestrator of the respawn `r`
    /// (`crates/horch/src/cmd/recipes.rs`, `respawn_orchestrator`).
    fn respawn(&self, r: &Respawn) -> Result<()>;

    /// Deliver the outbox rows of `run` that are not delivered.
    fn deliver(&self, run: &Run);
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
    state: RefCell<State>,
}

impl<'a> WatchdogDuties<'a> {
    pub fn new(
        observer: &'a dyn Observer,
        probe: &'a dyn Probe,
        effects: &'a dyn DutyEffects,
    ) -> WatchdogDuties<'a> {
        WatchdogDuties {
            observer,
            probe,
            effects,
            state: RefCell::new(State::default()),
        }
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
    fn observe(&self, conn: &Connection) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.observed.clear();
        state.workspaces.clear();
        let runs = watchdog::active_runs(conn)?;
        let active: BTreeSet<String> = runs.iter().map(|r| r.run_id.clone()).collect();
        state.sightings.retain(|run, _| active.contains(run));
        for run in &runs {
            let now = util::now();
            let unclaimed = watchdog::unclaimed_spawns(conn, run, &now)?;
            let due = watchdog::due_workers(conn, run, false, &now)?;
            for w in due.iter().chain(unclaimed.iter().map(|u| &u.worker)) {
                let o = self.observer.observe(w);
                state.observed.insert(w.worker_id.clone(), o);
            }
            if let Some(ws) = orchestrator_workspace(conn, run)? {
                let seen = Seen {
                    holder: self.probe.holder(&ws),
                    open: self.probe.workspace_open(&ws),
                };
                state.workspaces.insert(ws, seen);
            }
        }
        Ok(())
    }

    fn run(&self, tx: &Transaction<'_>, ctx: &DutyCtx<'_>) -> Result<Vec<Stop>> {
        let mut state = self.state.borrow_mut();
        let mut reaped = BTreeSet::new();
        for run in watchdog::active_runs(tx)? {
            let now = util::now();
            reaped.extend(check_workers(tx, &run, &state.observed, &now)?);
            flag_unclaimed(tx, &run, &state.observed, &now)?;
            let seen = state.sightings.entry(run.run_id.clone()).or_default();
            if let Some(wake) = watchdog::startable_rewake(tx, &run, &now, seen)? {
                outbox::write(tx, &run.run_id, &wake)?;
            }
            wake_reviews_in(tx, &run, &now)?;
            let facts = Facts {
                seen: &state.workspaces,
                live: self.probe,
            };
            check_orchestrator(tx, ctx, &run, &now, &facts)?;
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

/// Check each due worker of `run` that part 1 observed. A worker past its
/// stall rounds is reaped; a first stall or a suspect gets its wake.
/// Returns the reaped workers.
fn check_workers(
    tx: &Transaction<'_>,
    run: &Run,
    observed: &BTreeMap<String, Observation>,
    now: &str,
) -> Result<Vec<String>> {
    let mut reaped = Vec::new();
    for w in watchdog::due_workers(tx, run, false, now)? {
        let Some(o) = observed.get(&w.worker_id) else {
            continue;
        };
        let c = watchdog::check_observed(tx, run, &w, o, now)?;
        let wake = if c.verdict == "dead" {
            let wake = watchdog::reap_stalled(tx, &c.worker_id, &c.note)?;
            end_attempts(tx, &c.worker_id, STALLED, &format!("stalled: {}", c.note))?;
            reaped.push(c.worker_id.clone());
            wake
        } else {
            watchdog::verdict_wake(&c)
        };
        if let Some(wake) = wake {
            outbox::write(tx, &run.run_id, &wake)?;
        }
    }
    Ok(reaped)
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

/// Flag each spawned worker of `run` with no claim, once.
fn flag_unclaimed(
    tx: &Transaction<'_>,
    run: &Run,
    observed: &BTreeMap<String, Observation>,
    now: &str,
) -> Result<()> {
    for u in watchdog::unclaimed_spawns(tx, run, now)? {
        let status = observed
            .get(&u.worker.worker_id)
            .and_then(|o| o.herdr_status.clone());
        if let Some(wake) = watchdog::flag_unclaimed(tx, run, &u, status.as_deref())? {
            outbox::write(tx, &run.run_id, &wake)?;
        }
    }
    Ok(())
}

/// The wake of each review round of `run` that is in while its author
/// waits.
fn wake_reviews_in(tx: &Transaction<'_>, run: &Run, now: &str) -> Result<()> {
    for r in watchdog::reviews_waiting(tx, run, now)? {
        let role = r
            .author
            .role
            .clone()
            .unwrap_or_else(|| r.author.label().to_string());
        let wake = watchdog::reviews_in_wake(tx, &r.parent, &role)?;
        outbox::write(tx, &run.run_id, &wake)?;
    }
    Ok(())
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
            watchdog::inherit_harness(tx, &r)?;
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
    use crate::taskledger::model::{OutboxEvent, WorkerKind};
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
        fn panes(&self) -> Option<BTreeMap<String, Option<Recorded>>> {
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
    }

    impl Record {
        fn new(w: &World) -> Record {
            Record {
                dir: w.dir.clone(),
                delivered: RefCell::new(0),
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
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);

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
    fn pass_reaps_a_worker_past_its_stall_rounds_and_requeues_its_task() {
        let mut w = world();
        let mut agent = Proc::sleep();
        let (task, worker) = w.stalled_task(agent.recorded());
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);

        let report = pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "incomplete");
        assert_eq!(w.status("tasks", "task_id", &task), "queued");
        let outcome: String = w
            .conn
            .query_row(
                "SELECT outcome FROM attempts WHERE worker_id = ?1",
                [&worker],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome, STALLED);
        assert_eq!(w.outbox(OutboxEvent::Reaped), 1);
        let pid = agent.0.id();
        assert!(
            report.done.contains(&Done::Stop(format!("process {pid}"))),
            "{report:?}"
        );
        assert!(agent.wait_exit(), "the stop of part 3 ends the agent");
        let reaps = w.count("SELECT count(*) FROM events WHERE kind = 'reaped'");
        assert_eq!(reaps, 1);
    }

    #[test]
    fn pass_writes_a_launching_attempt_for_a_gone_orchestrator_and_respawns_it_after_the_commit() {
        let mut w = world();
        let old = w.gone_orchestrator();
        let effects = Record::new(&w);
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);

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
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);
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
        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);
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
        let mut agent = Proc::sleep();
        let (task, worker) = w.stalled_task(agent.recorded());
        w.gone_orchestrator();

        crash_after_commit(&w);

        assert_eq!(w.status("workers", "worker_id", &worker), "incomplete");
        assert_eq!(w.status("tasks", "task_id", &task), "queued");
        let attempts = w.respawn_attempts();
        assert_eq!(attempts.len(), 1, "{attempts:?}");
        assert_eq!(attempts[0].1, "launching");
        let effects = Record::new(&w);
        assert!(
            effects.respawns().is_empty(),
            "the crash came before part 3"
        );
        assert!(
            matches!(agent.0.try_wait(), Ok(None)),
            "the crash came before the stop"
        );

        let duties = WatchdogDuties::new(&Quiet, &NoHolder, &effects);
        pass(&mut w, &duties, Faults::default());

        assert!(agent.wait_exit(), "the next pass stops the reaped agent");
        assert_eq!(
            effects.respawns(),
            vec![format!("{} launching", attempts[0].0)]
        );
        assert_eq!(w.outbox(OutboxEvent::Reaped), 1, "1 reap wake");
        assert_eq!(*effects.delivered.borrow(), 1, "the next pass delivers it");
        pass(&mut w, &duties, Faults::default());
        assert_eq!(effects.respawns().len(), 1, "1 respawn for 1 attempt");
    }
}
