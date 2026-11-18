//! The use cases of rung 6 (`docs/specs/fleetd/design.md`, "Stall ladder",
//! rung 6): a burned attempt, and the escalation of a `failed` task past
//! its replan limit.
//!
//! - [`burn_attempt`]: 1 attempt ended with an outcome that burns it. The
//!   task waits for its backoff in `ready`, or goes to `failed` with 1 wake
//!   to the orchestrator of the run, or goes to `escalated` after its 1
//!   replan.
//! - [`escalate_overdue`]: the pass moves each `failed` task past its
//!   `due_at` to `escalated`.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::task_execution::domain::retry::{replan_overdue, Outcome};
use crate::fleetd::worker_profiles::domain::profile::Retry;
use crate::taskledger::tasks::{self, Requeue};

/// The numbers that rung 6 reads: the retry numbers of the profile of the
/// task and `replan_limit_s` of `fleet.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub retry: Retry,
    pub replan_limit_s: u64,
}

impl Default for Policy {
    /// The design defaults. A caller with no profile and no `fleet.toml`
    /// uses them.
    fn default() -> Self {
        Policy {
            retry: Retry::default(),
            replan_limit_s: FleetConfig::default().replan_limit_s,
        }
    }
}

/// Rung 6 for task `task_id`, whose attempt ended with `outcome`, in the
/// open transaction `tx`. `policy` holds the numbers of its profile.
/// Refuses an outcome that burns no attempt: it has its own path.
pub fn burn_attempt(
    tx: &Connection,
    task_id: &str,
    outcome: Outcome,
    policy: &Policy,
    why: &str,
) -> Result<Requeue> {
    if tx.is_autocommit() {
        bail!("rung 6 writes its rows in 1 open transaction of the ledger");
    }
    if !outcome.burns_attempt() {
        bail!(
            "the outcome {} burns no attempt; rung 6 does not apply",
            outcome.as_str()
        );
    }
    tasks::burn_attempt(
        tx,
        task_id,
        &format!("{why} ({})", outcome.as_str()),
        policy,
    )
}

/// Move each `failed` task past its `due_at` at fleet clock `clock` to
/// `escalated`. A `failed` task with no `due_at` (it failed while no
/// fleetd row had a clock) gets `clock + replan_limit_s`. Gives the ids of
/// the escalated tasks.
pub fn escalate_overdue(
    tx: &Connection,
    clock: FleetSeconds,
    replan_limit_s: u64,
) -> Result<Vec<String>> {
    if tx.is_autocommit() {
        bail!("rung 6 writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Vec::new();
    for t in tasks::failed(tx)? {
        match t.due_at {
            None => tasks::set_replan_due(tx, &t.task_id, clock, replan_limit_s)?,
            Some(d) if replan_overdue(Some(d), clock.0) => {
                tasks::escalate(tx, &t.task_id)?;
                out.push(t.task_id);
            }
            Some(_) => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::clock_store::ClockStore;
    use crate::fleetd::fleet_runtime::domain::clock::AwakeReading;
    use crate::fleetd::ideas::application::needs_you;
    use crate::fleetd::task_execution::application::commands;
    use crate::fleetd::task_execution::domain::retry::{rung6, Numbers, Rung6};
    use crate::fleetd::worker_profiles::domain::profile::Retry;
    use crate::taskledger::model::WorktreeMode;
    use crate::taskledger::model::{Actor, WorkerStatus};
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{NewTask, Task};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        run: Run,
        task: String,
    }

    /// A ledger with the `fleetd` row at fleet clock `clock`, 1 run with
    /// `max_attempts` 3 and 1 task.
    fn fixture(clock: i64) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
                [clock],
            )
            .unwrap();
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
        let mut new = NewTask::work("a", "tickets/a.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
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
        fn set_clock(&mut self, clock: i64) {
            self.store
                .conn()
                .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
                .unwrap();
        }

        fn task(&self) -> Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        /// The task is bound to a new worker of `dev` when it can start
        /// now; the task id, if it was bound.
        fn claim(&mut self) -> Option<String> {
            let conn = self.store.conn();
            let startable = tasks::startable(conn, &self.run.run_id).unwrap();
            startable.iter().find(|t| t.task_id == self.task)?;
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            };
            crate::taskledger::testing::bind_live(conn, &self.task, &new, None).unwrap();
            Some(self.task.clone())
        }

        /// The worker of the running task ends, and its attempt burns 1.
        fn fail(&mut self, outcome: Outcome) -> Requeue {
            let task = self.task.clone();
            let worker = self.task().current_worker_id.unwrap();
            self.store
                .immediate(|tx| {
                    commands::end_own_attempt(tx, &worker, "crashed")?;
                    workers::end(tx, &worker, WorkerStatus::Incomplete, "test", None)?;
                    burn_attempt(tx, &task, outcome, &Policy::default(), "the worker ended")
                })
                .unwrap()
        }

        /// Claim at `clock`, then fail the attempt.
        fn claim_and_fail(&mut self, clock: i64) -> Requeue {
            self.set_clock(clock);
            assert_eq!(self.claim().as_deref(), Some(self.task.as_str()));
            self.fail(Outcome::Crashed)
        }

        fn replan(&mut self) {
            let t = self.task();
            self.store
                .immediate(|tx| tasks::requeue_by_operator(tx, &t, "replan: the ticket is fixed"))
                .unwrap();
        }

        fn overdue(&mut self, clock: i64) -> Vec<String> {
            self.store
                .immediate(|tx| escalate_overdue(tx, FleetSeconds(clock), 1800))
                .unwrap()
        }

        fn replan_wakes(&self) -> Vec<String> {
            let mut stmt = self
                .store
                .conn()
                .prepare("SELECT text FROM outbox WHERE key LIKE '%:replan:%' ORDER BY rowid")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }

        fn escalated_items(&self) -> Vec<String> {
            needs_you::open_items(self.store.conn())
                .unwrap()
                .into_iter()
                .filter(|i| i.key.starts_with("task_escalated:"))
                .map(|i| i.key)
                .collect()
        }
    }

    #[test]
    fn ready_task_is_not_claimed_before_not_before() {
        let mut l = fixture(100);
        assert_eq!(l.claim_and_fail(100), Requeue::Queued);
        let t = l.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.not_before, Some(130), "30 s on the fleet clock");
        l.set_clock(129);
        assert_eq!(l.claim(), None, "the claim waits for the backoff");
        let waits = tasks::waits(l.store.conn(), &l.run.run_id).unwrap();
        assert_eq!(waits.len(), 1);
        assert_eq!(waits[0].1.to_string(), "retry after 1 s");
        l.set_clock(130);
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn sleep_past_a_backoff_fires_it_only_after_the_rest_of_its_fleet_time() {
        let mut l = fixture(0);
        let boot = |s: u64| AwakeReading {
            boot_id: "b1".into(),
            awake_ms: s * 1000,
        };
        let mut clock = ClockStore::new();
        let mut tick = |l: &mut Fixture, now: AwakeReading| {
            let t = l.store.immediate(|tx| clock.tick(tx, &now, 5)).unwrap();
            clock.committed(t.clone());
            t.clock.0
        };
        tick(&mut l, boot(0));
        assert_eq!(tick(&mut l, boot(5)), 5);
        assert_eq!(l.claim_and_fail(5), Requeue::Queued);
        assert_eq!(l.task().not_before, Some(35));
        // The machine sleeps 1 hour: the awake time does not move, so the
        // tick after the wake adds 0, and the backoff does not fire.
        assert_eq!(tick(&mut l, boot(5)), 5);
        assert_eq!(l.claim(), None);
        // The rest of its fleet time: 25 s more, 5 s for each pass.
        let mut now = 5;
        while now < 30 {
            now += 5;
            assert_eq!(tick(&mut l, boot(now)), i64::try_from(now).unwrap());
            assert_eq!(l.claim(), None, "fleet clock {now}");
        }
        assert_eq!(tick(&mut l, boot(35)), 35);
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn failed_task_wakes_the_orchestrator_once() {
        let mut l = fixture(0);
        assert_eq!(l.claim_and_fail(0), Requeue::Queued);
        assert_eq!(l.claim_and_fail(30), Requeue::Queued);
        assert_eq!(l.claim_and_fail(150), Requeue::Failed);
        let t = l.task();
        assert_eq!(t.status, "failed");
        assert_eq!(t.due_at, Some(150 + 1800));
        assert_eq!(t.not_before, None);
        let wakes = l.replan_wakes();
        assert_eq!(wakes.len(), 1, "{wakes:?}");
        let short = t.short();
        assert!(
            wakes[0].contains(&format!("replan {short}: requeue it 1 time, or cancel it")),
            "{}",
            wakes[0]
        );
        assert!(wakes[0].contains(&format!("horch ledger requeue {short}")));
        assert!(wakes[0].contains(&format!("horch ledger cancel {short}")));
        // The pass before the limit changes nothing and wakes nobody again.
        assert_eq!(l.overdue(1000), Vec::<String>::new());
        assert_eq!(l.replan_wakes().len(), 1);
        assert_eq!(l.escalated_items(), Vec::<String>::new());
        assert_eq!(l.claim(), None, "a failed task is not claimed");
    }

    #[test]
    fn requeue_of_a_failed_task_is_the_1_replan() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.replan();
        let t = l.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.replans, 1);
        assert_eq!(t.attempt_floor, t.attempts, "the attempt count is 0 again");
        assert_eq!((t.due_at, t.not_before), (None, None));
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn second_failure_after_the_replan_escalates_with_1_needs_you_item() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.replan();
        assert_eq!(l.claim_and_fail(200), Requeue::Queued);
        assert_eq!(l.claim_and_fail(230), Requeue::Queued);
        assert_eq!(l.claim_and_fail(350), Requeue::Escalated);
        let t = l.task();
        assert_eq!(t.status, "escalated");
        assert_eq!(t.held_reason.as_deref(), Some("3 attempts failed"));
        assert_eq!(l.replan_wakes().len(), 1, "no second replan wake");
        assert_eq!(l.escalated_items(), [format!("task_escalated:{}", l.task)]);
        let moves: Vec<(String, String)> = {
            let mut stmt = l
                .store
                .conn()
                .prepare(
                    "SELECT from_state, to_state FROM events
                     WHERE entity = 'task' AND entity_id = ?1
                       AND to_state IN ('failed', 'escalated')
                     ORDER BY event_id",
                )
                .unwrap();
            stmt.query_map([&l.task], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let moves: Vec<(&str, &str)> = moves
            .iter()
            .map(|(f, t)| (f.as_str(), t.as_str()))
            .collect();
        assert_eq!(
            moves,
            [
                ("running", "failed"),
                ("running", "failed"),
                ("failed", "escalated")
            ],
            "the task reaches escalated through failed"
        );
        // The operator requeue is the exit, and it answers the item.
        l.replan();
        assert_eq!(l.task().status, "ready");
        assert_eq!(l.escalated_items(), Vec::<String>::new());
    }

    #[test]
    fn cancel_of_an_escalated_task_resolves_its_needs_you_item() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.overdue(150 + 1800);
        assert_eq!(l.escalated_items().len(), 1);
        let t = l.task();
        l.store
            .immediate(|tx| tasks::cancel(tx, &t, "the plan drops it"))
            .unwrap();
        assert_eq!(l.task().status, "cancelled");
        assert_eq!(l.escalated_items(), Vec::<String>::new());
    }

    #[test]
    fn failed_task_past_replan_limit_escalates() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        assert_eq!(l.overdue(150 + 1799), Vec::<String>::new());
        assert_eq!(l.overdue(150 + 1800), [l.task.clone()]);
        let t = l.task();
        assert_eq!(t.status, "escalated");
        assert_eq!(t.held_reason.as_deref(), Some("3 attempts failed"));
        assert_eq!(l.escalated_items(), [format!("task_escalated:{}", l.task)]);
        assert_eq!(l.overdue(9999), Vec::<String>::new(), "1 escalation");
    }

    /// The task states that the walk reaches.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum State {
        Ready,
        Running,
        Failed,
        Escalated,
    }

    /// The state of the walk: the task state and its counters.
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Node {
        state: State,
        used: i64,
        replans: i64,
    }

    /// Each move of the task from `n`: a claim, each outcome of a running
    /// attempt, and the replan and the limit of a failed task. The operator
    /// requeue of an escalated task is the exit of the walk.
    fn moves(n: &Node, policy: &Policy, run_max: i64) -> Vec<(bool, Node)> {
        let limit =
            crate::fleetd::task_execution::domain::retry::limit(policy.retry.max_attempts, run_max);
        let to = |state, used, replans| Node {
            state,
            used,
            replans,
        };
        match n.state {
            State::Ready => vec![(false, to(State::Running, n.used + 1, n.replans))],
            // An outcome that burns no attempt (asked, quota,
            // critical_open) gives the attempt back: it is no retry.
            State::Running => Outcome::ALL
                .into_iter()
                .filter(|o| o.burns_attempt())
                .map(|_| {
                    let nums = Numbers {
                        used: n.used,
                        limit,
                        replans: n.replans,
                    };
                    let next = match rung6(&nums, &policy.retry.backoff_s, 1, 0) {
                        Rung6::Retry { .. } => to(State::Ready, n.used, n.replans),
                        Rung6::Fail { .. } => to(State::Failed, n.used, n.replans),
                        Rung6::Escalate => to(State::Escalated, n.used, n.replans),
                    };
                    (true, next)
                })
                .collect(),
            State::Failed => {
                let mut out = vec![(false, to(State::Escalated, n.used, n.replans))];
                if n.replans == 0 {
                    out.push((false, to(State::Ready, 0, 1)));
                }
                out
            }
            // The operator requeue is the exit of the walk.
            State::Escalated => Vec::new(),
        }
    }

    #[test]
    fn rr6_no_sequence_of_outcomes_retries_forever() {
        for (max, run_max) in [(None, 1), (None, 3), (Some(2), 3), (Some(5), 1)] {
            let policy = Policy {
                retry: Retry {
                    max_attempts: max,
                    ..Retry::default()
                },
                ..Policy::default()
            };
            let limit = crate::fleetd::task_execution::domain::retry::limit(
                policy.retry.max_attempts,
                run_max,
            );
            // Each path from ready
            // with 0 used reaches escalated after at most 2 * limit burned
            // attempts, and no path of burned attempts comes back to a
            // node it left (no cycle).
            let start = Node {
                state: State::Ready,
                used: 0,
                replans: 0,
            };
            let mut most = 0;
            let mut stack = vec![(start, 0_i64, BTreeSet::new())];
            while let Some((n, burned, seen)) = stack.pop() {
                assert!(burned <= 2 * limit, "{n:?}: {burned} burned attempts");
                // The 6 outcomes that burn an attempt lead to the same node.
                let next: BTreeSet<(bool, Node)> =
                    moves(&n, &policy, run_max).into_iter().collect();
                if next.is_empty() {
                    assert_eq!(n.state, State::Escalated, "{n:?} has no exit");
                    most = most.max(burned);
                }
                for (burns, m) in next {
                    let mut path: BTreeSet<Node> = seen.clone();
                    assert!(path.insert(n.clone()), "{n:?} repeats: a retry loop");
                    stack.push((m, burned + i64::from(burns), path));
                }
            }
            assert_eq!(most, 2 * limit, "max {max:?}, run {run_max}");
        }
    }
}
