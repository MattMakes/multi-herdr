//! The use cases of a lease: take, renew and expire, and the lease part of
//! I1 (`docs/specs/fleetd/design.md`, "Ledger", rule 2 "Leases, not
//! claims").
//!
//! - The claim takes the lease of its attempt ([`take`]), in the
//!   transaction of the claim.
//! - Each pass renews or expires each lease ([`renew_or_expire_leases`]),
//!   after `finish_exited`: a task that a result moved out of `running` in
//!   the pass is not expired ("Order inside 1 pass").
//! - Each command of task execution that moves a task out of a state that
//!   holds a lease deletes the lease ([`release`]). The I1 check of the
//!   pass ([`check_i1`]) deletes a row that another writer leaves.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::leases_store::{self as store, LeaseRow};
use crate::fleetd::task_execution::domain::lease::{
    i1_repairs, judge, Holder, Lease, Limits, Repair, Signals, TaskLeases, Verdict, CLAIM_BASIS,
    EARNED_BASIS, I1_BASIS,
};
use crate::fleetd::task_execution::domain::task::Seen;
use crate::fleetd::worker_profiles::domain::profile::Liveness;
use crate::taskledger::model::{Actor, EventKind, TaskStatus};
use crate::taskledger::util;

/// The cause of the move of a task to `stalled` when its lease expires.
pub const LEASE_EXPIRED: &str = "lease_expired";

/// The cause of the event of each repair of the I1 check: a `note` about
/// the task, with the repair, the attempt and the fleet clock in its data.
pub const I1_REPAIRED: &str = "i1_repaired";

/// The kind of the needs-you item of a breach of I1 that no repair can
/// fix: 2 live attempts of 1 task.
pub const I1_BREACH: &str = "i1_breach";

/// What part 1 of a pass saw of 1 live attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    pub process: Seen,
    /// The age of the last wrapper heartbeat (`workers.last_checkin_at`).
    pub heartbeat_age_s: Option<i64>,
    /// The pass saw activity of the attempt.
    pub active: bool,
}

/// A lease that expired in a pass: its task moved to `stalled`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expired {
    pub task_id: String,
    pub attempt_id: String,
}

/// The `lease_s` of a claim: the design default of a profile. The claim
/// has no profile, so the first renewal of a pass sets the `lease_s` of
/// the profile of the worker.
pub fn claim_lease_s() -> i64 {
    i64::try_from(Liveness::default().lease_s).unwrap_or(i64::MAX)
}

/// The fleet clock of the `fleetd` row; 0 before the first start of
/// fleetd, which starts the clock at 0.
pub fn fleet_clock(tx: &Connection) -> Result<FleetSeconds> {
    if door::generation(tx)?.is_none() {
        return Ok(FleetSeconds(0));
    }
    clock_store::read(tx)
}

/// Attempt `attempt_id` takes the lease of task `task_id` at fleet clock
/// `clock`, for `lease_s`, and is active at `clock`. FM-14: refused while
/// the lease belongs to another attempt that is not ended.
pub fn take(
    tx: &Connection,
    clock: FleetSeconds,
    task_id: &str,
    attempt_id: &str,
    lease_s: i64,
) -> Result<()> {
    if let Some(old) = store::get(tx, task_id)? {
        if old.attempt_id != attempt_id && store::attempt_live(tx, &old.attempt_id)? {
            bail!(
                "task {task_id} is leased to live attempt {}; a second attempt of 1 task cannot \
                 hold a lease while the first is live (FM-14)",
                old.attempt_id
            );
        }
    }
    let lease = LeaseRow {
        task_id: task_id.to_string(),
        attempt_id: attempt_id.to_string(),
        expires_at: Lease::claimed(clock.0, lease_s).expires_at,
        renew_basis: CLAIM_BASIS.to_string(),
    };
    store::put(tx, &lease, &util::now())?;
    store::set_active_at(tx, attempt_id, clock.0)
}

/// Delete the lease of task `task_id`, which leaves the states that hold
/// one.
pub fn release(tx: &Connection, task_id: &str) -> Result<()> {
    store::delete(tx, task_id)?;
    Ok(())
}

/// Renew or expire each lease of a live attempt at fleet clock `clock`.
/// `facts` holds what part 1 saw of each attempt, by attempt id; an
/// attempt with no facts earns nothing. `limits` gives the numbers of a
/// worker. An expired lease moves its `running` task to `stalled` with the
/// cause [`LEASE_EXPIRED`]. Gives each expired lease.
pub fn renew_or_expire_leases(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    facts: &BTreeMap<String, Observed>,
    limits: &dyn Fn(&str) -> Limits,
) -> Result<Vec<Expired>> {
    check_pass(tx, guard)?;
    let now = util::now();
    let mut expired = Vec::new();
    for h in store::held(tx)? {
        // A task that left the held states, or a lease of an ended
        // attempt: the I1 check deletes the row.
        let Some(holder) = Holder::of(&h.task_status) else {
            continue;
        };
        if !h.attempt_live {
            continue;
        }
        let seen = facts.get(&h.attempt_id);
        let mut active_at = h.active_at;
        if seen.is_some_and(|o| o.active) {
            store::set_active_at(tx, &h.attempt_id, clock.0)?;
            active_at = Some(clock.0);
        }
        let signals = Signals {
            process: seen.map_or(Seen::Unknown, |o| o.process),
            heartbeat_age_s: seen.and_then(|o| o.heartbeat_age_s),
            active_at,
        };
        let lease = Lease {
            expires_at: h.expires_at,
        };
        match judge(holder, lease, clock.0, &signals, &limits(&h.worker_id)) {
            Verdict::Renew { expires_at } => {
                store::renew(tx, &h.task_id, expires_at, EARNED_BASIS, &now)?
            }
            Verdict::Keep => {}
            Verdict::Expire => {
                let cause = Cause {
                    actor: Actor::Horch,
                    text: LEASE_EXPIRED,
                };
                let stalled = TaskStatus::Stalled.as_str();
                transition(
                    tx,
                    guard,
                    Entity::Task,
                    &h.task_id,
                    stalled,
                    cause,
                    &NeedsYouList,
                )?;
                expired.push(Expired {
                    task_id: h.task_id,
                    attempt_id: h.attempt_id,
                });
            }
        }
    }
    Ok(expired)
}

/// The lease part of I1 at fleet clock `clock`: write the missing lease of
/// a live attempt with a full `lease_s` of its worker, delete a lease of an
/// ended attempt or of a task that is not held, and open 1 needs-you item
/// for a task with 2 live attempts. Each write or delete has 1 event with
/// the cause [`I1_REPAIRED`], and the item of a breach that ended
/// resolves. The fleetd of generation `guard` writes them.
pub fn check_i1(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    limits: &dyn Fn(&str) -> Limits,
) -> Result<()> {
    check_pass(tx, guard)?;
    let (tasks, worker_of) = task_leases(tx)?;
    let repairs = i1_repairs(&tasks);
    let lease_s = |attempt: &str| limits(&worker_of[attempt]).lease_s;
    for repair in &repairs {
        apply(tx, guard, clock, repair, &lease_s)?;
    }
    resolve_ended_breaches(tx, &repairs)
}

/// Refuse a pass with no open transaction, or a pass of a fleetd whose
/// generation `guard` is not the generation of the `fleetd` row, before
/// its first write.
fn check_pass(tx: &Connection, guard: Option<Generation>) -> Result<()> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction.into());
    }
    if let Some(held) = guard {
        let found = door::generation(tx)?;
        if found != Some(held) {
            return Err(DoorError::GenerationLost { held, found }.into());
        }
    }
    Ok(())
}

/// The attempts of 1 task that are not ended.
#[derive(Default)]
struct Unended {
    /// Their worker row is live.
    live: Vec<String>,
    /// Their worker row ended: the pass reaps them.
    ending: Vec<String>,
}

/// The lease facts of I1 of each task, and the worker of each attempt that
/// is not ended.
fn task_leases(tx: &Connection) -> Result<(Vec<TaskLeases>, BTreeMap<String, String>)> {
    let (rows, attempts) = store::i1_rows(tx)?;
    let mut by_task: BTreeMap<String, Unended> = BTreeMap::new();
    let mut worker_of = BTreeMap::new();
    for a in attempts {
        let group = by_task.entry(a.task_id).or_default();
        let list = if a.worker_live {
            &mut group.live
        } else {
            &mut group.ending
        };
        list.push(a.id.clone());
        worker_of.insert(a.id, a.worker_id);
    }
    let tasks = rows
        .into_iter()
        .map(|t| {
            let group = by_task.remove(&t.task_id).unwrap_or_default();
            TaskLeases {
                holder: Holder::of(&t.status),
                lease_attempt: t.lease_attempt,
                live_attempts: group.live,
                ending_attempts: group.ending,
                task_id: t.task_id,
            }
        })
        .collect();
    Ok((tasks, worker_of))
}

/// Do 1 repair of I1 at fleet clock `clock`. `lease_s` gives the
/// `lease_s` of the worker of an attempt.
fn apply(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    repair: &Repair,
    lease_s: &dyn Fn(&str) -> i64,
) -> Result<()> {
    match repair {
        Repair::Write {
            task_id,
            attempt_id,
        } => {
            repaired(tx, guard, clock, task_id, "write", attempt_id)?;
            let lease = LeaseRow {
                task_id: task_id.clone(),
                attempt_id: attempt_id.clone(),
                expires_at: Lease::claimed(clock.0, lease_s(attempt_id)).expires_at,
                renew_basis: I1_BASIS.to_string(),
            };
            store::put(tx, &lease, &util::now())
        }
        Repair::Delete {
            task_id,
            attempt_id,
        } => {
            repaired(tx, guard, clock, task_id, "delete", attempt_id)?;
            release(tx, task_id)
        }
        Repair::TwoLive { task_id, attempts } => {
            let text = format!(
                "task {task_id} has {} live attempts: {}. Each live attempt of a task needs \
                 its own lease, and a task has 1 lease (I1, FM-14). Stop each attempt but 1.",
                attempts.len(),
                attempts.join(", ")
            );
            needs_you::open_item(
                tx,
                &Write {
                    key: &breach_key(task_id),
                    kind: I1_BREACH,
                    entity: Entity::Task.as_str(),
                    entity_id: task_id,
                    text: &text,
                },
            )
        }
    }
}

/// The key of the needs-you item of the breach of task `task_id`.
fn breach_key(task_id: &str) -> String {
    format!("{I1_BREACH}:{task_id}")
}

/// Resolve each open needs-you item of kind [`I1_BREACH`] whose task is
/// not in `repairs` with 2 live attempts.
fn resolve_ended_breaches(tx: &Connection, repairs: &[Repair]) -> Result<()> {
    let open: BTreeSet<String> = repairs
        .iter()
        .filter_map(|r| match r {
            Repair::TwoLive { task_id, .. } => Some(breach_key(task_id)),
            _ => None,
        })
        .collect();
    for key in needs_you::open_keys(tx, I1_BREACH)? {
        if !open.contains(&key) {
            needs_you::resolve(tx, &key)?;
        }
    }
    Ok(())
}

/// Record the event of 1 repair of I1: `repair` of the lease of task
/// `task_id` for attempt `attempt_id` at fleet clock `clock`.
fn repaired(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    task_id: &str,
    repair: &str,
    attempt_id: &str,
) -> Result<()> {
    let cause = Cause {
        actor: Actor::Horch,
        text: I1_REPAIRED,
    };
    let data =
        serde_json::json!({ "repair": repair, "attempt": attempt_id, "fleet_clock": clock.0 });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Task,
        task_id,
        cause,
        &data,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rusqlite::params;

    use super::*;
    use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
    use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
    use crate::fleetd::ideas::application::needs_you;
    use crate::fleetd::task_execution::adapters::leases_store::{self as store, LeaseRow};
    use crate::fleetd::task_execution::application::commands;
    use crate::fleetd::task_execution::domain::lease::{Limits, CLAIM_BASIS, EARNED_BASIS};
    use crate::fleetd::task_execution::domain::task::Seen;
    use crate::taskledger::model::{Actor, TaskStatus, WorkerStatus};
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{self, ClaimOutcome, NewTask};
    use crate::taskledger::util::{self, Rule, Why};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    /// The generation of the `fleetd` row of the fixture.
    const GEN: Generation = Generation(1);

    const LIMITS: Limits = Limits {
        lease_s: 60,
        stall_after_s: 300,
        heartbeat_max_age_s: 30,
    };

    fn limits(_worker: &str) -> Limits {
        LIMITS
    }

    struct Fixture {
        dir: tempfile::TempDir,
        store: Store,
        run: Run,
    }

    /// A ledger with 1 run and the `fleetd` row at fleet clock `clock`.
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
            .immediate(|tx| runs::create(tx, dir.path(), &new_run))
            .unwrap();
        Fixture { dir, store, run }
    }

    impl Fixture {
        fn add_task(&mut self, ticket: &str) -> String {
            let new = NewTask::work(ticket, &format!("tickets/{ticket}.md"), "dev");
            let run = self.run.clone();
            self.store
                .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        }

        fn worker(&mut self) -> String {
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            };
            self.store
                .immediate(|tx| workers::insert(tx, &new))
                .unwrap()
        }

        /// A new worker claims the next task. Gives (task, worker).
        fn claimed(&mut self) -> (String, String) {
            let worker = self.worker();
            match self
                .store
                .immediate(|tx| tasks::claim(tx, &worker))
                .unwrap()
            {
                ClaimOutcome::Claimed(c) => (c.task.task_id, worker),
                ClaimOutcome::NoTask { .. } => panic!("no task"),
            }
        }

        fn leases(&self) -> Vec<LeaseRow> {
            self.store
                .conn()
                .prepare("SELECT task_id, attempt_id, expires_at, renew_basis FROM leases ORDER BY task_id")
                .unwrap()
                .query_map([], |r| {
                    Ok(LeaseRow {
                        task_id: r.get(0)?,
                        attempt_id: r.get(1)?,
                        expires_at: r.get(2)?,
                        renew_basis: r.get(3)?,
                    })
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }

        fn active_at(&self, attempt: &str) -> Option<i64> {
            self.store
                .conn()
                .query_row(
                    "SELECT active_at FROM attempts WHERE id = ?1",
                    [attempt],
                    |r| r.get(0),
                )
                .unwrap()
        }

        fn status(&self, task: &str) -> String {
            tasks::get(self.store.conn(), task).unwrap().status
        }

        fn pass(&mut self, clock: i64, facts: &BTreeMap<String, Observed>) -> Vec<Expired> {
            self.store
                .immediate(|tx| {
                    renew_or_expire_leases(tx, Some(GEN), FleetSeconds(clock), facts, &limits)
                })
                .unwrap()
        }

        fn check_i1(&mut self, clock: i64) {
            self.store
                .immediate(|tx| check_i1(tx, Some(GEN), FleetSeconds(clock), &limits))
                .unwrap();
        }

        /// The `data_json` of each `i1_repaired` event of `task`, oldest
        /// first. Each is a `note` about the task row, by horch.
        fn i1_events(&self, task: &str) -> Vec<serde_json::Value> {
            self.store
                .conn()
                .prepare(
                    "SELECT kind, entity, actor, data_json FROM events
                     WHERE cause = ?1 AND entity_id = ?2 ORDER BY event_id",
                )
                .unwrap()
                .query_map(params![I1_REPAIRED, task], |r| {
                    let head: (String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
                    assert_eq!(head, ("note".into(), "task".into(), "horch".into()));
                    let data: String = r.get(3)?;
                    Ok(serde_json::from_str(&data).unwrap())
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }

        /// `horch ledger finish`, another writer, moves `task` to `verifying`.
        fn finish(&mut self, task: &str) {
            self.store
                .immediate(|tx| {
                    util::transition(
                        tx,
                        Entity::Task,
                        task,
                        TaskStatus::Verifying.as_str(),
                        Actor::Worker,
                        Why::Rule(Rule::Finish, "test"),
                    )
                })
                .unwrap();
        }
    }

    fn earning() -> Observed {
        Observed {
            process: Seen::Alive,
            heartbeat_age_s: Some(5),
            active: true,
        }
    }

    #[test]
    fn claim_writes_1_lease_on_the_fleet_clock() {
        let mut l = fixture(1_000);
        l.add_task("a");
        let (task, _) = l.claimed();
        let attempt = format!("{task}/A1");
        assert_eq!(
            l.leases(),
            vec![LeaseRow {
                task_id: task.clone(),
                attempt_id: attempt.clone(),
                expires_at: 1_000 + claim_lease_s(),
                renew_basis: CLAIM_BASIS.into(),
            }]
        );
        assert_eq!(claim_lease_s(), 120, "the design default of lease_s");
        assert_eq!(l.active_at(&attempt), Some(1_000));
    }

    #[test]
    fn claim_with_no_fleetd_row_writes_its_lease_at_fleet_clock_0() {
        let mut l = fixture(0);
        l.store.conn().execute("DELETE FROM fleetd", []).unwrap();
        l.add_task("a");
        let (task, _) = l.claimed();
        assert_eq!(l.leases()[0].expires_at, claim_lease_s());
        assert_eq!(l.active_at(&format!("{task}/A1")), Some(0));
    }

    #[test]
    fn first_renewal_moves_the_lease_to_the_lease_s_of_the_profile() {
        let mut l = fixture(1_000);
        l.add_task("a");
        let (task, _) = l.claimed();
        let attempt = format!("{task}/A1");
        let facts = BTreeMap::from([(attempt.clone(), earning())]);
        assert_eq!(l.pass(1_005, &facts), vec![]);
        let lease = &l.leases()[0];
        assert_eq!(lease.expires_at, 1_005 + LIMITS.lease_s);
        assert_eq!(lease.renew_basis, EARNED_BASIS);
        assert_eq!(l.active_at(&attempt), Some(1_005), "the pass saw activity");
    }

    #[test]
    fn pass_with_no_earned_signal_keeps_the_lease() {
        let mut l = fixture(1_000);
        l.add_task("a");
        let (task, _) = l.claimed();
        let attempt = format!("{task}/A1");
        let idle = Observed {
            active: false,
            ..earning()
        };
        let facts = BTreeMap::from([(attempt.clone(), idle)]);
        // The last activity is 305 s before the pass: past stall_after_s.
        l.store
            .conn()
            .execute(
                "UPDATE attempts SET active_at = 700 WHERE id = ?1",
                [&attempt],
            )
            .unwrap();
        let before = l.leases();
        assert_eq!(l.pass(1_005, &facts), vec![]);
        assert_eq!(l.leases(), before, "an idle attempt earns no renewal");
        assert_eq!(l.active_at(&attempt), Some(700));
    }

    #[test]
    fn expired_lease_moves_the_task_to_stalled_in_the_same_pass() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        let attempt = format!("{task}/A1");
        assert_eq!(l.pass(claim_lease_s(), &BTreeMap::new()), vec![]);
        assert_eq!(
            l.status(&task),
            "running",
            "the lease holds up to its expiry"
        );
        let expired = l.pass(claim_lease_s() + 1, &BTreeMap::new());
        assert_eq!(
            expired,
            vec![Expired {
                task_id: task.clone(),
                attempt_id: attempt.clone()
            }]
        );
        assert_eq!(l.status(&task), "stalled");
        let cause: String = l
            .store
            .conn()
            .query_row(
                "SELECT cause FROM events WHERE entity = 'task' AND entity_id = ?1 AND to_state = 'stalled'",
                [&task],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cause, LEASE_EXPIRED);
        assert_eq!(l.leases().len(), 1, "a stalled task keeps its lease");
        assert_eq!(
            l.pass(claim_lease_s() + 500, &BTreeMap::new()),
            vec![],
            "a stalled task does not expire again"
        );
    }

    #[test]
    fn task_that_left_running_in_the_pass_is_not_expired() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        // Earlier in the same pass, the result moved the task on.
        l.finish(&task);
        assert_eq!(l.pass(10_000, &BTreeMap::new()), vec![]);
        assert_eq!(l.status(&task), "verifying");
    }

    #[test]
    fn waiting_quota_task_keeps_a_paused_lease() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        l.store
            .immediate(|tx| {
                util::transition(
                    tx,
                    Entity::Task,
                    &task,
                    TaskStatus::WaitingQuota.as_str(),
                    Actor::Horch,
                    Why::Rule(Rule::Finish, "test"),
                )
            })
            .unwrap();
        l.store
            .conn()
            .execute("UPDATE attempts SET state = 'ended'", [])
            .unwrap();
        assert_eq!(l.pass(100_000, &BTreeMap::new()), vec![]);
        l.check_i1(100_000);
        assert_eq!(l.status(&task), "waiting_quota");
        assert_eq!(l.leases().len(), 1);
    }

    #[test]
    fn fm14_second_lease_for_1_task_is_refused() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, worker) = l.claimed();
        let second = format!("{task}/A2");
        l.store
            .conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at)
                 VALUES (?1, ?2, ?3, 'live', 'now')",
                params![second, task, worker],
            )
            .unwrap();
        let err = l
            .store
            .immediate(|tx| take(tx, FleetSeconds(5), &task, &second, 60))
            .unwrap_err();
        assert!(format!("{err:#}").contains("FM-14"), "{err:#}");
        assert_eq!(l.leases().len(), 1);
        assert_eq!(l.leases()[0].attempt_id, format!("{task}/A1"));
        // Once the first attempt ends, the second takes the lease.
        l.store
            .conn()
            .execute(
                "UPDATE attempts SET state = 'ended' WHERE id = ?1",
                [format!("{task}/A1")],
            )
            .unwrap();
        l.store
            .immediate(|tx| take(tx, FleetSeconds(5), &task, &second, 60))
            .unwrap();
        assert_eq!(l.leases()[0].attempt_id, second);
        assert_eq!(l.leases()[0].expires_at, 65);
    }

    #[test]
    fn a_command_that_moves_the_task_out_deletes_its_lease() {
        let mut l = fixture(0);
        l.add_task("a");
        l.add_task("b");
        let (retried, _) = l.claimed();
        let (held, _) = l.claimed();
        l.store
            .immediate(|tx| {
                commands::schedule_retry(tx, &retried, "test")?;
                commands::escalate_task(tx, &held)
            })
            .unwrap();
        assert_eq!(l.status(&retried), "ready");
        assert_eq!(l.status(&held), "escalated");
        assert_eq!(l.leases(), vec![]);
    }

    #[test]
    fn i1_live_attempt_with_no_lease_is_repaired_with_an_event() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        store::delete(l.store.conn(), &task).unwrap();
        l.check_i1(50);
        assert_eq!(
            l.leases(),
            vec![LeaseRow {
                task_id: task.clone(),
                attempt_id: format!("{task}/A1"),
                expires_at: 50 + LIMITS.lease_s,
                renew_basis: I1_BASIS.into(),
            }]
        );
        let attempt = format!("{task}/A1");
        assert_eq!(
            l.i1_events(&task),
            vec![serde_json::json!({ "repair": "write", "attempt": attempt, "fleet_clock": 50 })]
        );
    }

    #[test]
    fn i1_lease_of_a_task_that_is_not_held_is_deleted() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        l.finish(&task);
        assert_eq!(l.leases().len(), 1, "another writer leaves a stale row");
        l.check_i1(10);
        assert_eq!(l.leases(), vec![]);
        let attempt = format!("{task}/A1");
        assert_eq!(
            l.i1_events(&task),
            vec![serde_json::json!({ "repair": "delete", "attempt": attempt, "fleet_clock": 10 })]
        );
        l.check_i1(15);
        assert_eq!(
            l.i1_events(&task).len(),
            1,
            "a repaired task records no more"
        );
    }

    #[test]
    fn a_pass_of_another_generation_writes_nothing() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        let other = Some(Generation(GEN.0 + 1));
        let facts = BTreeMap::new();
        let expired = l
            .store
            .immediate(|tx| renew_or_expire_leases(tx, other, FleetSeconds(500), &facts, &limits));
        assert!(expired.is_err(), "{expired:?}");
        assert_eq!(l.status(&task), "running");
        store::delete(l.store.conn(), &task).unwrap();
        let repaired = l
            .store
            .immediate(|tx| check_i1(tx, other, FleetSeconds(5), &limits));
        assert!(repaired.is_err());
        assert_eq!(l.leases(), vec![]);
        assert_eq!(l.i1_events(&task), Vec::<serde_json::Value>::new());
    }

    /// A stale generation is refused before its first write, also on the
    /// paths that move no row: a renewal, an activity mark and a breach
    /// item. Each write runs in an open transaction that commits on `Ok`.
    #[test]
    fn a_stale_generation_is_refused_before_a_renewal_an_activity_mark_or_a_breach_item() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        let attempt = format!("{task}/A1");
        let other = Some(Generation(GEN.0 + 1));
        let dead_but_active = Observed {
            process: Seen::Dead,
            ..earning()
        };
        for facts in [
            BTreeMap::from([(attempt.clone(), earning())]),
            BTreeMap::from([(attempt.clone(), dead_but_active)]),
        ] {
            let out = l.store.immediate(|tx| {
                renew_or_expire_leases(tx, other, FleetSeconds(5), &facts, &limits)
            });
            assert!(out.is_err(), "{out:?}");
        }
        let worker = l.worker();
        l.store
            .conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at)
                 VALUES (?1, ?2, ?3, 'live', 'now')",
                params![format!("{task}/A2"), task, worker],
            )
            .unwrap();
        let out = l
            .store
            .immediate(|tx| check_i1(tx, other, FleetSeconds(5), &limits));
        assert!(out.is_err(), "{out:?}");
        assert_eq!(breaches(&l), Vec::<String>::new());
        let no_tx = renew_or_expire_leases(
            l.store.conn(),
            Some(GEN),
            FleetSeconds(5),
            &BTreeMap::new(),
            &limits,
        );
        assert!(no_tx.is_err(), "a pass writes in 1 open transaction");
        assert!(check_i1(l.store.conn(), Some(GEN), FleetSeconds(5), &limits).is_err());
    }

    #[test]
    fn i1_lease_of_an_ended_attempt_is_deleted() {
        let mut l = fixture(0);
        l.add_task("a");
        l.claimed();
        l.store
            .conn()
            .execute("UPDATE attempts SET state = 'ended'", [])
            .unwrap();
        l.check_i1(10);
        assert_eq!(l.leases(), vec![]);
    }

    /// The open needs-you items of kind [`I1_BREACH`].
    fn breaches(l: &Fixture) -> Vec<String> {
        needs_you::open_items(l.store.conn())
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == I1_BREACH)
            .map(|i| i.entity_id)
            .collect()
    }

    #[test]
    fn a_handoff_then_a_claim_gives_no_i1_breach_item() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, worker) = l.claimed();
        let path = l.dir.path().join("handoff.md");
        std::fs::write(&path, "next steps").unwrap();
        l.store
            .immediate(|tx| tasks::handoff(tx, &worker, path.to_str().unwrap()))
            .unwrap();
        assert_eq!(l.status(&task), "ready");
        assert_eq!(l.leases(), vec![], "a ready task holds no lease");
        let (again, _) = l.claimed();
        assert_eq!(again, task);
        l.check_i1(10);
        assert_eq!(breaches(&l), Vec::<String>::new());
        let leases = l.leases();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].attempt_id, format!("{task}/A2"));
    }

    #[test]
    fn a_requeue_then_a_claim_gives_no_i1_breach_item() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, worker) = l.claimed();
        l.store
            .immediate(|tx| {
                workers::end(tx, &worker, WorkerStatus::Incomplete, "reaped", None)?;
                tasks::requeue(tx, &task, "worker ended")
            })
            .unwrap();
        assert_eq!(l.leases(), vec![], "a ready task holds no lease");
        // Past the retry backoff of 30 s on the fleet clock (rung 6).
        l.store
            .conn()
            .execute("UPDATE fleetd SET fleet_clock_s = 30 WHERE id = 1", [])
            .unwrap();
        let (again, _) = l.claimed();
        assert_eq!(again, task);
        l.check_i1(40);
        assert_eq!(breaches(&l), Vec::<String>::new());
        assert_eq!(l.leases()[0].attempt_id, format!("{task}/A2"));
    }

    #[test]
    fn i1_two_live_attempts_of_1_task_open_1_needs_you_item() {
        let mut l = fixture(0);
        l.add_task("a");
        let (task, _) = l.claimed();
        let other = l.worker();
        l.store
            .conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at)
                 VALUES (?1, ?2, ?3, 'live', 'now')",
                params![format!("{task}/A2"), task, other],
            )
            .unwrap();
        let before = l.leases();
        l.check_i1(10);
        l.check_i1(15);
        assert_eq!(breaches(&l), vec![task.clone()]);
        assert_eq!(l.leases(), before, "no repair is safe");
        // The operator stops the second attempt: the item resolves.
        l.store
            .conn()
            .execute(
                "UPDATE attempts SET state = 'ended' WHERE id = ?1",
                [format!("{task}/A2")],
            )
            .unwrap();
        l.check_i1(20);
        assert_eq!(breaches(&l), Vec::<String>::new());
    }
}
