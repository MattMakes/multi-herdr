use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, Transaction};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::application::pass::{
    ActCtx, DutyBudget, DutyCtx, DutyReport, RunDuties, IDEAS_DIR,
};
use crate::fleetd::fleet_runtime::domain::budget::{Cursor, PassBudget};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::answers::live_owner;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::ideas::application::sub_orchestrators;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::task::{AttemptRow, AttemptState, RowRef, Stop};
use crate::taskledger::model::{Actor, EventKind, WorkerStatus};
use crate::taskledger::runs::Run;
use crate::taskledger::wakes::{self, Item, Routed};
use crate::taskledger::watchdog::{self, Checked, Observation, Observer};
use crate::taskledger::workers::{self, Worker};
use crate::taskledger::{tasks, util};

const GONE: &str = "gone";

const RETIRED: &str = "no respawn: there is no long-lived orchestrator conversation";

pub const WORKER_CHECKS: &str = "duty_worker_checks";
pub const PRECLAIM: &str = "duty_preclaim";
pub const BLOCKER_TASKS: &str = "duty_blocker_tasks";
pub const HELD_TASKS: &str = "duty_held_tasks";

const BEFORE_CLAIM: [WorkerStatus; 3] = [
    WorkerStatus::Spawning,
    WorkerStatus::Starting,
    WorkerStatus::Claiming,
];

struct Page {
    workers: Vec<Worker>,
    start: Cursor,
}

pub trait DutyEffects {
    fn deliver(&self, run: &Run);

    fn retry_cleanups(&self) -> Result<Vec<String>>;
}

#[derive(Default)]
struct State {
    observed: BTreeMap<String, Observation>,
    pages: BTreeMap<&'static str, Page>,
}

pub struct WatchdogDuties<'a> {
    observer: &'a dyn Observer,
    effects: &'a dyn DutyEffects,
    state: RefCell<State>,
    force: bool,
    checked: RefCell<Vec<(String, Checked)>>,
    rows: RefCell<BTreeMap<&'static str, Vec<String>>>,
}

impl<'a> WatchdogDuties<'a> {
    pub fn new(observer: &'a dyn Observer, effects: &'a dyn DutyEffects) -> WatchdogDuties<'a> {
        WatchdogDuties {
            observer,
            effects,
            state: RefCell::new(State::default()),
            force: false,
            checked: RefCell::new(Vec::new()),
            rows: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn forced(mut self) -> Self {
        self.force = true;
        self
    }

    pub fn take_checked(&self) -> Vec<(String, Checked)> {
        self.checked.take()
    }

    pub fn take_rows(&self) -> BTreeMap<&'static str, Vec<String>> {
        self.rows.take()
    }

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
            let Some(c) = check_worker(tx, ctx, run, &w.worker_id, observed, now, self.force)?
            else {
                continue;
            };
            checked.push((run.run_id.clone(), c));
        }
        cursors_store::save(tx, WORKER_CHECKS, &cursor)?;
        self.rows.borrow_mut().insert(WORKER_CHECKS, keys);
        Ok(())
    }

    fn preclaim_page(
        &self,
        tx: &Transaction<'_>,
        ctx: &DutyCtx<'_>,
        runs: &[Run],
        page: Page,
        observed: &BTreeMap<String, Observation>,
        now: &str,
    ) -> Result<Vec<String>> {
        let mut cursor = page.start;
        let mut keys = Vec::new();
        let mut ended: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
                    ended
                        .entry(run.run_id.clone())
                        .or_default()
                        .push(p.worker.worker_id);
                }
            }
            flag_unclaimed(tx, run, &w.worker_id, observed, now)?;
        }
        let out = ended.into_values().flatten().collect();
        cursors_store::save(tx, PRECLAIM, &cursor)?;
        self.rows.borrow_mut().insert(PRECLAIM, keys);
        Ok(out)
    }
}

fn is_respawn(a: &AttemptRow) -> bool {
    respawn_number(a).is_some()
}

fn respawn_number(a: &AttemptRow) -> Option<i64> {
    if a.task_id.is_some() {
        return None;
    }
    let (worker, n) = a.id.rsplit_once("/R")?;
    (worker == a.worker_id).then(|| n.parse().ok()).flatten()
}

impl RunDuties for WatchdogDuties<'_> {
    fn observe(&self, conn: &Connection, budget: DutyBudget<'_>) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        state.observed.clear();
        state.pages.clear();
        let runs = watchdog::active_runs(conn)?;
        let now = util::now();
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
        crate::taskledger::import::sync_native(tx)?;
        let mut reaped = BTreeSet::new();
        if let Some(page) = state.pages.remove(WORKER_CHECKS) {
            self.check_page(tx, ctx, &runs, page, &state.observed, &now)?;
        }
        if let Some(page) = state.pages.remove(PRECLAIM) {
            reaped.extend(self.preclaim_page(tx, ctx, &runs, page, &state.observed, &now)?);
        }
        for run in runs.iter().take_while(|_| !time_up()) {
            run_items(tx, run)?;
        }
        let blockers = task_duty(
            tx,
            ctx,
            &runs,
            BLOCKER_TASKS,
            watchdog::blocker_page,
            &|_, t| {
                if watchdog::is_stalled_blocker(tx, t)? {
                    open_item(tx, wakes::blocker_item(tx, &t.task_id)?)?;
                }
                Ok(())
            },
        )?;
        let held = task_duty(tx, ctx, &runs, HELD_TASKS, watchdog::held_page, &|_, t| {
            open_item(tx, wakes::held_item(tx, t)?)
        })?;
        let mut rows = self.rows.borrow_mut();
        rows.insert(BLOCKER_TASKS, blockers);
        rows.insert(HELD_TASKS, held);
        for run in runs.iter().take_while(|_| !time_up()) {
            end_respawns(tx, run)?;
        }
        reaped_stops(tx, &reaped, ctx)
    }

    fn launches(&self, a: &AttemptRow) -> bool {
        is_respawn(a)
    }

    fn act(&self, conn: &Connection, _ctx: &ActCtx<'_>) -> DutyReport {
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
            self.effects.deliver(run);
        }
        report
    }
}

fn check_worker(
    tx: &Transaction<'_>,
    ctx: &DutyCtx<'_>,
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
    route(tx, ctx, wakes::verdict_doorbell(tx, &c)?)?;
    Ok(Some(c))
}

fn end_attempt(tx: &Connection, id: &str, outcome: &str, text: &str) -> Result<()> {
    attempts_store::set_ended(tx, id, outcome, &util::now())?;
    let by = Cause {
        actor: Actor::Watchdog,
        text,
    };
    let to = AttemptState::Ended.as_str();
    transition(tx, None, Entity::Attempt, id, to, by, &NeedsYouList)?;
    Ok(())
}

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

fn run_of<'r>(runs: &'r [Run], w: &Worker) -> Option<&'r Run> {
    runs.iter()
        .find(|r| w.run_id.as_deref() == Some(r.run_id.as_str()))
}

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

fn run_items(tx: &Transaction<'_>, run: &Run) -> Result<()> {
    open_item(tx, wakes::stuck_item(tx, run)?)?;
    open_item(tx, wakes::settled_item(tx, run)?)
}

fn open_item(tx: &Connection, item: Option<Item>) -> Result<()> {
    match item {
        Some(i) => i.open(tx),
        None => Ok(()),
    }
}

fn route(tx: &Connection, ctx: &DutyCtx<'_>, routed: Option<Routed>) -> Result<()> {
    match routed {
        None => Ok(()),
        Some(Routed::NeedsYou(i)) => i.open(tx),
        Some(Routed::Doorbell { feature_id, text }) => {
            let Some((attempt, _pane)) = live_owner(tx, &feature_id)? else {
                return Ok(());
            };
            let by = Cause {
                actor: Actor::Watchdog,
                text: &text,
            };
            let data = json!({ "fleet_clock": ctx.clock.0, "feature": feature_id });
            let guard = Some(ctx.guard);
            record(
                tx,
                guard,
                EventKind::Doorbell,
                Entity::Attempt,
                &attempt,
                by,
                &data,
            )?;
            Ok(())
        }
    }
}

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
    watchdog::flag_unclaimed(tx, run, &u, status.as_deref())?;
    Ok(())
}

fn waits_for_claim(w: &Worker, now: &str) -> bool {
    w.spawned_at
        .as_deref()
        .or(w.started_at.as_deref())
        .and_then(|t| util::seconds_between(t, now))
        .is_some_and(|s| s >= watchdog::UNCLAIMED_AFTER_S)
}

fn end_respawns(tx: &Transaction<'_>, run: &Run) -> Result<()> {
    let Some(orch) = run.orchestrator_worker_id.as_deref() else {
        return Ok(());
    };
    for a in attempts_store::attempts_of_worker(tx, orch)? {
        if a.state == AttemptState::Launching && is_respawn(&a) {
            end_attempt(tx, &a.id, GONE, RETIRED)?;
        }
    }
    Ok(())
}

pub struct HerdrObserver {
    pub herdr: crate::workspace::PaneHost,
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

fn agent_alive(w: &Worker) -> bool {
    match (w.agent_pid, w.agent_started) {
        (Some(p), Some(s)) => crate::procid::is_same(p as u32, s as u64),
        (Some(p), None) => crate::procid::alive(p as u32, None),
        _ => false,
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaLeadLaunch {
    pub attempt: String,
    pub worker_id: String,
    pub persona: String,
    pub idea: String,
    pub phase: String,
    pub folder: PathBuf,
    pub brief: String,
    pub state: String,
    pub run_id: String,
    pub run_slug: String,
    pub project_root: String,
    pub pane: Option<String>,
    pub workspace_id: Option<String>,
}

pub fn idea_lead_launch(
    conn: &Connection,
    fleet_root: &Path,
    attempt: &str,
) -> Result<Option<IdeaLeadLaunch>> {
    let Some((task, state, _, persona)) = attempts_store::attempt_via(conn, attempt)? else {
        return Ok(None);
    };
    if task.is_some() || persona.as_deref() != Some(sub_orchestrators::PERSONA) {
        return Ok(None);
    }
    let Some((_, Some(idea))) = attempts_store::task_and_idea(conn, attempt)? else {
        return Ok(None);
    };
    let Some(phase) = phase_of_attempt(attempt, &idea) else {
        return Ok(None);
    };
    let Some(feature) = ideas_store::idea(conn, &idea)?.map(|i| i.feature_id) else {
        return Ok(None);
    };
    let Some(row) = attempts_store::idea_attempts(conn, &idea)?
        .into_iter()
        .map(|(a, _)| a)
        .find(|a| a.id == attempt)
    else {
        return Ok(None);
    };
    let Some(run_id) = workers::get(conn, &row.worker_id)?.run_id else {
        return Ok(None);
    };
    let run = crate::taskledger::runs::get(conn, &run_id)?;
    let folder = fleet_root.join(IDEAS_DIR).join(&idea);
    Ok(Some(IdeaLeadLaunch {
        attempt: attempt.to_string(),
        worker_id: row.worker_id,
        persona: sub_orchestrators::PERSONA.to_string(),
        brief: idea_lead_brief(&idea, &feature, &phase, &folder),
        idea,
        phase,
        folder,
        state,
        run_id,
        run_slug: run.slug,
        project_root: run.project_root,
        pane: row.pane,
        workspace_id: run.workspace_id,
    }))
}

fn phase_of_attempt(attempt: &str, idea: &str) -> Option<String> {
    let rest = attempt.strip_prefix(idea)?.strip_prefix('/')?;
    let (phase, n) = rest.split_once('/')?;
    n.parse::<i64>().ok()?;
    Some(phase.to_string())
}

fn idea_lead_brief(idea: &str, feature: &str, phase: &str, folder: &Path) -> String {
    format!(
        "You are the idea-lead of idea {idea} in phase {phase}. \
         The feature of your idea is {feature}. \
         Your idea folder is {} and it is your only memory. \
         Read idea.md in that folder, then do the {phase} phase.",
        folder.display()
    )
}

#[cfg(unix)]
#[cfg(test)]
mod tests {

    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use rusqlite::{params, Connection};

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::scans::FolderItem;
    use crate::fleetd::fleet_runtime::application::pass::{
        Done, Outside, Pass, PassReport, Ports, Settings,
    };
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::fleet_runtime::domain::health::Hold;
    use crate::fleetd::fleet_runtime::domain::ledger::Generation;
    use crate::fleetd::task_execution::domain::task::Recorded;
    use crate::fleetd::worker_profiles::application::ports::{
        LaunchError, LaunchSpec, Launched, ProcessHost,
    };
    use crate::runtime::fault::Faults;
    use crate::taskledger::model::{OutboxEvent, TaskKind, WorkerKind, WorktreeMode};
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::{outbox, schema, util};

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
        crate::taskledger::schema::run_idea(
            &conn,
            &run.run_id,
            &crate::taskledger::util::now(),
            true,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("fleet")).unwrap();
        World {
            dir,
            _temp: Some(temp),
            conn,
            run,
        }
    }

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

        fn stalled_task(&self, agent: Recorded) -> (String, String) {
            let mut new = NewTask::work("a", "tickets/a.md", "dev");
            new.worktree_mode = WorktreeMode::Shared;
            let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
            let w = NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "dev".into(),
                workspace_id: Some("w1".into()),
                ..Default::default()
            };
            let worker =
                crate::taskledger::testing::bind_live(&self.conn, &task.task_id, &w, Some(agent))
                    .unwrap()
                    .worker_id;
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

    struct NoHost;

    impl ProcessHost for NoHost {
        fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
            panic!("the pass launched {:?}", spec.program)
        }
    }

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

    #[derive(Default)]
    struct Record {
        delivered: RefCell<usize>,
        cleanups: RefCell<usize>,
    }

    impl DutyEffects for Record {
        fn deliver(&self, _run: &Run) {
            *self.delivered.borrow_mut() += 1;
        }

        fn retry_cleanups(&self) -> Result<Vec<String>> {
            *self.cleanups.borrow_mut() += 1;
            Ok(Vec::new())
        }
    }

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

        fn dead() -> Recorded {
            let mut p = Proc::sleep();
            let r = p.recorded();
            p.0.kill().unwrap();
            p.0.wait().unwrap();
            r
        }

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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        let flags = w.count("SELECT count(*) FROM events WHERE text LIKE 'No claim:%'");
        assert_eq!(flags, 1, "1 flag for 1 unclaimed spawn");
        assert_eq!(w.outbox(OutboxEvent::UnclaimedSpawn), 0);
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

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
    fn pass_respawns_no_gone_orchestrator() {
        let mut w = world();
        let old = w.gone_orchestrator();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        for _ in 0..3 {
            let report = pass(&mut w, &duties, Faults::default());
            assert!(report.failed.is_empty(), "{report:?}");
        }

        let run = runs::get(&w.conn, &w.run.run_id).unwrap();
        assert_eq!(run.orchestrator_worker_id.as_deref(), Some(old.as_str()));
        assert!(w.respawn_attempts().is_empty(), "no respawn attempt");
        assert_eq!(
            w.count("SELECT count(*) FROM workers WHERE kind = 'orchestrator'"),
            1
        );
        let guard = crate::taskledger::orchestrator::guard(&w.conn, &w.run.run_id).unwrap();
        assert_eq!(guard.respawns, 0, "no respawn is counted");
    }

    #[test]
    fn pass_ends_a_respawn_attempt_of_an_older_fleetd_and_launches_nothing() {
        let mut w = world();
        let orch = w.gone_orchestrator();
        let id = format!("{orch}/R1");
        let a = AttemptRow {
            id: id.clone(),
            task_id: None,
            worker_id: orch,
            state: AttemptState::Launching,
            process: None,
            pane: None,
        };
        attempts_store::insert_attempt(&w.conn, &a, &util::now()).unwrap();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);
        assert!(duties.launches(&a), "the pass does not launch it");

        let report = pass(&mut w, &duties, Faults::default());

        assert!(report.failed.is_empty(), "{report:?}");
        assert_eq!(w.respawn_attempts(), vec![(id, "ended".into())]);
    }

    #[test]
    fn pass_writes_no_startable_rewake() {
        let mut w = world();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);
        let startable = |w: &World, title: &str| {
            let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
            tasks::add(&w.conn, &w.run, &new, Actor::Orchestrator).unwrap();
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
        pass(&mut w, &duties, Faults::default());
        startable(&w, "b");
        pass(&mut w, &duties, Faults::default());
        assert_eq!(rewakes(&w), 0);
    }

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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "no_task");
        assert_eq!(
            wakes_with(&w, "ended before they claimed a task"),
            0,
            "the wake goes to fleetd (FM-16)"
        );
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.status("workers", "worker_id", &worker), "claiming");
        let flags = w.count("SELECT count(*) FROM events WHERE text LIKE 'No claim:%'");
        assert_eq!(flags, 1, "1 flag");
        assert_eq!(w.outbox(OutboxEvent::UnclaimedSpawn), 0, "no wake (FM-16)");
    }

    #[test]
    fn pass_opens_1_item_for_a_stuck_run_and_1_for_a_held_task() {
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.items("stuck").len(), 1, "1 stuck item");
        let held = w.items("held");
        assert_eq!(held.len(), 1, "1 held item");
        assert!(held[0].contains("3 attempts failed"), "{held:?}");
        assert_eq!(wakes_with(&w, "the run is stuck"), 0);
        assert_eq!(wakes_with(&w, "is held: 3 attempts failed"), 0);
    }

    #[test]
    fn pass_opens_1_item_for_a_settled_run_of_a_ready_feature() {
        let mut w = world();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        assert!(
            w.items("feature_ready").is_empty(),
            "the feature of the run is not ready"
        );

        w.conn
            .execute(
                "UPDATE features SET state = 'ready'
                 WHERE id = (SELECT printf('F-%04d', rowid) FROM runs WHERE run_id = ?1)",
                [&w.run.run_id],
            )
            .unwrap();
        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.items("feature_ready").len(), 1);
        assert_eq!(w.outbox(OutboxEvent::Settled), 0);
    }

    #[test]
    fn pass_opens_1_item_for_a_cancelled_task_that_open_tasks_wait_for() {
        let mut w = world();
        let first = NewTask::work("a", "tickets/a.md", "dev");
        let first = tasks::add(&w.conn, &w.run, &first, Actor::Orchestrator).unwrap();
        let mut second = NewTask::work("b", "tickets/b.md", "dev");
        second.blocked_by = vec![first.task_id.clone()];
        tasks::add(&w.conn, &w.run, &second, Actor::Orchestrator).unwrap();
        tasks::cancel(&w.conn, &first, "drop").unwrap();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.items("blocker").len(), 1);
        assert_eq!(w.outbox(OutboxEvent::Cancelled), 0);
    }

    #[test]
    fn pass_rings_the_live_sub_orchestrator_of_the_feature_on_the_first_stall() {
        let mut w = world();
        let agent = Proc::sleep();
        let (task, worker) = w.stalled_task(agent.recorded());
        w.conn
            .execute(
                "UPDATE runs SET stall_rounds = 3 WHERE run_id = ?1",
                [&w.run.run_id],
            )
            .unwrap();
        let lead = Proc::sleep();
        let attempt = w.sub_orchestrator(&task, lead.recorded());
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.doorbells(&attempt), vec!["shows no progress".to_string()]);
        assert!(w.items("stall").is_empty());
        assert_eq!(wakes_with(&w, "shows no progress"), 0);
        assert_eq!(w.status("workers", "worker_id", &worker), "working");
        assert_eq!(w.status("tasks", "task_id", &task), "running");
    }

    #[test]
    fn pass_writes_nothing_on_a_first_stall_when_no_sub_orchestrator_runs() {
        let mut w = world();
        let agent = Proc::sleep();
        let (task, _worker) = w.stalled_task(agent.recorded());
        w.conn
            .execute(
                "UPDATE runs SET stall_rounds = 3 WHERE run_id = ?1",
                [&w.run.run_id],
            )
            .unwrap();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());

        assert!(w.items("stall").is_empty());
        assert_eq!(wakes_with(&w, "shows no progress"), 0);
        let rung = w.count("SELECT count(*) FROM events WHERE kind = 'doorbell'");
        assert_eq!(rung, 0, "no doorbell of task {task}");
    }

    #[test]
    fn pass_writes_no_reviews_in_wake() {
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(w.outbox(OutboxEvent::ReviewsIn), 0);
    }

    #[test]
    fn pass_retries_the_worktree_cleanups_once_each_pass() {
        let mut w = world();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        assert_eq!(*effects.cleanups.borrow(), 2);
    }

    impl World {
        fn sub_orchestrator(&self, task: &str, process: Recorded) -> String {
            let idea = attempts_store::idea_of_task(&self.conn, task)
                .unwrap()
                .expect("the task has an idea");
            let lead = self.worker(WorkerKind::Worker, "working", Some(process));
            let a = AttemptRow {
                id: format!("{lead}/lead"),
                task_id: None,
                worker_id: lead,
                state: AttemptState::Live,
                process: Some(process),
                pane: Some("p9".into()),
            };
            attempts_store::insert_attempt(&self.conn, &a, &util::now()).unwrap();
            attempts_store::set_idea(&self.conn, &a.id, &idea).unwrap();
            a.id
        }

        fn doorbells(&self, attempt: &str) -> Vec<String> {
            let mut st = self
                .conn
                .prepare(
                    "SELECT text FROM events WHERE kind = 'doorbell' AND entity_id = ?1
                     ORDER BY event_id",
                )
                .unwrap();
            st.query_map([attempt], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| {
                    let t = r.unwrap();
                    if t.contains("shows no progress") {
                        "shows no progress".to_string()
                    } else {
                        t
                    }
                })
                .collect()
        }

        fn items(&self, kind: &str) -> Vec<String> {
            let mut st = self
                .conn
                .prepare(
                    "SELECT text FROM needs_you WHERE kind = ?1 AND state = 'open' ORDER BY key",
                )
                .unwrap();
            st.query_map([kind], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        }

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

    #[test]
    fn pass_opens_a_lost_parked_blocker_item_once() {
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&Quiet, &effects);

        pass(&mut w, &duties, Faults::default());
        pass(&mut w, &duties, Faults::default());

        let items = w.items("blocker");
        assert_eq!(items.len(), 1, "{items:?}");
        assert!(
            items[0].contains("T-2 waits: T-1 is parked unlanded"),
            "{items:?}"
        );
        assert!(w.rows(OutboxEvent::Held).is_empty());
    }

    #[derive(Default)]
    struct Counting(RefCell<BTreeMap<String, usize>>);

    impl Observer for Counting {
        fn observe(&self, w: &workers::Worker) -> Observation {
            *self.0.borrow_mut().entry(w.worker_id.clone()).or_default() += 1;
            Quiet.observe(w)
        }
    }

    fn seed_big_run(w: &World, working: usize, claiming: usize, held: usize) {
        let pid = std::process::id();
        let start = crate::procid::start_time(pid).expect("the start time of this process") as i64;
        let now = util::now();
        let hour = ago(3600);
        let host = util::host_name();
        let run = &w.run.run_id;
        let tx = w.conn.unchecked_transaction().unwrap();
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
            let later = ago(-86_400);
            for n in 1..=claiming {
                let wk = format!("BC-{n:05}");
                let none: Option<&str> = None;
                let no_attempt: Option<i64> = None;
                worker
                    .execute(params![
                        wk, run, host, pid, start, none, no_attempt, "claiming", later, none, later
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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&observer, &effects);
        let mut settings = w.settings(Faults::default());
        settings.config.pass_budget = BUDGET as u64;
        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
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
        for (duty, n) in [(PRECLAIM, CLAIMING), (HELD_TASKS, HELD)] {
            let keys = &reached[duty];
            assert_eq!(keys.len(), n, "{duty}");
            let fair = (PASSES * BUDGET / n)..=(PASSES * BUDGET).div_ceil(n);
            assert!(keys.values().all(|c| fair.contains(c)), "{duty}: {keys:?}");
        }
        assert_eq!(w.items("held").len(), HELD);
    }

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
        let effects = Record::default();
        let duties = WatchdogDuties::new(&observer, &effects);
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

    #[test]
    fn a_pass_past_its_time_budget_starts_no_duty_row_and_writes_its_heartbeat() {
        let mut w = world();
        seed_big_run(&w, 50, 0, 0);
        let observer = Counting::default();
        let effects = Record::default();
        let duties = WatchdogDuties::new(&observer, &effects);
        let mut settings = w.settings(Faults::default());
        settings.config.pass_time_budget_s = 0;
        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        let start = Instant::now();
        let report = Pass::new(settings, ports)
            .with_clock(Box::new(move || start))
            .run(&mut w.conn)
            .unwrap();

        assert!(report.heartbeat, "{report:?}");
        assert!(duties.take_rows().values().all(Vec::is_empty));
        assert_eq!(w.count("SELECT count(*) FROM checks"), 0);

        let ports = Ports {
            outside: &NoOutside,
            duties: &duties,
            needs_you: &NeedsYouList,
            host: &NoHost,
        };
        Pass::new(w.settings(Faults::default()), ports)
            .with_clock(Box::new(move || start))
            .run(&mut w.conn)
            .unwrap();
        assert_eq!(w.count("SELECT count(*) FROM checks"), 50);
    }

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
                &|_, t| {
                    done.set(done.get() + 1);
                    up.set(done.get() == cut_after);
                    open_item(&tx, wakes::held_item(&tx, t)?)
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
        assert_eq!(w.items("held").len(), HELD);
        assert_eq!(duty(6, 0), all[..6], "the next lap starts at row 1");
        assert_eq!(w.items("held").len(), HELD, "1 item for each held task");
    }
}

#[cfg(test)]
mod idea_lead_tests {

    use std::path::Path;

    use rusqlite::Connection;

    use super::*;
    use crate::taskledger::model::WorkerKind;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::schema;
    use crate::taskledger::workers::NewWorker;

    fn ledger(dir: &Path) -> (Connection, Run, String) {
        let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            workspace_id: Some("w1".into()),
            ..Default::default()
        };
        let run = runs::create(&conn, dir, &new_run).unwrap();
        let idea = schema::run_idea(&conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("the idea of the run");
        (conn, run, idea)
    }

    fn attempt(conn: &Connection, run: &Run, id: &str, idea: &str, persona: &str) {
        let worker_id = workers::insert(
            conn,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: persona.into(),
                ..NewWorker::default()
            },
        )
        .unwrap();
        let row = AttemptRow {
            id: id.into(),
            task_id: None,
            worker_id,
            state: AttemptState::Launching,
            process: None,
            pane: None,
        };
        attempts_store::insert_attempt(conn, &row, &util::now()).unwrap();
        attempts_store::set_idea(conn, id, idea).unwrap();
    }

    #[test]
    fn an_idea_lead_attempt_gives_its_persona_folder_and_brief() {
        let temp = tempfile::tempdir().unwrap();
        let (conn, run, idea) = ledger(temp.path());
        let id = format!("{idea}/clarifying/1");
        attempt(&conn, &run, &id, &idea, sub_orchestrators::PERSONA);
        let fleet = temp.path().join("fleet");

        let l = idea_lead_launch(&conn, &fleet, &id)
            .unwrap()
            .expect("an idea-lead attempt has a launch");

        let folder = fleet.join("ideas").join(&idea);
        assert_eq!(l.persona, "idea-lead");
        assert_eq!(l.idea, idea);
        assert_eq!(l.phase, "clarifying");
        assert_eq!(l.folder, folder);
        assert_eq!(l.state, "launching");
        assert_eq!(l.run_slug, "r1");
        assert_eq!(l.project_root, run.project_root);
        assert_eq!(l.workspace_id.as_deref(), Some("w1"));
        assert!(l.brief.contains(&idea), "{}", l.brief);
        assert!(l.brief.contains("phase clarifying"), "{}", l.brief);
        let feature: String = conn
            .query_row("SELECT feature_id FROM ideas WHERE id = ?1", [&idea], |r| {
                r.get(0)
            })
            .unwrap();
        let line = format!("The feature of your idea is {feature}.");
        assert!(l.brief.contains(&line), "{}", l.brief);
        let word = folder.display().to_string();
        assert!(l.brief.split_whitespace().any(|w| w == word), "{}", l.brief);
    }

    #[test]
    fn an_attempt_with_no_task_of_another_persona_is_no_idea_lead() {
        let temp = tempfile::tempdir().unwrap();
        let (conn, run, idea) = ledger(temp.path());
        let id = format!("{idea}/clarifying/1");
        attempt(&conn, &run, &id, &idea, "orchestrator");
        let fleet = temp.path().join("fleet");

        assert_eq!(idea_lead_launch(&conn, &fleet, &id).unwrap(), None);
        assert_eq!(idea_lead_launch(&conn, &fleet, "no-such").unwrap(), None);
    }
}
