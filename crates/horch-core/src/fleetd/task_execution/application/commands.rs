//! The task commands: I4, I5 and the process part of I1.
//!
//! Each command writes its rows in the transaction it is given and returns
//! the stops to do after the commit. No command signals a process or
//! closes a pane: a reap ends the rows and queues the task in 1
//! transaction before any signal (`preservation-04-behavior.md`, S7).
//!
//! - `transition()` deletes the lease of a task that moves out of the
//!   states that hold a lease: [`schedule_retry`] and [`escalate_task`].
//! - `horch ledger done` ends the attempt of its worker:
//!   [`end_own_attempt`].
//! - I1, process part: [`finish_exited`]. Each dead attempt goes through
//!   [`end_attempt`], then rung 6 ([`schedule_retry`]).
//! - I5: [`mark_orphans`].

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::application::retry::{self, Policy};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::{
    dead_attempts, held_attempt, orphans, AttemptState, Facts, KeptGroup, KeptPane, Reap, Recorded,
    RowRef, Stop,
};
use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
use crate::taskledger::workers;
use crate::taskledger::{tasks, util};

/// The kind of the needs-you item of a pane that is not closed.
const PANE_KEPT: &str = "pane_kept";

const GROUP_KEPT: &str = "group_kept";

/// The commands write rows and events together: refuse a connection with
/// no open transaction.
fn check_open(tx: &Connection) -> Result<()> {
    if tx.is_autocommit() {
        bail!("the reap path writes its rows in 1 open transaction of the ledger");
    }
    Ok(())
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// `horch ledger done`: the attempt that worker `worker_id` holds moves to
/// `ended` with the outcome `outcome`, in the transaction of `done`. A
/// worker with no attempt that holds it writes nothing.
pub fn end_own_attempt(tx: &Connection, worker_id: &str, outcome: &str) -> Result<()> {
    let text = format!("its worker ran horch ledger done: {outcome}");
    let by = Cause {
        actor: Actor::Worker,
        text: &text,
    };
    end_held_attempt(tx, worker_id, outcome, by)
}

/// `horch ledger stop`: the attempt that worker `worker_id` holds moves to
/// `ended` with the outcome `killed`, in the transaction of the stop. The
/// pass then stops its processes with the ladder, as for each ended
/// attempt (I5). A worker with no attempt that holds it writes nothing.
pub fn end_stopped_attempt(tx: &Connection, worker_id: &str, reason: &str) -> Result<()> {
    let text = format!("its worker was stopped: {reason}");
    let by = Cause {
        actor: Actor::Orchestrator,
        text: &text,
    };
    end_held_attempt(tx, worker_id, Outcome::Killed.as_str(), by)
}

fn end_held_attempt(tx: &Connection, worker_id: &str, outcome: &str, by: Cause<'_>) -> Result<()> {
    check_open(tx)?;
    let attempts = store::attempts_of_worker(tx, worker_id)?;
    let Some(a) = held_attempt(worker_id, &attempts) else {
        return Ok(());
    };
    store::set_ended(tx, &a.id, outcome, &util::now())?;
    transition(
        tx,
        None,
        Entity::Attempt,
        &a.id,
        AttemptState::Ended.as_str(),
        by,
        &NeedsYouList,
    )?;
    Ok(())
}

/// `EndAttempt`: the attempt of `reap` to `ended` with its outcome, and a
/// live worker row to `incomplete`. Returns the stops of the rest of the
/// row; each pane that no fact proves goes on the needs-you list.
pub fn end_attempt(tx: &Connection, reap: &Reap) -> Result<Vec<Stop>> {
    check_open(tx)?;
    let text = format!("its process is gone: {}", reap.outcome);
    store::set_ended(tx, &reap.attempt, &reap.outcome, &util::now())?;
    transition(
        tx,
        None,
        Entity::Attempt,
        &reap.attempt,
        AttemptState::Ended.as_str(),
        cause(&text),
        &NeedsYouList,
    )?;
    if reap.end_worker {
        let reason = format!("dead: the process of attempt {} is gone", reap.attempt);
        workers::end(tx, &reap.worker, WorkerStatus::Incomplete, &reason, None)?;
    }
    keep_panes(tx, &reap.cleanup.kept)?;
    Ok(reap.cleanup.stops.clone())
}

/// The launch of the attempt `attempt` in `launching` is seen: its process
/// `launched`, when fleetd launched it, goes on the row, and the attempt
/// moves to `live` in `tx`. The fleetd of generation `guard` writes it.
pub fn attempt_live(
    tx: &Connection,
    guard: Option<Generation>,
    attempt: &str,
    launched: Option<Recorded>,
) -> Result<()> {
    check_open(tx)?;
    if let Some(p) = launched {
        store::set_process(tx, attempt, p)?;
    }
    transition(
        tx,
        guard,
        Entity::Attempt,
        attempt,
        AttemptState::Live.as_str(),
        cause("a live process has the attempt id"),
        &NeedsYouList,
    )?;
    Ok(())
}

/// The wrapper `wrapper` of the `live` attempt `attempt` passed its PID
/// check after the attempt went live: it goes on the row when the row has
/// no process. The first heartbeat can come before the agent starts, and
/// only a heartbeat with the agent PID names the wrapper.
pub fn attempt_wrapper(tx: &Connection, attempt: &str, wrapper: Recorded) -> Result<()> {
    check_open(tx)?;
    store::set_missing_process(tx, attempt, wrapper)?;
    Ok(())
}

/// The agent of the push attempt `attempt` runs: a heartbeat named it and
/// its wrapper. Its worker row, which no claim moves, goes from a
/// pre-claim state to `working`, so the pre-claim sweep does not end it
/// `launch_failed` and an exit ends it by rung 5.
pub fn push_worker_working(
    tx: &Connection,
    guard: Option<Generation>,
    attempt: &str,
) -> Result<()> {
    check_open(tx)?;
    let worker: String = tx.query_row(
        "SELECT worker_id FROM attempts WHERE id = ?1",
        [attempt],
        |r| r.get(0),
    )?;
    let pre_claim = [
        WorkerStatus::Spawning,
        WorkerStatus::Starting,
        WorkerStatus::Claiming,
    ];
    if !pre_claim.contains(&workers::get(tx, &worker)?.status()?) {
        return Ok(());
    }
    let text = format!("the agent of push attempt {attempt} runs");
    let why = Cause {
        actor: Actor::Horch,
        text: &text,
    };
    let to = WorkerStatus::Working.as_str();
    transition(tx, guard, Entity::Worker, &worker, to, why, &NeedsYouList)?;
    Ok(())
}

/// `ScheduleRetry`, rung 6 with the design numbers: the attempt of task
/// `task_id` crashed. The task waits for its backoff in `ready`, first in
/// line, or goes to `failed` or `escalated`
/// ([`retry::burn_attempt`]).
pub fn schedule_retry(tx: &Connection, task_id: &str, why: &str) -> Result<()> {
    check_open(tx)?;
    retry::burn_attempt(tx, task_id, Outcome::Crashed, &Policy::default(), why)?;
    Ok(())
}

/// `EscalateTask`: task `task_id` is held for the operator, through
/// `failed`.
pub fn escalate_task(tx: &Connection, task_id: &str) -> Result<()> {
    check_open(tx)?;
    tasks::escalate(tx, task_id)
}

/// I1, process part: each live attempt of this host whose process is dead
/// ends, and the task of its worker is queued again or held. A resolver or
/// fix attempt of a `done` task ends with no rung 6: the task stays `done`
/// (ticket 33), and ticket 34 counts the end on its branch.
pub fn finish_exited(tx: &Connection, facts: &Facts) -> Result<Vec<Stop>> {
    check_open(tx)?;
    let rows = store::rows(tx, &util::host_name())?;
    let mut stops = Vec::new();
    for reap in dead_attempts(&rows, facts) {
        stops.extend(end_attempt(tx, &reap)?);
        let why = format!("the process of attempt {} is gone", reap.attempt);
        if let Some((task, _)) = &reap.task {
            if tasks::get(tx, task)?.status != TaskStatus::Done.as_str() {
                schedule_retry(tx, task, &why)?;
            }
        }
    }
    Ok(stops)
}

/// I5: the stops of each orphan process, and of each pane that a fact
/// proves.
pub fn mark_orphans(tx: &Connection, facts: &Facts) -> Result<Vec<Stop>> {
    check_open(tx)?;
    let rows = store::rows(tx, &util::host_name())?;
    let out = orphans(&rows, facts);
    keep_panes(tx, &out.kept)?;
    keep_groups(tx, &out.kept_groups)?;
    Ok(out.stops)
}

/// Open 1 needs-you item for each process group in `kept`: the leader of
/// the row is gone, and a new process has its pid, so fleetd does not stop
/// the group (FM-22).
pub fn keep_groups(tx: &Connection, kept: &[KeptGroup]) -> Result<()> {
    for k in kept {
        let (entity, id) = match &k.row {
            RowRef::Worker(id) => (Entity::Worker, id),
            RowRef::Attempt(id) => (Entity::Attempt, id),
        };
        let key = format!("{GROUP_KEPT}:{}", k.group);
        let text = format!(
            "fleetd did not stop process group {} of {entity} {id}: the leader of the row is \
             gone, and a new process has its pid, so the group can be a new group. Stop the \
             processes of the row yourself when they still run.",
            k.group
        );
        needs_you::open_item(
            tx,
            &Write {
                key: &key,
                kind: GROUP_KEPT,
                entity: entity.as_str(),
                entity_id: id,
                text: &text,
            },
        )?;
    }
    Ok(())
}

/// Open 1 needs-you item for each pane in `kept`.
fn keep_panes(tx: &Connection, kept: &[KeptPane]) -> Result<()> {
    for k in kept {
        let (entity, id) = match &k.row {
            RowRef::Worker(id) => (Entity::Worker, id),
            RowRef::Attempt(id) => (Entity::Attempt, id),
        };
        let key = format!("{PANE_KEPT}:{}", k.pane);
        let text = format!(
            "fleetd did not close pane {} of {entity} {id}: no fact proves that it is the pane \
             of the row, and herdr can give an old pane id to a pane outside the fleet. Close \
             the pane yourself when it is the pane of the row.",
            k.pane
        );
        needs_you::open_item(
            tx,
            &Write {
                key: &key,
                kind: PANE_KEPT,
                entity: entity.as_str(),
                entity_id: id,
                text: &text,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};

    use super::*;
    use crate::fleetd::ideas::application::needs_you;
    use crate::fleetd::task_execution::adapters::sqlite;
    use crate::fleetd::task_execution::domain::task::{
        InPane, ProcessScope, Recorded, RowRef, Seen, Target,
    };
    use crate::taskledger::model::WorktreeMode;
    use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::{util, Store};

    /// A child `sleep` that is killed and reaped when the test ends.
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

        fn alive(&self) -> bool {
            let r = self.recorded();
            crate::procid::alive(r.pid, r.pid_start)
        }

        /// Kill and reap it: its pid then names no process.
        fn end(mut self) -> Recorded {
            let r = self.recorded();
            self.0.kill().unwrap();
            self.0.wait().unwrap();
            r
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
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir.path(), &new_run)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        Fixture {
            _dir: dir,
            store,
            run,
        }
    }

    impl Fixture {
        fn add_task(&mut self) -> String {
            let mut new = NewTask::work("a", "tickets/a.md", "dev");
            new.worktree_mode = WorktreeMode::Shared;
            let run = self.run.clone();
            self.store
                .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        }

        /// A worker with `agent` and `supervisor`, in pane `w0:p1`.
        fn worker(&mut self, agent: Recorded, supervisor: Recorded) -> String {
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                pane_id: Some("w0:p1".into()),
                ..Default::default()
            };
            self.store
                .immediate(|tx| {
                    let id = workers::insert(tx, &new)?;
                    workers::supervisor_started(tx, &id, supervisor.pid, supervisor.pid_start)?;
                    workers::agent_started(tx, &id, agent.pid, agent.pid_start)?;
                    Ok(id)
                })
                .unwrap()
        }

        /// Bind `task` to a new worker with `agent` and `supervisor`, in
        /// pane `w0:p1`: 1 live attempt.
        fn bind(&mut self, task: &str, agent: Recorded, supervisor: Recorded) -> String {
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                pane_id: Some("w0:p1".into()),
                ..Default::default()
            };
            let conn = self.store.conn();
            let w = crate::taskledger::testing::bind_live(conn, task, &new, Some(agent))
                .unwrap()
                .worker_id;
            conn.execute(
                "UPDATE workers SET supervisor_pid = ?2, supervisor_started = ?3 WHERE worker_id = ?1",
                rusqlite::params![w, supervisor.pid, supervisor.pid_start.map(|s| s as i64)],
            )
            .unwrap();
            w
        }

        fn end_worker(&mut self, worker: &str, status: WorkerStatus, host: Option<&str>) {
            self.store
                .immediate(|tx| {
                    workers::end(tx, worker, status, "test", None)?;
                    if let Some(host) = host {
                        tx.execute(
                            "UPDATE workers SET host = ?2 WHERE worker_id = ?1",
                            rusqlite::params![worker, host],
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
        }

        fn task_status(&self, task: &str) -> String {
            tasks::get(self.store.conn(), task).unwrap().status
        }

        fn worker_status(&self, worker: &str) -> String {
            workers::get(self.store.conn(), worker).unwrap().status
        }

        fn attempt_states(&self) -> Vec<(String, String)> {
            let rows = sqlite::rows(self.store.conn(), &util::host_name()).unwrap();
            rows.attempts
                .into_iter()
                .map(|a| (a.id, a.state.as_str().to_string()))
                .collect()
        }

        /// The facts of the OS for each process that a row names, and the
        /// panes `panes`.
        fn facts(&self, panes: &[(&str, InPane)]) -> Facts {
            let rows = sqlite::rows(self.store.conn(), &util::host_name()).unwrap();
            let processes = rows
                .named_processes()
                .into_iter()
                .map(|p| {
                    let seen = if crate::procid::alive(p.pid, p.pid_start) {
                        Seen::Alive
                    } else {
                        Seen::Dead
                    };
                    (p, seen)
                })
                .collect();
            Facts {
                processes,
                panes: Some(panes.iter().map(|(p, r)| (p.to_string(), *r)).collect()),
                stopped: Vec::new(),
            }
        }

        fn finish_exited(&mut self, facts: &Facts) -> Vec<Stop> {
            self.store.immediate(|tx| finish_exited(tx, facts)).unwrap()
        }

        fn mark_orphans(&mut self, facts: &Facts) -> Vec<Stop> {
            self.store.immediate(|tx| mark_orphans(tx, facts)).unwrap()
        }

        fn open_items(&self) -> Vec<String> {
            needs_you::open_items(self.store.conn())
                .unwrap()
                .into_iter()
                .map(|i| i.key)
                .collect()
        }
    }

    /// `p` runs in a pane.
    fn runs(p: Recorded) -> InPane {
        InPane::Runs {
            pid: p.pid,
            pid_start: p.pid_start.unwrap(),
        }
    }

    fn process_stop(row: RowRef, p: Recorded) -> Stop {
        Stop {
            row,
            target: Target::Process {
                pid: p.pid,
                pid_start: p.pid_start.unwrap(),
                scope: ProcessScope::LeaderGroup,
            },
        }
    }

    #[test]
    fn i1_running_task_with_a_dead_process_ends_its_attempt_and_is_queued_again() {
        let mut l = fixture();
        let task = l.add_task();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        let w = l.bind(&task, agent.recorded(), supervisor.recorded());
        agent.end();

        let stops = l.finish_exited(&l.facts(&[]));

        assert_eq!(l.attempt_states(), [(format!("{task}/A1"), "ended".into())]);
        assert_eq!(l.worker_status(&w), "incomplete");
        assert_eq!(l.task_status(&task), "ready");
        assert_eq!(
            stops,
            [process_stop(
                RowRef::Worker(w.clone()),
                supervisor.recorded()
            )]
        );
        // The next pass finds nothing more to end.
        assert_eq!(l.finish_exited(&l.facts(&[])), []);
    }

    /// Ticket 33: the dead process of a resolver or fix attempt of a `done`
    /// task ends its attempt with no rung 6, and the task stays `done`.
    #[test]
    fn dead_attempt_of_a_done_task_ends_and_the_task_stays_done() {
        let mut l = fixture();
        let task = l.add_task();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        let w = l.bind(&task, agent.recorded(), supervisor.recorded());
        l.store
            .conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&task],
            )
            .unwrap();
        agent.end();

        l.finish_exited(&l.facts(&[]));

        assert_eq!(l.attempt_states(), [(format!("{task}/A1"), "ended".into())]);
        assert_eq!(l.worker_status(&w), "incomplete");
        assert_eq!(l.task_status(&task), "done");
    }

    #[test]
    fn dead_process_at_max_attempts_fails_the_task_for_its_replan() {
        let mut l = fixture();
        let task = l.add_task();
        l.store
            .immediate(|tx| {
                tx.execute("UPDATE runs SET max_attempts = 1", [])?;
                Ok(())
            })
            .unwrap();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        l.bind(&task, agent.recorded(), supervisor.recorded());
        agent.end();

        l.finish_exited(&l.facts(&[]));

        // L4: no attempt left. Rung 6: failed, for its 1 replan.
        let t = tasks::get(l.store.conn(), &task).unwrap();
        assert_eq!(t.status, TaskStatus::Failed.as_str());
        assert_eq!(t.held_reason, None);
    }

    #[test]
    fn fm13_after_a_herdr_restart_dead_attempts_end_and_their_tasks_are_queued_again() {
        let mut l = fixture();
        let (t1, t2) = (l.add_task(), l.add_task());
        let mut agents = Vec::new();
        let mut workers = Vec::new();
        for t in [&t1, &t2] {
            let (agent, supervisor) = (Sleep::start(), Sleep::start());
            let w = l.bind(t, agent.recorded(), supervisor.end());
            agents.push(agent);
            workers.push(w);
        }
        // The restart: every agent process is gone, and herdr gave the
        // pane id of the rows to a pane that runs another process.
        let stranger = Sleep::start();
        for a in agents {
            a.end();
        }
        let facts = l.facts(&[("w0:p1", runs(stranger.recorded()))]);

        let stops = l.finish_exited(&facts);

        assert_eq!(stops, []);
        for t in [&t1, &t2] {
            assert_eq!(l.task_status(t), "ready");
        }
        for w in &workers {
            assert_eq!(l.worker_status(w), "incomplete");
        }
        assert_eq!(l.open_items(), ["pane_kept:w0:p1"]);
        assert!(stranger.alive());
    }

    #[test]
    fn i5_orphan_process_or_pane_is_stopped() {
        let mut l = fixture();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        let w = l.worker(agent.recorded(), supervisor.end());
        l.end_worker(&w, WorkerStatus::Stopped, None);

        let stops = l.mark_orphans(&l.facts(&[]));
        assert_eq!(
            stops,
            [process_stop(RowRef::Worker(w.clone()), agent.recorded())]
        );

        // A pane whose process is the recorded agent is the pane of the row.
        let stops = l.mark_orphans(&l.facts(&[("w0:p1", runs(agent.recorded()))]));
        assert_eq!(
            stops,
            [
                process_stop(RowRef::Worker(w.clone()), agent.recorded()),
                Stop {
                    row: RowRef::Worker(w),
                    target: Target::Pane {
                        pane: "w0:p1".into(),
                        shell: agent.recorded(),
                    },
                },
            ]
        );
        assert!(agent.alive(), "a command signals nothing");
    }

    #[test]
    fn pane_with_a_reused_id_and_another_process_is_not_closed_after_a_herdr_restart() {
        let mut l = fixture();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        let w = l.worker(agent.recorded(), supervisor.end());
        l.end_worker(&w, WorkerStatus::Stopped, None);
        let stranger = Sleep::start();

        let stops = l.mark_orphans(&l.facts(&[("w0:p1", runs(stranger.recorded()))]));

        assert_eq!(stops, [process_stop(RowRef::Worker(w), agent.recorded())]);
        assert_eq!(l.open_items(), ["pane_kept:w0:p1"]);
        // No process of the snapshot either: the pane is kept too.
        let stops = l.mark_orphans(&l.facts(&[("w0:p1", InPane::Unknown)]));
        assert!(!stops
            .iter()
            .any(|s| matches!(s.target, Target::Pane { .. })));
    }

    #[test]
    fn pres_04_s7_reap_requeues_in_1_transaction_before_the_signal() {
        let mut l = fixture();
        let task = l.add_task();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        let w = l.bind(&task, agent.recorded(), supervisor.recorded());
        agent.end();
        let facts = l.facts(&[]);

        // No transaction: the command refuses and writes nothing.
        assert!(finish_exited(l.store.conn(), &facts).is_err());
        assert_eq!(l.task_status(&task), "running");

        // A rolled back transaction leaves every row as it was.
        {
            let tx = l.store.conn().unchecked_transaction().unwrap();
            let stops = finish_exited(&tx, &facts).unwrap();
            assert_eq!(stops.len(), 1);
            assert_eq!(tasks::get(&tx, &task).unwrap().status, "ready");
            assert!(supervisor.alive(), "no signal inside the transaction");
        }
        assert_eq!(l.task_status(&task), "running");
        assert_eq!(l.worker_status(&w), "working");
        assert_eq!(l.attempt_states(), [(format!("{task}/A1"), "live".into())]);

        // The commit writes the 3 rows together; the stop is still to do.
        let stops = l.finish_exited(&facts);
        assert_eq!(
            (l.task_status(&task), l.worker_status(&w)),
            ("ready".into(), "incomplete".into())
        );
        assert_eq!(l.attempt_states(), [(format!("{task}/A1"), "ended".into())]);
        assert_eq!(
            stops,
            [process_stop(RowRef::Worker(w), supervisor.recorded())]
        );
        assert!(supervisor.alive());
    }

    #[test]
    fn pres_04_s8_stop_signals_only_the_recorded_pid_of_this_host() {
        let mut l = fixture();
        let (here, there, restarted) = (Sleep::start(), Sleep::start(), Sleep::start());
        let gone = Sleep::start().end();
        let w_here = l.worker(here.recorded(), gone);
        l.end_worker(&w_here, WorkerStatus::Stopped, None);
        let w_there = l.worker(there.recorded(), gone);
        l.end_worker(&w_there, WorkerStatus::Stopped, Some("another-host"));
        // The pid of a live process with another start time.
        let other_start = Recorded {
            pid: restarted.recorded().pid,
            pid_start: restarted.recorded().pid_start.map(|s| s + 1),
        };
        let w_reused = l.worker(other_start, gone);
        l.end_worker(&w_reused, WorkerStatus::Stopped, None);

        let stops = l.mark_orphans(&l.facts(&[]));

        assert_eq!(
            stops,
            [process_stop(RowRef::Worker(w_here), here.recorded())]
        );
    }

    #[test]
    fn process_that_no_row_names_is_not_touched() {
        let mut l = fixture();
        let task = l.add_task();
        let (agent, supervisor) = (Sleep::start(), Sleep::start());
        l.bind(&task, agent.recorded(), supervisor.recorded());
        agent.end();
        let unnamed = Sleep::start();
        let facts = l.facts(&[]);
        assert!(!facts.processes.contains_key(&unnamed.recorded()));

        let mut stops = l.mark_orphans(&facts);
        stops.extend(l.finish_exited(&facts));
        stops.extend(l.mark_orphans(&l.facts(&[])));

        let pids: Vec<u32> = stops
            .iter()
            .filter_map(|s| match s.target {
                Target::Process { pid, .. } => Some(pid),
                Target::Pane { .. } => None,
            })
            .collect();
        assert!(!pids.contains(&unnamed.recorded().pid), "{stops:?}");
        assert!(unnamed.alive());
        assert_eq!(l.task_status(&task), "ready");
    }
}
