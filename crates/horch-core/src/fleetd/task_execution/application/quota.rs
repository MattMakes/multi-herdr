//! The use case that moves a task on a usage limit (RR-11, FM-5): the
//! rows of [`exit`](crate::fleetd::task_execution::domain::quota::exit)
//! and [`released`](crate::fleetd::task_execution::domain::quota::released),
//! and the restart on the fallback after the commit.
//!
//! - [`on_quota`]: the pass calls it for each attempt that ended with the
//!   outcome `quota` (the stall ladder stopped its process). The attempt
//!   burns nothing: `attempt_floor` goes up by 1, and `tasks.attempts`
//!   does not change. The task goes `running -> ready` for the fallback of
//!   another pool, with 1 new worker row and 1 attempt in `launching`, or
//!   `running -> waiting_quota` with its lease paused.
//! - [`release_reset_pools`]: the pass calls it for the tasks in
//!   `waiting_quota`. A task whose pool reset goes to `ready`.
//! - [`restart`]: part 3 calls it for each attempt that [`is_restart`]
//!   names. It spawns the fallback worker once, after each process of an
//!   older attempt of the task is gone.
//!
//! The Phase 2 bridge: fleetd writes the worker row first, and
//! `horch spawn` adopts it ([`ADOPT_WORKER_ENV`]). Phase 4 replaces this
//! with the push dispatch, which binds the task to a worker before the
//! pane opens.
//!
//! The pool reading comes only from the quota file or the probe result
//! in a [`QuotaView`] (`preservation-06`, row S13).

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::domain::quota::{exit, released, Exit, Reading, Window};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::{AttemptRow, AttemptState, Recorded};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, Launched, ProcessHost};
use crate::roster::Roster;
use crate::routing::quota::{model_family, pool_for, QuotaView};
use crate::taskledger::model::{Actor, TaskStatus, WorkerKind};
use crate::taskledger::workers::{self, NewWorker};
use crate::taskledger::{runs, tasks, util};

/// The cause of `running -> ready` on the fallback of another pool.
pub const USAGE_LIMIT_FALLBACK: &str = "usage_limit_fallback";
/// The cause of `running -> waiting_quota`.
pub const USAGE_LIMIT: &str = "usage_limit";
/// The cause of `waiting_quota -> ready`.
pub const POOL_RESET: &str = "pool_reset";

/// The variable that names the worker row that fleetd wrote for a restart.
/// `horch spawn` adopts that row and inserts none. fleetd passes it only
/// through the launcher. The Phase 2 bridge: Phase 4 replaces it with the
/// push dispatch.
pub const ADOPT_WORKER_ENV: &str = "HORCH_ADOPT_WORKER";

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The pool readings of the personas: the roster, the quota file or probe
/// result, and `pool_full_use` of `fleet.toml`.
pub struct Pools<'a> {
    pub roster: &'a Roster,
    pub quota: &'a QuotaView,
    pub full_use: f64,
}

impl Pools<'_> {
    /// The reading of the pool of `persona`, with the windows that limit
    /// its model. `None` for a persona that is not in the roster.
    pub fn reading(&self, persona: &str) -> Option<Reading> {
        let t = self.roster.get(persona)?;
        let agent = t.agent.as_str();
        let model = t.model.as_deref().unwrap_or_default();
        let pool = pool_for(agent, model);
        let scope = model_family(model);
        let windows = self
            .quota
            .file
            .pools
            .get(pool)
            .map(|p| {
                p.windows
                    .iter()
                    .filter(|w| w.scope_model.is_none() || w.scope_model.as_deref() == scope)
                    .map(|w| Window {
                        used: w.used,
                        reset_at: w
                            .resets_at
                            .as_deref()
                            .and_then(crate::clock::parse)
                            .map(|t| t.timestamp()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Reading {
            pool: pool.to_string(),
            blocks: self.quota.assess(agent, model).state.blocks(),
            windows,
        })
    }

    /// The calendar time of the readings, in Unix seconds.
    fn now(&self) -> i64 {
        self.quota.now.timestamp()
    }

    /// The fallback of a task of `persona`: the task's own `fallback`
    /// when it is set, else the first entry of `fallbacks` of the persona
    /// (`docs/tickets/fleetd-p2/MAP.md`, Q8). A persona with a
    /// `no_fallback_reason` never restarts on another persona.
    fn fallback(&self, persona: &str, of_task: Option<&str>) -> Option<String> {
        let t = self.roster.get(persona);
        if t.is_some_and(|t| t.no_fallback_reason.is_some()) {
            return None;
        }
        of_task
            .map(str::to_string)
            .or_else(|| t.and_then(|t| t.profile().fallback.clone()))
    }
}

/// Where [`on_quota`] moved a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moved {
    /// `ready` for the fallback, with the attempt `attempt` in `launching`
    /// on the new worker row `worker`. Part 3 calls [`restart`].
    Restart { attempt: String, worker: String },
    /// `waiting_quota`, with the lease paused.
    Wait,
}

/// The id of the attempt in `launching` of a restart of `task_id` after
/// `attempts` attempts. It differs from the id that a claim writes, so a
/// restart attempt that timed out never blocks the next claim.
fn restart_attempt_id(task_id: &str, attempts: i64) -> String {
    format!("{task_id}/A{}-fallback", attempts + 1)
}

/// The fields of 1 attempt that the quota rule reads.
struct Attempt {
    task_id: Option<String>,
    worker_id: String,
    state: String,
    outcome: Option<String>,
}

fn attempt(conn: &Connection, id: &str) -> Result<Attempt> {
    conn.query_row(
        "SELECT task_id, worker_id, state, outcome FROM attempts WHERE id = ?1",
        [id],
        |r| {
            Ok(Attempt {
                task_id: r.get(0)?,
                worker_id: r.get(1)?,
                state: r.get(2)?,
                outcome: r.get(3)?,
            })
        },
    )
    .optional()?
    .with_context(|| format!("no attempt {id}"))
}

/// `move-to-fallback` for the attempt `attempt_id`, which ended with the
/// outcome `quota`, in the open transaction `tx`. `None`: its task already
/// moved (it is not `running` with the worker of the attempt).
pub fn on_quota(
    tx: &Connection,
    guard: Option<Generation>,
    attempt_id: &str,
    pools: &Pools<'_>,
) -> Result<Option<Moved>> {
    if tx.is_autocommit() {
        bail!("the quota rule writes its rows in 1 open transaction of the ledger");
    }
    let a = attempt(tx, attempt_id)?;
    let quota = Outcome::Quota.as_str();
    if a.state != AttemptState::Ended.as_str() || a.outcome.as_deref() != Some(quota) {
        bail!("attempt {attempt_id} did not end with the outcome {quota}");
    }
    let Some(task_id) = a.task_id else {
        return Ok(None);
    };
    let task = tasks::get(tx, &task_id)?;
    if task.status != TaskStatus::Running.as_str()
        || task.current_worker_id.as_deref() != Some(a.worker_id.as_str())
    {
        return Ok(None);
    }
    let old = workers::get(tx, &a.worker_id)?;
    let persona = old.persona.clone().unwrap_or_default();
    let own = pools.reading(&persona);
    let own_pool = own.as_ref().map_or("", |r| r.pool.as_str());
    let fallback = pools.fallback(&persona, task.fallback_persona.as_deref());
    let candidate = fallback
        .as_deref()
        .and_then(|f| Some((f, pools.reading(f)?)));
    let decided = exit(
        own_pool,
        candidate.as_ref().map(|(f, r)| (*f, r)),
        pools.full_use,
        pools.now(),
    );
    // No attempt is burned: the next claim adds 1 to `attempts`, and the
    // floor moves with it.
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL,
            updated_at = ?2
         WHERE task_id = ?1",
        params![task_id, util::now()],
    )?;
    match decided {
        Exit::Restart { persona: fb } => {
            let to = TaskStatus::Ready.as_str();
            let why = cause(USAGE_LIMIT_FALLBACK);
            transition(tx, guard, Entity::Task, &task_id, to, why, &NeedsYouList)?;
            tx.execute(
                "UPDATE tasks SET claimable_by = ?2, fallback_reason = ?3, not_before = NULL
                 WHERE task_id = ?1",
                params![task_id, fb, format!("usage limit of pool {own_pool}")],
            )?;
            // A ready task holds no lease.
            lease::release(tx, &task_id)?;
            let worker = workers::insert(
                tx,
                &NewWorker {
                    run_id: task.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: fb,
                    workspace_id: old.workspace_id.clone(),
                    previous_worker_id: Some(old.worker_id.clone()),
                    target_task_id: Some(task_id.clone()),
                    ..Default::default()
                },
            )?;
            let row = AttemptRow {
                id: restart_attempt_id(&task_id, task.attempts),
                task_id: Some(task_id.clone()),
                worker_id: worker.clone(),
                state: AttemptState::Launching,
                process: None,
                pane: None,
            };
            store::insert_attempt(tx, &row, &util::now())?;
            created(
                tx,
                guard,
                Entity::Attempt,
                &row.id,
                cause(USAGE_LIMIT_FALLBACK),
            )?;
            Ok(Some(Moved::Restart {
                attempt: row.id,
                worker,
            }))
        }
        Exit::Wait => {
            // The lease stays, and `waiting_quota` pauses it.
            let to = TaskStatus::WaitingQuota.as_str();
            transition(
                tx,
                guard,
                Entity::Task,
                &task_id,
                to,
                cause(USAGE_LIMIT),
                &NeedsYouList,
            )?;
            Ok(Some(Moved::Wait))
        }
    }
}

/// Move each task in `waiting_quota` whose pool reset to `ready`, in the
/// open transaction `tx`, and delete its lease. The pool is the pool of
/// the persona that claims the task next (`claimable_by`). A persona that
/// is not in the roster has no pool to wait on. Returns the ids of the
/// moved tasks.
pub fn release_reset_pools(
    tx: &Connection,
    guard: Option<Generation>,
    pools: &Pools<'_>,
) -> Result<Vec<String>> {
    if tx.is_autocommit() {
        bail!("the quota rule writes its rows in 1 open transaction of the ledger");
    }
    let waiting: Vec<(String, Option<String>)> = {
        let mut stmt = tx.prepare(
            "SELECT task_id, claimable_by FROM tasks WHERE status = ?1 ORDER BY task_id",
        )?;
        let rows = stmt.query_map([TaskStatus::WaitingQuota.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut moved = Vec::new();
    for (task_id, persona) in waiting {
        let reading = persona.as_deref().and_then(|p| pools.reading(p));
        if reading.is_some_and(|r| !released(&r, pools.full_use, pools.now())) {
            continue;
        }
        let to = TaskStatus::Ready.as_str();
        transition(
            tx,
            guard,
            Entity::Task,
            &task_id,
            to,
            cause(POOL_RESET),
            &NeedsYouList,
        )?;
        lease::release(tx, &task_id)?;
        moved.push(task_id);
    }
    Ok(moved)
}

/// The attempt `a` is a restart on a fallback: an attempt of a task in
/// `launching`. Part 3 launches it with [`restart`], not with the launch
/// of an attempt of Phase 1.
pub fn is_restart(a: &AttemptRow) -> bool {
    a.state == AttemptState::Launching && a.task_id.is_some()
}

/// What `horch spawn` needs to start the fallback worker of a restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restart {
    /// The attempt in `launching`.
    pub attempt: String,
    pub task_id: String,
    /// The worker row that the spawn adopts ([`ADOPT_WORKER_ENV`]).
    pub worker_id: String,
    /// The fallback persona.
    pub persona: String,
    pub run_slug: String,
    pub project_root: String,
    /// The pane to split: the pane of the orchestrator of the run, else
    /// the pane of the worker that hit the limit.
    pub from_pane: Option<String>,
}

/// The effects of a restart outside the ledger.
pub trait Restarts {
    /// Whether the process `p` of an older attempt is still alive: its
    /// pid runs with its start time, or its process group has members.
    fn alive(&self, p: Recorded) -> bool {
        crate::procid::alive(p.pid, p.pid_start)
            || p.pid_start
                .is_some_and(|s| crate::procid::is_same_group(p.pid, s))
    }

    /// The launch of `horch spawn <persona>` for `r`.
    fn spec(&self, r: &Restart) -> Result<LaunchSpec>;
}

/// What [`restart`] did.
#[derive(Debug)]
pub enum Restarted {
    /// The spawn started. The caller reaps its process.
    Launched(Launched),
    /// The attempt is not in `launching`, or it has a launch record: the
    /// spawn ran once already.
    Done,
    /// The process `pid` of an older attempt of the task is alive: stops
    /// come before launches. A later pass tries again.
    Waiting { pid: u32 },
}

/// The processes of the attempts of `task_id` other than `attempt_id`. An
/// attempt with no process of its own has the agent of its worker.
fn older_processes(conn: &Connection, task_id: &str, attempt_id: &str) -> Result<Vec<Recorded>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(a.pid, w.agent_pid),
            CASE WHEN a.pid IS NULL THEN w.agent_started ELSE a.pid_start END
         FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.task_id = ?1 AND a.id <> ?2",
    )?;
    let rows = stmt.query_map(params![task_id, attempt_id], |r| {
        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (pid, start) = row?;
        if let Some(pid) = pid.and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0) {
            out.push(Recorded {
                pid,
                pid_start: start.and_then(|s| u64::try_from(s).ok()),
            });
        }
    }
    Ok(out)
}

/// The [`Restart`] of the attempt `attempt_id`, from its rows.
fn restart_of(
    conn: &Connection,
    attempt_id: &str,
    task_id: &str,
    worker_id: &str,
) -> Result<Restart> {
    let w = workers::get(conn, worker_id)?;
    let run = runs::get(conn, &task_id_run(conn, task_id)?)?;
    let orchestrator_pane = match &run.orchestrator_worker_id {
        Some(id) => workers::find(conn, id)?.and_then(|o| o.pane_id),
        None => None,
    };
    let previous_pane = match &w.previous_worker_id {
        Some(id) => workers::find(conn, id)?.and_then(|p| p.pane_id),
        None => None,
    };
    Ok(Restart {
        attempt: attempt_id.to_string(),
        task_id: task_id.to_string(),
        worker_id: w.worker_id.clone(),
        persona: w.persona.clone().unwrap_or_default(),
        run_slug: run.slug,
        project_root: run.project_root,
        from_pane: orchestrator_pane.or(previous_pane),
    })
}

fn task_id_run(conn: &Connection, task_id: &str) -> Result<String> {
    Ok(tasks::get(conn, task_id)?.run_id)
}

/// Part 3: spawn the fallback worker of the restart attempt `attempt_id`
/// at fleet clock `clock`, after the commit of its rows. The launch record
/// under `fleet_root` is written before the spawn, so the spawn runs at
/// most once. The record names no process: the spawn is not the agent,
/// and the attempt goes `live` when the agent of its worker runs or
/// claims. A spawn that fails ends its worker row `launch_failed`, and the
/// start timeout ends the attempt.
pub fn restart(
    conn: &Connection,
    fleet_root: &Path,
    clock: FleetSeconds,
    attempt_id: &str,
    host: &dyn ProcessHost,
    ports: &dyn Restarts,
) -> Result<Restarted> {
    let a = attempt(conn, attempt_id)?;
    let Some(task_id) = a.task_id else {
        return Ok(Restarted::Done);
    };
    if a.state != AttemptState::Launching.as_str()
        || effects::read_launch(fleet_root, attempt_id)?.is_some()
    {
        return Ok(Restarted::Done);
    }
    for p in older_processes(conn, &task_id, attempt_id)? {
        if ports.alive(p) {
            return Ok(Restarted::Waiting { pid: p.pid });
        }
    }
    let r = restart_of(conn, attempt_id, &task_id, &a.worker_id)?;
    let record = LaunchRecord {
        attempt: attempt_id.to_string(),
        launched_at: clock,
        process: None,
    };
    effects::write_launch(fleet_root, &record)?;
    let launched = ports
        .spec(&r)
        .and_then(|spec| host.launch(&spec).map_err(anyhow::Error::from));
    match launched {
        Ok(l) => Ok(Restarted::Launched(l)),
        Err(e) => {
            let reason = format!("the restart on fallback {} did not start: {e:#}", r.persona);
            workers::launch_failed(conn, &r.worker_id, &reason)?;
            Err(e.context(format!("restart attempt {attempt_id}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::fleetd::task_execution::application::commands;
    use crate::fleetd::task_execution::domain::lease::Limits;
    use crate::fleetd::task_execution::domain::task::{attempt_id, Reap};
    use crate::harness::HarnessKind;
    use crate::roster::Teammate;
    use crate::routing::policy::Policy;
    use crate::routing::quota::{PoolReading, QuotaFile, Window as QuotaWindow};
    use crate::taskledger::model::WorkerStatus;
    use crate::taskledger::runs::NewRun;
    use crate::taskledger::tasks::{ClaimOutcome, NewTask};
    use crate::taskledger::Store;

    const FULL: f64 = 0.98;
    const AGENT: Recorded = Recorded {
        pid: 4_242,
        pid_start: Some(77),
    };

    fn now() -> DateTime<Utc> {
        crate::clock::parse("2026-11-17T12:00:00Z").unwrap()
    }

    fn teammate(name: &str, agent: HarnessKind, model: &str, fallback: Option<&str>) -> Teammate {
        Teammate {
            name: name.to_string(),
            agent,
            model: Some(model.to_string()),
            fallbacks: fallback.map(str::to_string).into_iter().collect(),
            ..Default::default()
        }
    }

    /// `dev` on claude with the fallback `alt` on codex; `same` on claude;
    /// `pinned` on claude with a reason for no fallback.
    fn roster() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(teammate(
            "dev",
            HarnessKind::Claude,
            "claude-opus-5-5",
            Some("alt"),
        ));
        r.insert_for_test(teammate("alt", HarnessKind::Codex, "gpt-5.5", None));
        r.insert_for_test(teammate(
            "same",
            HarnessKind::Claude,
            "claude-sonnet-5-5",
            None,
        ));
        let mut pinned = teammate("pinned", HarnessKind::Claude, "claude-opus-5-5", None);
        pinned.no_fallback_reason = Some("a top-tier seat".into());
        r.insert_for_test(pinned);
        r
    }

    fn pool(used: f64, resets_at: Option<&str>) -> PoolReading {
        PoolReading {
            windows: vec![QuotaWindow::new(
                300,
                used,
                resets_at.map(str::to_string),
                None,
            )],
            observed_at: Some("2026-11-17T11:59:00Z".into()),
            ..Default::default()
        }
    }

    /// A quota file with `claude` and `codex` at the used shares given.
    fn quota(claude: PoolReading, codex: PoolReading) -> QuotaView {
        let mut pools = BTreeMap::new();
        pools.insert("claude".to_string(), claude);
        pools.insert("codex".to_string(), codex);
        let file = QuotaFile {
            schema: 1,
            written_at: None,
            pools,
            harnesses: BTreeMap::new(),
        };
        QuotaView::new(file, now(), Policy::default(), true)
    }

    struct Fixture {
        dir: tempfile::TempDir,
        store: Store,
        task: String,
        worker: String,
    }

    /// A run with 1 task of `persona` (with the task fallback `fallback`)
    /// that 1 worker of `persona` claimed. Its attempt then ended with the
    /// outcome `quota`, as the stall ladder ends it.
    fn fixture(persona: &str, fallback: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (task, worker) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                let mut new = NewTask::work("a", "tickets/a.md", persona);
                new.fallback = fallback.map(str::to_string);
                tasks::add(tx, &run, &new, Actor::Orchestrator)?;
                let w = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: persona.into(),
                    pane_id: Some("w0:p1".into()),
                    ..Default::default()
                };
                let id = workers::insert(tx, &w)?;
                workers::supervisor_started(tx, &id, 4_241, Some(76))?;
                workers::agent_started(tx, &id, AGENT.pid, AGENT.pid_start)?;
                let ClaimOutcome::Claimed(c) = tasks::claim(tx, &id)? else {
                    panic!("no claim");
                };
                Ok((c.task.task_id, id))
            })
            .unwrap();
        let mut f = Fixture {
            dir,
            store,
            task,
            worker,
        };
        f.end_with_quota();
        f
    }

    impl Fixture {
        fn first_attempt(&self) -> String {
            attempt_id(&self.task, 1)
        }

        /// The stall ladder ends the attempt `quota` and the worker row.
        fn end_with_quota(&mut self) {
            let reap = Reap {
                attempt: self.first_attempt(),
                worker: self.worker.clone(),
                outcome: Outcome::Quota.as_str().to_string(),
                end_worker: true,
                task: None,
                cleanup: Default::default(),
            };
            self.store
                .immediate(|tx| commands::end_attempt(tx, &reap).map(|_| ()))
                .unwrap();
        }

        fn on_quota(&mut self, q: &QuotaView) -> Option<Moved> {
            let roster = roster();
            let pools = Pools {
                roster: &roster,
                quota: q,
                full_use: FULL,
            };
            let a = self.first_attempt();
            self.store
                .immediate(|tx| on_quota(tx, None, &a, &pools))
                .unwrap()
        }

        fn release(&mut self, q: &QuotaView) -> Vec<String> {
            let roster = roster();
            let pools = Pools {
                roster: &roster,
                quota: q,
                full_use: FULL,
            };
            self.store
                .immediate(|tx| release_reset_pools(tx, None, &pools))
                .unwrap()
        }

        fn task(&self) -> tasks::Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        fn lease(&self) -> Option<i64> {
            self.store
                .conn()
                .query_row(
                    "SELECT expires_at FROM leases WHERE task_id = ?1",
                    [&self.task],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
        }

        fn causes(&self) -> Vec<(String, String, String)> {
            let mut stmt = self
                .store
                .conn()
                .prepare(
                    "SELECT from_state, to_state, cause FROM events
                     WHERE entity = 'task' AND entity_id = ?1 AND from_state IS NOT NULL
                     ORDER BY rowid",
                )
                .unwrap();
            stmt.query_map([&self.task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }
    }

    fn claude_full() -> QuotaView {
        quota(pool(1.0, Some("2026-11-17T14:00:00Z")), pool(0.1, None))
    }

    fn both_full() -> QuotaView {
        quota(pool(1.0, Some("2026-11-17T14:00:00Z")), pool(0.99, None))
    }

    #[test]
    fn fm5_usage_limit_burns_no_attempt_and_moves_to_fallback() {
        let mut f = fixture("dev", None);
        let before = f.task();
        let moved = f.on_quota(&claude_full());
        let Some(Moved::Restart { attempt, worker }) = moved else {
            panic!("{moved:?}");
        };
        let t = f.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.claimable_by.as_deref(), Some("alt"));
        assert_eq!(t.current_worker_id, None);
        assert_eq!(
            t.attempts, before.attempts,
            "tasks.attempts does not change"
        );
        assert_eq!(t.attempt_floor, before.attempt_floor + 1);
        assert_eq!(
            t.attempts - t.attempt_floor,
            before.attempts - before.attempt_floor - 1
        );
        assert_eq!(f.lease(), None, "a ready task holds no lease");
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            (
                "running".into(),
                "ready".into(),
                USAGE_LIMIT_FALLBACK.into()
            )
        );
        // The new worker of the fallback, spawned for the task.
        let w = workers::get(f.store.conn(), &worker).unwrap();
        assert_eq!(w.persona.as_deref(), Some("alt"));
        assert_eq!(w.status, WorkerStatus::Spawning.as_str());
        assert_eq!(w.target_task_id.as_deref(), Some(f.task.as_str()));
        assert_eq!(w.previous_worker_id.as_deref(), Some(f.worker.as_str()));
        let rows = store::attempts_of_worker(f.store.conn(), &worker).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].id.as_str(), rows[0].state),
            (attempt.as_str(), AttemptState::Launching)
        );
        assert!(is_restart(&rows[0]));
        // A second report of the same attempt moves nothing.
        assert_eq!(f.on_quota(&claude_full()), None);
    }

    #[test]
    fn fallback_on_the_same_pool_or_no_fallback_waits() {
        // The task's own fallback wins over the persona's: `same` is on
        // claude too.
        let mut f = fixture("dev", Some("same"));
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        assert_eq!(f.task().status, "waiting_quota");
        // Both pools limited: the task waits.
        let mut f = fixture("dev", None);
        assert_eq!(f.on_quota(&both_full()), Some(Moved::Wait));
    }

    #[test]
    fn persona_with_a_no_fallback_reason_waits_in_waiting_quota() {
        // Even a fallback of the task on another pool does not move it.
        let mut f = fixture("pinned", Some("alt"));
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        let t = f.task();
        assert_eq!(t.status, "waiting_quota");
        assert_eq!(t.claimable_by.as_deref(), Some("pinned"));
    }

    #[test]
    fn transition_running_to_waiting_quota_pauses_the_lease() {
        let mut f = fixture("same", None);
        let before = f.task();
        let lease = f.lease().expect("the claim wrote a lease");
        assert_eq!(f.on_quota(&claude_full()), Some(Moved::Wait));
        let t = f.task();
        assert_eq!(t.status, "waiting_quota");
        assert_eq!(t.attempts, before.attempts);
        assert_eq!(t.attempt_floor, before.attempt_floor + 1);
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            ("running".into(), "waiting_quota".into(), USAGE_LIMIT.into())
        );
        // A pass far past the deadline neither expires nor renews it.
        let limits = |_: &str| Limits {
            lease_s: 120,
            stall_after_s: 300,
            heartbeat_max_age_s: 30,
        };
        let expired = f
            .store
            .immediate(|tx| {
                lease::renew_or_expire_leases(
                    tx,
                    None,
                    FleetSeconds(lease + 100_000),
                    &BTreeMap::new(),
                    &limits,
                )
            })
            .unwrap();
        assert!(expired.is_empty());
        assert_eq!(f.lease(), Some(lease));
        assert_eq!(f.task().status, "waiting_quota");
        // Not reset yet: the task waits.
        assert!(f.release(&claude_full()).is_empty());
        // The calendar time passes `reset_at`: ready, and the lease goes.
        let after_reset = quota(pool(1.0, Some("2026-11-17T11:00:00Z")), pool(0.1, None));
        assert_eq!(f.release(&after_reset), vec![f.task.clone()]);
        assert_eq!(f.task().status, "ready");
        assert_eq!(f.lease(), None);
        let last = f.causes().pop().unwrap();
        assert_eq!(
            last,
            ("waiting_quota".into(), "ready".into(), POOL_RESET.into())
        );
    }

    #[test]
    fn waiting_task_without_reset_at_is_released_when_the_probe_reports_ok() {
        let mut f = fixture("same", None);
        let limited = quota(pool(1.0, None), pool(0.1, None));
        assert_eq!(f.on_quota(&limited), Some(Moved::Wait));
        assert!(f.release(&limited).is_empty());
        let ok = quota(pool(0.2, None), pool(0.1, None));
        assert_eq!(f.release(&ok), vec![f.task.clone()]);
    }

    /// The effects of a restart in a test: which processes live, and each
    /// spec that the launch got.
    struct Fake {
        alive: Cell<bool>,
        specs: RefCell<Vec<Restart>>,
        program: std::path::PathBuf,
    }

    impl Restarts for Fake {
        fn alive(&self, _p: Recorded) -> bool {
            self.alive.get()
        }

        fn spec(&self, r: &Restart) -> Result<LaunchSpec> {
            self.specs.borrow_mut().push(r.clone());
            Ok(LaunchSpec {
                program: self.program.clone(),
                args: Vec::new(),
                cwd: None,
                env_rule: Default::default(),
                detach: false,
                output: None,
            })
        }
    }

    /// A host that starts a real `true` process and counts the launches.
    struct Host {
        launches: Cell<usize>,
    }

    impl ProcessHost for Host {
        fn launch(
            &self,
            spec: &LaunchSpec,
        ) -> std::result::Result<
            Launched,
            crate::fleetd::worker_profiles::application::ports::LaunchError,
        > {
            self.launches.set(self.launches.get() + 1);
            let child = std::process::Command::new(&spec.program).spawn()?;
            Ok(Launched {
                pid: child.id(),
                pid_start: None,
                child,
            })
        }
    }

    fn fake(alive: bool) -> Fake {
        Fake {
            alive: Cell::new(alive),
            specs: RefCell::new(Vec::new()),
            program: which_true(),
        }
    }

    fn which_true() -> std::path::PathBuf {
        ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.exists())
            .expect("a true binary")
    }

    fn host() -> Host {
        Host {
            launches: Cell::new(0),
        }
    }

    #[test]
    fn restart_writes_1_launching_attempt_and_spawns_once_after_the_commit() {
        let mut f = fixture("dev", None);
        let Some(Moved::Restart { attempt, worker }) = f.on_quota(&claude_full()) else {
            panic!("no restart");
        };
        let launching: i64 = f
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE task_id = ?1 AND state = 'launching'",
                [&f.task],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(launching, 1);
        let root = f.dir.path().join("fleet");
        let (ports, host) = (fake(false), host());
        let run = |f: &Fixture| {
            restart(
                f.store.conn(),
                &root,
                FleetSeconds(500),
                &attempt,
                &host,
                &ports,
            )
            .unwrap()
        };
        let Restarted::Launched(mut l) = run(&f) else {
            panic!("no launch");
        };
        l.child.wait().unwrap();
        let specs = ports.specs.borrow().clone();
        assert_eq!(specs.len(), 1);
        let r = &specs[0];
        assert_eq!(
            (r.task_id.as_str(), r.worker_id.as_str()),
            (f.task.as_str(), worker.as_str())
        );
        assert_eq!((r.persona.as_str(), r.run_slug.as_str()), ("alt", "r1"));
        assert_eq!(r.from_pane.as_deref(), Some("w0:p1"));
        let record = effects::read_launch(&root, &attempt).unwrap().unwrap();
        assert_eq!(
            record.process, None,
            "the spawn is not the agent of the attempt"
        );
        // The next pass, and a fleetd restart after the commit, spawn nothing.
        assert!(matches!(run(&f), Restarted::Done));
        assert_eq!(host.launches.get(), 1);
    }

    #[test]
    fn restart_waits_for_the_old_process_to_be_gone() {
        let mut f = fixture("dev", None);
        let Some(Moved::Restart { attempt, .. }) = f.on_quota(&claude_full()) else {
            panic!("no restart");
        };
        let root = f.dir.path().join("fleet");
        let (ports, host) = (fake(true), host());
        let run = || {
            restart(
                f.store.conn(),
                &root,
                FleetSeconds(500),
                &attempt,
                &host,
                &ports,
            )
            .unwrap()
        };
        assert!(matches!(run(), Restarted::Waiting { pid } if pid == AGENT.pid));
        assert_eq!(host.launches.get(), 0);
        assert!(effects::read_launch(&root, &attempt).unwrap().is_none());
        // The stop of part 3 ended the old process: the spawn runs.
        ports.alive.set(false);
        let Restarted::Launched(mut l) = run() else {
            panic!("no launch");
        };
        l.child.wait().unwrap();
        assert_eq!(host.launches.get(), 1);
    }

    #[test]
    fn restart_that_does_not_start_ends_its_worker_launch_failed() {
        let mut f = fixture("dev", None);
        let Some(Moved::Restart { attempt, worker }) = f.on_quota(&claude_full()) else {
            panic!("no restart");
        };
        let root = f.dir.path().join("fleet");
        let mut ports = fake(false);
        ports.program = f.dir.path().join("no-such-program");
        let host = host();
        let r = restart(
            f.store.conn(),
            &root,
            FleetSeconds(500),
            &attempt,
            &host,
            &ports,
        );
        assert!(r.is_err());
        let w = workers::get(f.store.conn(), &worker).unwrap();
        assert_eq!(w.status, WorkerStatus::LaunchFailed.as_str());
        // The record stays: no second spawn, and the start timeout ends
        // the attempt.
        assert!(matches!(
            restart(
                f.store.conn(),
                &root,
                FleetSeconds(501),
                &attempt,
                &host,
                &ports
            )
            .unwrap(),
            Restarted::Done
        ));
    }

    /// The adopted worker of the restart runs its agent `pid`, then claims.
    fn claim_as(f: &mut Fixture, worker: &str, pid: u32) -> tasks::Claimed {
        f.store
            .immediate(|tx| {
                workers::supervisor_started(tx, worker, pid - 1, Some(8))?;
                workers::agent_started(tx, worker, pid, Some(9))?;
                let ClaimOutcome::Claimed(c) = tasks::claim(tx, worker)? else {
                    panic!("no claim");
                };
                Ok(*c)
            })
            .unwrap()
    }

    fn attempts_of_task(f: &Fixture) -> Vec<(String, String, String)> {
        let mut stmt = f
            .store
            .conn()
            .prepare("SELECT id, worker_id, state FROM attempts WHERE task_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&f.task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn leased_attempt(f: &Fixture) -> Option<String> {
        f.store
            .conn()
            .query_row(
                "SELECT attempt_id FROM leases WHERE task_id = ?1",
                [&f.task],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    }

    #[test]
    fn claim_of_the_fallback_worker_takes_its_launching_attempt_to_live() {
        // The pass can see the agent of the new worker before its claim and
        // move the attempt to live: the claim takes it in both states.
        for seen_first in [false, true] {
            let mut f = fixture("dev", None);
            let before = f.task();
            let Some(Moved::Restart { attempt, worker }) = f.on_quota(&claude_full()) else {
                panic!("no restart");
            };
            if seen_first {
                f.store
                    .immediate(|tx| commands::attempt_live(tx, None, &attempt, None))
                    .unwrap();
            }
            let c = claim_as(&mut f, &worker, 5_000);
            assert_eq!(c.task.task_id, f.task);
            let t = f.task();
            assert_eq!(t.status, "running");
            assert_eq!(t.current_worker_id.as_deref(), Some(worker.as_str()));
            assert_eq!(t.attempts, before.attempts + 1);
            assert_eq!(
                t.attempts - t.attempt_floor,
                before.attempts - before.attempt_floor,
                "the usage limit burned no attempt"
            );
            assert_eq!(
                attempts_of_task(&f),
                vec![
                    (f.first_attempt(), f.worker.clone(), "ended".into()),
                    (attempt.clone(), worker.clone(), "live".into()),
                ],
                "seen first: {seen_first}"
            );
            let rows = store::attempts_of_worker(f.store.conn(), &worker).unwrap();
            assert_eq!(rows[0].process.map(|p| p.pid), Some(5_000));
            assert_eq!(leased_attempt(&f), Some(attempt));
        }
    }

    #[test]
    fn claim_after_the_restart_attempt_timed_out_starts_a_new_attempt() {
        let mut f = fixture("dev", None);
        let Some(Moved::Restart { attempt, worker }) = f.on_quota(&claude_full()) else {
            panic!("no restart");
        };
        let reap = Reap {
            attempt: attempt.clone(),
            worker: worker.clone(),
            outcome: "timed_out".into(),
            end_worker: false,
            task: None,
            cleanup: Default::default(),
        };
        f.store
            .immediate(|tx| commands::end_attempt(tx, &reap).map(|_| ()))
            .unwrap();
        claim_as(&mut f, &worker, 5_000);
        let new = attempt_id(&f.task, 2);
        assert_eq!(
            attempts_of_task(&f),
            vec![
                (f.first_attempt(), f.worker.clone(), "ended".into()),
                (attempt, worker.clone(), "ended".into()),
                (new.clone(), worker, "live".into()),
            ]
        );
        assert_eq!(leased_attempt(&f), Some(new));
    }

    #[test]
    fn pres_06_s13_quota_probe_holds_no_credential() {
        // The reading is the quota file and nothing else: a pool that the
        // file does not name has no window and no limit.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.json");
        let file = QuotaFile {
            schema: 1,
            written_at: None,
            pools: [(
                "claude".to_string(),
                pool(1.0, Some("2026-11-17T14:00:00Z")),
            )]
            .into_iter()
            .collect(),
            harnesses: BTreeMap::new(),
        };
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
        let loaded = QuotaFile::load(dir.path(), Some(&path)).unwrap();
        let view = QuotaView::new(loaded, now(), Policy::default(), true);
        let roster = roster();
        let pools = Pools {
            roster: &roster,
            quota: &view,
            full_use: FULL,
        };
        let dev = pools.reading("dev").unwrap();
        assert_eq!(dev.pool, "claude");
        assert_eq!(dev.windows.len(), 1);
        assert_eq!(dev.windows[0].used, 1.0);
        let alt = pools.reading("alt").unwrap();
        assert_eq!((alt.pool.as_str(), alt.windows.len()), ("codex", 0));
        // This module names no credential and reads no environment.
        let src = include_str!("quota.rs");
        let code = src
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .to_ascii_lowercase();
        for word in [
            "credential",
            "token",
            "keychain",
            "api_key",
            "env::var",
            "auth",
        ] {
            assert!(!code.contains(word), "quota.rs names {word}");
        }
    }
}
