use anyhow::Result;
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::application::dispatch::{self, Bound, DispatchCtx};
use crate::fleetd::task_execution::application::{commands, lease};
use crate::fleetd::task_execution::domain::task::Recorded;
use crate::taskledger::git::Git;
use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
use crate::taskledger::util;
use crate::taskledger::workers::NewWorker;

pub fn count_statements<T>(conn: &Connection, f: impl FnOnce() -> T) -> (T, usize) {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Select
                | AuthAction::Insert { .. }
                | AuthAction::Update { .. }
                | AuthAction::Delete { .. }
        ) {
            seen.fetch_add(1, Ordering::Relaxed);
        }
        Authorization::Allow
    }));
    let out = f();
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    (out, count.load(Ordering::Relaxed))
}

pub fn bind_live(
    conn: &Connection,
    task_id: &str,
    worker: &NewWorker,
    agent: Option<Recorded>,
) -> Result<Bound> {
    util::atomic(conn, |tx| {
        let clock = lease::fleet_clock(tx)?;
        let config = FleetConfig::default();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock,
            git: &git,
        };
        let bound = dispatch::bind(tx, &ctx, task_id, &worker.persona, worker)?;
        commands::attempt_live(tx, None, &bound.attempt_id, agent)?;
        lease::take(
            tx,
            clock,
            task_id,
            &bound.attempt_id,
            lease::claim_lease_s(),
        )?;
        let why = Cause {
            actor: Actor::Horch,
            text: "the test binds its task to a live attempt",
        };
        let running = TaskStatus::Running.as_str();
        transition(tx, None, Entity::Task, task_id, running, why, &NeedsYouList)?;
        let working = WorkerStatus::Working.as_str();
        let id = bound.worker_id.as_str();
        transition(tx, None, Entity::Worker, id, working, why, &NeedsYouList)?;
        tx.execute(
            "UPDATE workers SET agent_pid = ?2, agent_started = ?3, claimed_at = ?4,
                last_progress_at = ?4, last_progress_source = 'agent'
             WHERE worker_id = ?1",
            params![
                id,
                agent.map(|a| a.pid),
                agent.and_then(|a| a.pid_start).map(|s| s as i64),
                util::now()
            ],
        )?;
        Ok(bound)
    })
}

pub fn record_supervisor(
    conn: &Connection,
    worker_id: &str,
    pid: u32,
    started: Option<u64>,
) -> Result<()> {
    conn.execute(
        "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3,
            started_at = ?4, last_checkin_at = ?4
         WHERE worker_id = ?1",
        params![
            worker_id,
            pid as i64,
            started.map(|s| s as i64),
            util::now()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::task_execution::adapters::sqlite as attempts;
    use crate::fleetd::task_execution::domain::task::AttemptState;
    use crate::taskledger::model::WorktreeMode;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::{workers, Store};

    #[test]
    fn bind_live_gives_a_running_task_a_working_worker_and_a_live_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir.path(), &new_run)?;
                crate::taskledger::schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                Ok(run)
            })
            .unwrap();
        let mut new = NewTask::work("a", "tickets/a.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let agent = Recorded {
            pid: 4_242,
            pid_start: Some(7),
        };

        let bound = bind_live(store.conn(), &task, &worker, Some(agent)).unwrap();

        let conn = store.conn();
        let t = tasks::get(conn, &task).unwrap();
        assert_eq!(t.status, "running");
        assert_eq!(
            t.current_worker_id.as_deref(),
            Some(bound.worker_id.as_str())
        );
        let w = workers::get(conn, &bound.worker_id).unwrap();
        assert_eq!(w.status, "working");
        assert_eq!(w.task_id.as_deref(), Some(task.as_str()));
        let rows = attempts::attempts_of_worker(conn, &bound.worker_id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, bound.attempt_id);
        assert_eq!(rows[0].state, AttemptState::Live);
        assert_eq!(rows[0].process, Some(agent));
        let lease: String = conn
            .query_row(
                "SELECT attempt_id FROM leases WHERE task_id = ?1",
                [&task],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lease, bound.attempt_id);
        let moves: Vec<(String, String)> = conn
            .prepare(
                "SELECT from_state, to_state FROM events WHERE kind = 'transition'
                 AND entity = 'task' AND entity_id = ?1 ORDER BY rowid",
            )
            .unwrap()
            .query_map([&task], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let want = [("ready", "leased"), ("leased", "running")]
            .map(|(a, b)| (a.to_string(), b.to_string()));
        assert_eq!(moves, want);
    }

    #[test]
    fn bind_live_refuses_a_task_that_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir.path(), &new_run)?;
                crate::taskledger::schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                Ok(run)
            })
            .unwrap();
        let mut new = NewTask::work("a", "tickets/a.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        bind_live(store.conn(), &task, &worker, None).unwrap();
        let err = bind_live(store.conn(), &task, &worker, None).unwrap_err();
        assert!(format!("{err:#}").contains("not ready"), "{err:#}");
    }
}
