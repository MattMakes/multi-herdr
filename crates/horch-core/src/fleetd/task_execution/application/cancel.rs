//! The commands of the operator on 1 task: `horch task cancel` and
//! `horch task requeue` (`docs/specs/fleetd/design.md`, "The task state
//! machine" and "A cancel"). The rules are in
//! [`domain::cancel`](crate::fleetd::task_execution::domain::cancel).
//!
//! The cancel is crash safe (`docs/tickets/fleetd-p2/MAP.md`, Q11):
//!
//! - [`request`]: with no live attempt, the task moves to `cancelled` at
//!   once. With a live attempt, 1 transaction writes the reason in
//!   `tasks.cancel_requested`, ends the attempt with the outcome `killed`
//!   and the cause [`TASK_CANCELLED`], ends its worker row and lets go of
//!   it. Rung 6 and the claim skip such a task.
//! - After the commit, the caller stops each [`Held`] worker. When its
//!   processes are gone, [`complete`] moves the task to `cancelled` in a
//!   second transaction.
//! - When the caller does not see the processes gone (a timeout, or the
//!   caller dies), the pass calls [`complete_cancels`].
//!
//! [`requeue`]: `escalated` -> `ready` with the attempt count at 0, and
//! `failed` -> `ready` for its 1 replan. Both also requeue the abandoned
//! `task_check` job of the task.
//!
//! Before Phase 5 the commands write the ledger directly through
//! `transition()`.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::domain::cancel::{
    cancel_path, requeue_path, stop_seen, CancelPath, RequeuePath, TASK_CANCELLED,
};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::{AttemptState, Facts, Recorded};
use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::{util, workers};

/// A worker that a cancel stops, with the process of its attempt: the
/// agent process of the attempt, else of the worker. `None` when no row
/// records one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub worker_id: String,
    pub process: Option<Recorded>,
}

/// What [`request`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Cancel {
    /// The task is `cancelled`. `holder` is the worker that held it with
    /// no attempt row: the caller stops it after the commit, as today.
    Cancelled { task: Task, holder: Option<String> },
    /// The task waits for the stop of `held`, with `cancel_requested` set.
    Stopping { task: Task, held: Vec<Held> },
}

/// 1 attempt of a task, with its process.
struct AttemptOf {
    id: String,
    worker_id: String,
    holds: bool,
    process: Option<Recorded>,
}

fn recorded(pid: Option<i64>, start: Option<i64>) -> Option<Recorded> {
    let pid = pid.and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0)?;
    Some(Recorded {
        pid,
        pid_start: start.and_then(|s| u64::try_from(s).ok()),
    })
}

/// The attempts of task `task_id`. An attempt with no process of its own
/// has the agent of its worker.
fn attempts_of(conn: &Connection, task_id: &str) -> Result<Vec<AttemptOf>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.worker_id, a.state, COALESCE(a.pid, w.agent_pid),
            CASE WHEN a.pid IS NULL THEN w.agent_started ELSE a.pid_start END
         FROM attempts a LEFT JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.task_id = ?1 ORDER BY a.rowid",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        let state: String = r.get(2)?;
        Ok(AttemptOf {
            id: r.get(0)?,
            worker_id: r.get(1)?,
            holds: AttemptState::parse(&state).is_some_and(AttemptState::holds),
            process: recorded(r.get(3)?, r.get(4)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The processes of the attempts of task `task_id`: a cancel of the task
/// completes when each is gone.
pub fn watched(conn: &Connection, task_id: &str) -> Result<Vec<Recorded>> {
    let mut out: Vec<Recorded> = attempts_of(conn, task_id)?
        .into_iter()
        .filter_map(|a| a.process)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// `horch task cancel` of `task` for `reason`, in the open transaction
/// `tx`. `done` and `cancelled` refuse with the reason of the door.
pub fn request(tx: &Connection, task: &Task, reason: &str) -> Result<Cancel> {
    let attempts = attempts_of(tx, &task.task_id)?;
    let requested = tasks::cancel_requested(tx, &task.task_id)?;
    let live = attempts.iter().any(|a| a.holds);
    match cancel_path(&task.status, live, requested.is_some()) {
        CancelPath::Refused => Err(refused(task, reason).into()),
        CancelPath::Now => {
            let t = tasks::cancel(tx, task, reason)?;
            let holder = task.current_worker_id.clone();
            Ok(Cancel::Cancelled { task: t, holder })
        }
        CancelPath::StopFirst if requested.is_some() => {
            // A cancel that waits already: stop each attempt again.
            let held = attempts.into_iter().map(held_of).collect();
            let task = tasks::get(tx, &task.task_id)?;
            Ok(Cancel::Stopping { task, held })
        }
        CancelPath::StopFirst => stop_first(tx, task, reason, attempts),
    }
}

fn held_of(a: AttemptOf) -> Held {
    Held {
        worker_id: a.worker_id,
        process: a.process,
    }
}

/// The refusal of the move of `task` to `cancelled`, as the door words it.
fn refused(task: &Task, reason: &str) -> DoorError {
    DoorError::Refused {
        entity: Entity::Task,
        id: task.task_id.clone(),
        from: task.status.clone(),
        to: TaskStatus::Cancelled.as_str().to_string(),
        cause: format!("{TASK_CANCELLED}: {reason}"),
        actor: Actor::Orchestrator,
    }
}

/// The first transaction of a cancel with a live attempt: each attempt
/// that holds its worker ends `killed`, its worker row ends `stopped`, the
/// task lets go of its worker and its lease, and `cancel_requested` holds
/// the reason.
fn stop_first(
    tx: &Connection,
    task: &Task,
    reason: &str,
    attempts: Vec<AttemptOf>,
) -> Result<Cancel> {
    let why = format!("{TASK_CANCELLED}: {reason}");
    let now = util::now();
    let mut held: Vec<Held> = Vec::new();
    for a in attempts.into_iter().filter(|a| a.holds) {
        store::set_ended(tx, &a.id, Outcome::Killed.as_str(), &now)?;
        let cause = Cause {
            actor: Actor::Orchestrator,
            text: &why,
        };
        let ended = AttemptState::Ended.as_str();
        transition(
            tx,
            None,
            Entity::Attempt,
            &a.id,
            ended,
            cause,
            &NeedsYouList,
        )?;
        held.push(held_of(a));
    }
    if let Some(w) = &task.current_worker_id {
        if !held.iter().any(|h| &h.worker_id == w) {
            let agent = workers::get(tx, w)?;
            held.push(Held {
                worker_id: w.clone(),
                process: recorded(agent.agent_pid, agent.agent_started),
            });
        }
    }
    let ended = format!("task cancelled: {reason}");
    for h in &held {
        workers::end(tx, &h.worker_id, WorkerStatus::Stopped, &ended, None)?;
    }
    tx.execute(
        "UPDATE tasks SET cancel_requested = ?2, current_worker_id = NULL, updated_at = ?3
         WHERE task_id = ?1",
        params![task.task_id, reason, now],
    )?;
    lease::release(tx, &task.task_id)?;
    let task = tasks::get(tx, &task.task_id)?;
    Ok(Cancel::Stopping { task, held })
}

/// The second transaction of the cancel of task `task_id`: when each
/// process of its attempts is dead in `facts`, the task moves to
/// `cancelled` and `cancel_requested` is cleared. `None` while a process
/// is not seen dead, or when no cancel of the task waits.
pub fn complete(tx: &Connection, task_id: &str, facts: &Facts) -> Result<Option<Task>> {
    let Some(reason) = tasks::cancel_requested(tx, task_id)? else {
        return Ok(None);
    };
    if !stop_seen(&watched(tx, task_id)?, facts) {
        return Ok(None);
    }
    let task = tasks::get(tx, task_id)?;
    let task = tasks::cancel(tx, &task, &reason)?;
    tx.execute(
        "UPDATE tasks SET cancel_requested = NULL WHERE task_id = ?1",
        [task_id],
    )?;
    Ok(Some(task))
}

/// The pass: [`complete`] for each task with `cancel_requested`. Gives the
/// ids of the tasks that moved to `cancelled`.
pub fn complete_cancels(tx: &Connection, facts: &Facts) -> Result<Vec<String>> {
    let ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT task_id FROM tasks WHERE cancel_requested IS NOT NULL ORDER BY task_id",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for id in ids {
        if complete(tx, &id, facts)?.is_some() {
            out.push(id);
        }
    }
    Ok(out)
}

/// `horch task requeue` of `task` for `reason`, in the open transaction
/// `tx`: `escalated` -> `ready` with the attempt count at 0, or the 1
/// replan of a `failed` task. A parked task resumes on its worktree (row
/// L5). Each other state refuses.
pub fn requeue(tx: &Connection, jobs: &JobCtx<'_>, task: &Task, reason: &str) -> Result<Task> {
    match requeue_path(&task.status, task.replans) {
        RequeuePath::Escalated | RequeuePath::Replan => {
            let t = tasks::requeue_by_operator(tx, task, reason)?;
            requeue_task_check(tx, jobs, &task.task_id)?;
            Ok(t)
        }
        // `cancelled` is terminal: the door refuses and records it (Q22).
        RequeuePath::Door => tasks::requeue_by_operator(tx, task, reason),
        RequeuePath::ReplanUsed => bail!(
            "{} is {} after its 1 replan; it escalates to the operator, so it is not requeued now",
            task.short(),
            task.shown_status()
        ),
        RequeuePath::NotHeld => bail!(
            "{} is {}; only a held task is requeued",
            task.short(),
            task.shown_status()
        ),
    }
}

/// The exit of the `task_check` job of task `task_id`: when its last run
/// ended `abandoned`, queue 1 more run with its count of abandoned runs at
/// 0, as `horch job requeue` does. A check with a verdict, or no check,
/// gets no run.
fn requeue_task_check(tx: &Connection, ctx: &JobCtx<'_>, task_id: &str) -> Result<()> {
    let Some(last) = jobs_store::last(tx, Kind::TaskCheck, task_id)? else {
        return Ok(());
    };
    if last.state != State::Abandoned {
        return Ok(());
    }
    let key = JobKey {
        kind: Kind::TaskCheck,
        subject: task_id,
        input_key: &last.input_key,
    };
    jobs::requeue(ctx, tx, &key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::jobs_store::NewRun as NewJob;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::task_execution::domain::task::Seen;
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{ClaimOutcome, NewTask, Requeue};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::Store;

    /// A child `sleep`: the agent process of an attempt. It is killed and
    /// reaped when the test ends.
    struct Sleep(Child);

    impl Sleep {
        fn start() -> Sleep {
            Sleep(Command::new("sleep").arg("60").spawn().unwrap())
        }

        fn recorded(&self) -> Recorded {
            let pid = self.0.id();
            Recorded {
                pid,
                pid_start: crate::procid::start_time(pid),
            }
        }

        /// The stop: kill and reap it.
        fn stop(&mut self) {
            self.0.kill().unwrap();
            self.0.wait().unwrap();
        }
    }

    impl Drop for Sleep {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        run: Run,
        task: String,
    }

    /// A run with 1 ready task of `dev`.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, dir.path(), &new_run))
            .unwrap();
        let new = NewTask::work("a", "tickets/a.md", "dev");
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        Fixture {
            _dir: dir,
            store,
            run,
            task,
        }
    }

    impl Fixture {
        /// A worker of `dev` whose agent is `agent`.
        fn worker(&mut self, agent: Recorded) -> String {
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                pane_id: Some("w0:p1".into()),
                ..Default::default()
            };
            self.store
                .immediate(|tx| {
                    let id = workers::insert(tx, &new)?;
                    workers::agent_started(tx, &id, agent.pid, agent.pid_start)?;
                    Ok(id)
                })
                .unwrap()
        }

        /// A worker with agent `agent` claims the task: 1 live attempt.
        fn claimed(&mut self, agent: Recorded) -> String {
            let w = self.worker(agent);
            let out = self.store.immediate(|tx| tasks::claim(tx, &w)).unwrap();
            assert!(matches!(out, ClaimOutcome::Claimed(_)));
            w
        }

        /// Set the stored task word, as a test setup.
        fn set_status(&self, word: &str) {
            self.store
                .conn()
                .execute(
                    "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                    params![self.task, word],
                )
                .unwrap();
        }

        fn task(&self) -> Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        fn request(&mut self, reason: &str) -> Result<Cancel> {
            let t = self.task();
            self.store.immediate(|tx| request(tx, &t, reason))
        }

        fn complete(&mut self, facts: &Facts) -> Option<Task> {
            let id = self.task.clone();
            self.store.immediate(|tx| complete(tx, &id, facts)).unwrap()
        }

        fn requeue(&mut self, reason: &str) -> Result<Task> {
            let t = self.task();
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            self.store.immediate(|tx| requeue(tx, &ctx, &t, reason))
        }

        fn cancel_requested(&self) -> Option<String> {
            tasks::cancel_requested(self.store.conn(), &self.task).unwrap()
        }

        fn attempt(&self) -> (String, Option<String>) {
            self.store
                .conn()
                .query_row(
                    "SELECT state, outcome FROM attempts WHERE task_id = ?1",
                    [&self.task],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }

        fn leases(&self) -> i64 {
            self.store
                .conn()
                .query_row(
                    "SELECT count(*) FROM leases WHERE task_id = ?1",
                    [&self.task],
                    |r| r.get(0),
                )
                .unwrap()
        }

        /// The task fails its attempts and is held for the operator.
        fn escalated(&mut self) {
            let w = self.claimed(Recorded {
                pid: 4_000,
                pid_start: Some(7),
            });
            let id = self.task.clone();
            self.store.immediate(|tx| tasks::escalate(tx, &id)).unwrap();
            self.store
                .immediate(|tx| workers::end(tx, &w, WorkerStatus::Incomplete, "gone", None))
                .unwrap();
            assert_eq!(self.task().status, "escalated");
        }
    }

    /// What a pass saw of `p`.
    fn facts(p: Recorded, seen: Seen) -> Facts {
        let mut f = Facts::default();
        f.processes.insert(p, seen);
        f
    }

    const NOT_TERMINAL: [&str; 10] = [
        "pending",
        "ready",
        "leased",
        "running",
        "stalled",
        "verifying",
        "blocked_on_user",
        "waiting_quota",
        "failed",
        "escalated",
    ];

    /// `ddd/08-code/implementation-plan.md`, Slice 2: from each state that
    /// is not terminal, a task with a live attempt is `cancelled` only
    /// after the process of the attempt is gone.
    #[test]
    fn cancel_from_each_non_terminal_state_stops_the_process_group_before_the_move() {
        for state in NOT_TERMINAL {
            let mut f = fixture();
            let mut agent = Sleep::start();
            let p = agent.recorded();
            let w = f.claimed(p);
            f.set_status(state);

            let Cancel::Stopping { task, held } = f.request("dropped").unwrap() else {
                panic!("{state}: a live attempt is stopped first");
            };
            assert_eq!(task.status, state, "{state}: no move before the stop");
            assert_eq!(
                held,
                [Held {
                    worker_id: w.clone(),
                    process: Some(p),
                }],
                "{state}"
            );
            assert_eq!(f.cancel_requested().as_deref(), Some("dropped"), "{state}");
            assert_eq!(
                f.attempt(),
                ("ended".into(), Some("killed".into())),
                "{state}"
            );
            assert_eq!(f.leases(), 0, "{state}");
            assert_eq!(f.task().current_worker_id, None, "{state}");
            let worker = workers::get(f.store.conn(), &w).unwrap();
            assert_eq!(worker.status, "stopped", "{state}");

            // The process still runs: no move.
            assert_eq!(f.complete(&facts(p, Seen::Alive)), None, "{state}");
            assert_eq!(f.task().status, state, "{state}");

            // The stop, then the move.
            agent.stop();
            let done = f.complete(&facts(p, Seen::Dead)).expect(state);
            assert_eq!(done.status, "cancelled", "{state}");
            assert_eq!(f.cancel_requested(), None, "{state}");
        }
    }

    #[test]
    fn cancel_with_no_live_attempt_is_immediate() {
        let mut f = fixture();
        let Cancel::Cancelled { task, holder } = f.request("not needed").unwrap() else {
            panic!("no live attempt: the cancel is immediate");
        };
        assert_eq!(task.status, "cancelled");
        assert_eq!(holder, None);
        assert_eq!(f.cancel_requested(), None);
        assert_eq!(f.leases(), 0);
    }

    #[test]
    fn cancel_requested_task_is_not_retried_or_claimed() {
        let mut f = fixture();
        f.claimed(Recorded {
            pid: 4_000,
            pid_start: Some(7),
        });
        assert!(matches!(
            f.request("dropped").unwrap(),
            Cancel::Stopping { .. }
        ));
        let id = f.task.clone();

        // Rung 6: no retry of the attempt that the cancel ended.
        let how = f
            .store
            .immediate(|tx| tasks::requeue(tx, &id, "its worker ended"))
            .unwrap();
        assert_eq!(how, Requeue::Skipped);
        assert_eq!(f.task().status, "running");
        assert_eq!(f.task().attempts, 1);

        // No claim, also of a ready task (the restart on a fallback).
        f.set_status("ready");
        let w = f.worker(Recorded {
            pid: 4_001,
            pid_start: Some(7),
        });
        let out = f.store.immediate(|tx| tasks::claim(tx, &w)).unwrap();
        assert!(matches!(out, ClaimOutcome::NoTask { .. }));
        assert_eq!(f.task().status, "ready");
    }

    /// The command died after the first transaction: the pass completes
    /// the cancel when it sees the process gone.
    #[test]
    fn pass_completes_a_cancel_when_the_process_is_gone() {
        let mut f = fixture();
        let p = Recorded {
            pid: 4_000,
            pid_start: Some(7),
        };
        f.claimed(p);
        f.request("dropped").unwrap();

        let alive = f
            .store
            .immediate(|tx| complete_cancels(tx, &facts(p, Seen::Alive)))
            .unwrap();
        assert!(alive.is_empty());
        let unknown = f
            .store
            .immediate(|tx| complete_cancels(tx, &Facts::default()))
            .unwrap();
        assert!(unknown.is_empty(), "a process not read is not gone");
        assert_eq!(f.task().status, "running");

        let gone = f
            .store
            .immediate(|tx| complete_cancels(tx, &facts(p, Seen::Dead)))
            .unwrap();
        assert_eq!(gone, [f.task.clone()]);
        assert_eq!(f.task().status, "cancelled");
        assert_eq!(f.cancel_requested(), None);
    }

    /// A second cancel of a task whose cancel waits stops its attempts
    /// again and writes nothing new.
    #[test]
    fn a_second_cancel_stops_again() {
        let mut f = fixture();
        let p = Recorded {
            pid: 4_000,
            pid_start: Some(7),
        };
        let w = f.claimed(p);
        f.request("dropped").unwrap();
        let Cancel::Stopping { task, held } = f.request("again").unwrap() else {
            panic!("the cancel still waits for its stop");
        };
        assert_eq!(task.status, "running");
        assert_eq!(
            held,
            [Held {
                worker_id: w,
                process: Some(p),
            }]
        );
        assert_eq!(f.cancel_requested().as_deref(), Some("dropped"));
    }

    #[test]
    fn cancel_of_done_or_cancelled_is_refused() {
        for state in ["done", "cancelled"] {
            let mut f = fixture();
            f.set_status(state);
            let err = f.request("late").unwrap_err();
            let Some(DoorError::Refused { from, to, .. }) = err.downcast_ref::<DoorError>() else {
                panic!("{state}: {err:#}");
            };
            assert_eq!((from.as_str(), to.as_str()), (state, "cancelled"));
            assert_eq!(
                err.to_string(),
                format!(
                    "illegal transition of task {}: {state} -> cancelled",
                    f.task
                )
            );
            assert_eq!(f.task().status, state);
            // The refusal is on record: 1 event and 1 needs-you item.
            let refusals: i64 = f
                .store
                .conn()
                .query_row(
                    "SELECT count(*) FROM needs_you WHERE entity_id = ?1 AND state = 'open'",
                    [&f.task],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(refusals, 1, "{state}");
        }
    }

    /// `preservation-04-behavior.md`, row L8: the requeue of an escalated
    /// task by the operator sets its attempt count to 0.
    #[test]
    fn pres_04_l8_operator_requeue_resets_the_attempt_count() {
        let mut f = fixture();
        f.escalated();

        let t = f.requeue("try again").unwrap();

        assert_eq!(t.status, "ready");
        assert_eq!(t.attempts - t.attempt_floor, 0);
        assert_eq!(t.attempts, 1, "the history stays");
        assert_eq!(t.held_reason, None);
        assert_eq!(t.replans, 0);
        let item: String = f
            .store
            .conn()
            .query_row(
                "SELECT state FROM needs_you WHERE key = ?1",
                [tasks::escalated_key(&f.task)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(item, "resolved");
    }

    #[test]
    fn requeue_of_escalated_requeues_its_task_check_job() {
        let mut f = fixture();
        f.escalated();
        let held = jobs_store::insert(
            f.store.conn(),
            &NewJob {
                kind: Kind::TaskCheck,
                subject: &f.task,
                repo: None,
                input_key: "tip1",
                run: 3,
                abandoned_runs: 3,
                not_before: None,
                time_limit_s: 60,
            },
        )
        .unwrap();
        f.store
            .conn()
            .execute("UPDATE jobs SET state = 'abandoned' WHERE id = ?1", [held])
            .unwrap();

        f.requeue("try again").unwrap();

        let row = jobs_store::latest(f.store.conn(), Kind::TaskCheck, &f.task, "tip1")
            .unwrap()
            .unwrap();
        assert_ne!(row.id, held);
        assert_eq!((row.run, row.abandoned_runs), (4, 0));
        assert_eq!(row.state, State::Queued);
    }

    #[test]
    fn requeue_of_failed_is_the_1_replan() {
        let mut f = fixture();
        f.claimed(Recorded {
            pid: 4_000,
            pid_start: Some(7),
        });
        f.set_status("failed");

        let t = f.requeue("replanned").unwrap();

        assert_eq!(t.status, "ready");
        assert_eq!(t.attempts - t.attempt_floor, 0);
        assert_eq!(t.replans, 1);
    }

    #[test]
    fn requeue_of_a_failed_task_after_its_replan_is_refused() {
        let mut f = fixture();
        f.claimed(Recorded {
            pid: 4_000,
            pid_start: Some(7),
        });
        f.set_status("failed");
        f.store
            .conn()
            .execute("UPDATE tasks SET replans = 1 WHERE task_id = ?1", [&f.task])
            .unwrap();

        let err = f.requeue("again").unwrap_err();

        assert!(err.to_string().contains("after its 1 replan"), "{err:#}");
        assert_eq!(f.task().status, "failed");
        assert_eq!(f.task().replans, 1);
    }

    #[test]
    fn requeue_of_a_task_that_is_not_held_names_its_shown_state() {
        let mut f = fixture();
        let err = f.requeue("why").unwrap_err();
        assert_eq!(
            err.to_string(),
            "T-1 is queued; only a held task is requeued"
        );
    }
}
