//! The use case of the stall ladder: write the rows of 1 decision of
//! [`decide`](crate::fleetd::task_execution::domain::stall::decide) and
//! return its effects.
//!
//! The rows go through `transition()`, `record()` and the needs-you list
//! in the caller's transaction. No effect occurs here: a stop or a
//! doorbell is returned, and the pass does it after the commit.
//!
//! - A doorbell: 1 event of kind `doorbell` with the fleet clock.
//! - An end: the attempt ends with its outcome, and its live worker row
//!   ends `incomplete`. `stalled` moves the task to `stalled`, and `asked`
//!   moves it to `blocked_on_user`. No end burns an attempt here: rung 6
//!   (ticket 10) acts on `stalled` and `no_result`, and the quota rule
//!   (ticket 11) acts on `quota`.
//! - Rung 4: the end `stalled`, and 1 needs-you item `dialog:<attempt>`
//!   with the screen excerpt.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::domain::stall::{Decision, Outcome};
use crate::fleetd::task_execution::domain::task::{
    AttemptRow, AttemptState, ProcessScope, RowRef, Stop, Target,
};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerStatus};
use crate::taskledger::{util, workers};

/// The kind of the needs-you item of an attempt that waits on a dialog.
const DIALOG: &str = "dialog";

/// 1 effect of the ladder, to do after the commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Stop the process group of an ended attempt.
    Stop(Stop),
    /// Type the constant `DOORBELL` into the pane `pane`.
    Doorbell { pane: String },
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Write the rows of `decision` for the live attempt `a` at the fleet
/// clock `clock`, in `tx`, and return its effects. The fleetd of
/// generation `guard` writes them.
pub fn apply(
    tx: &Connection,
    guard: Option<Generation>,
    a: &AttemptRow,
    clock: i64,
    decision: &Decision,
) -> Result<Vec<Effect>> {
    if tx.is_autocommit() {
        bail!("the stall ladder writes its rows in 1 open transaction of the ledger");
    }
    match decision {
        Decision::None => Ok(Vec::new()),
        Decision::Doorbell { pane, rung } => {
            let text = format!("rung {rung}: the pane is idle and the task has no result");
            let data = serde_json::json!({ "fleet_clock": clock, "rung": rung });
            record(
                tx,
                EventKind::Doorbell,
                Entity::Attempt,
                &a.id,
                cause(&text),
                &data,
            )?;
            Ok(vec![Effect::Doorbell { pane: pane.clone() }])
        }
        Decision::End { outcome, stop } => {
            end(tx, guard, a, *outcome)?;
            Ok(if *stop { stop_of(a) } else { Vec::new() })
        }
        Decision::NeedsYou { excerpt } => {
            end(tx, guard, a, Outcome::Stalled)?;
            let key = format!("{DIALOG}:{}", a.id);
            let text = format!(
                "Attempt {} waited on a dialog, so fleetd ended it as stalled. A profile \
                 pre-approves what a task needs, so the dialog is a defect of the profile. \
                 The screen:\n{excerpt}",
                a.id
            );
            needs_you::open_item(
                tx,
                &Write {
                    key: &key,
                    kind: DIALOG,
                    entity: Entity::Attempt.as_str(),
                    entity_id: &a.id,
                    text: &text,
                },
            )?;
            Ok(stop_of(a))
        }
    }
}

/// End the attempt `a` with `outcome`, end its live worker row, and move
/// its task for `stalled` and `asked`.
fn end(tx: &Connection, guard: Option<Generation>, a: &AttemptRow, outcome: Outcome) -> Result<()> {
    let text = format!("the stall ladder ended it: {}", outcome.as_str());
    store::set_ended(tx, &a.id, outcome.as_str(), &util::now())?;
    transition(
        tx,
        guard,
        Entity::Attempt,
        &a.id,
        AttemptState::Ended.as_str(),
        cause(&text),
        &NeedsYouList,
    )?;
    let reason = format!("{}: attempt {}", outcome.as_str(), a.id);
    workers::end(tx, &a.worker_id, WorkerStatus::Incomplete, &reason, None)?;
    let to = match outcome {
        Outcome::Stalled => TaskStatus::Stalled,
        Outcome::Asked => TaskStatus::BlockedOnUser,
        Outcome::Quota | Outcome::NoResult => return Ok(()),
    };
    if let Some(task) = &a.task_id {
        transition(
            tx,
            guard,
            Entity::Task,
            task,
            to.as_str(),
            cause(&text),
            &NeedsYouList,
        )?;
    }
    Ok(())
}

/// The stop of the process group of `a`. An attempt with no recorded
/// process, or with no start time, has none: a pid alone can name a later
/// program (S8).
fn stop_of(a: &AttemptRow) -> Vec<Effect> {
    let Some(p) = a.process else {
        return Vec::new();
    };
    let Some(pid_start) = p.pid_start else {
        return Vec::new();
    };
    vec![Effect::Stop(Stop {
        row: RowRef::Attempt(a.id.clone()),
        target: Target::Process {
            pid: p.pid,
            pid_start,
            scope: ProcessScope::LeaderGroup,
        },
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::task_execution::domain::task::Recorded;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, ClaimOutcome, NewTask};
    use crate::taskledger::workers::NewWorker;
    use crate::taskledger::{status, Store};

    const AGENT: Recorded = Recorded {
        pid: 4_242,
        pid_start: Some(77),
    };

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        task: String,
        worker: String,
    }

    /// A run with 1 task that 1 worker in pane `w0:p1` claimed.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (task, worker) = store
            .immediate(|tx| {
                let new_run = NewRun {
                    slug: "r1".into(),
                    ..Default::default()
                };
                let run = runs::create(tx, dir.path(), &new_run)?;
                let new = NewTask::work("a", "tickets/a.md", "dev");
                tasks::add(tx, &run, &new, Actor::Orchestrator)?;
                let w = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "dev".into(),
                    pane_id: Some("w0:p1".into()),
                    ..Default::default()
                };
                let id = workers::insert(tx, &w)?;
                workers::supervisor_started(tx, &id, 4_241, Some(76))?;
                workers::agent_started(tx, &id, AGENT.pid, AGENT.pid_start)?;
                let ClaimOutcome::Claimed(c) = tasks::claim(tx, &id)? else {
                    panic!("no task");
                };
                Ok((c.task.task_id, id))
            })
            .unwrap();
        Fixture {
            _dir: dir,
            store,
            task,
            worker,
        }
    }

    impl Fixture {
        fn attempt(&self) -> AttemptRow {
            let mut rows = store::attempts_of_worker(self.store.conn(), &self.worker).unwrap();
            assert_eq!(rows.len(), 1);
            rows.remove(0)
        }

        fn apply(&mut self, decision: &Decision) -> Vec<Effect> {
            let a = self.attempt();
            self.store
                .immediate(|tx| apply(tx, None, &a, 1_300, decision))
                .unwrap()
        }

        fn outcome(&self) -> (String, Option<String>) {
            self.store
                .conn()
                .query_row(
                    "SELECT state, outcome FROM attempts WHERE worker_id = ?1",
                    [&self.worker],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }

        fn task(&self) -> tasks::Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        fn worker_status(&self) -> String {
            workers::get(self.store.conn(), &self.worker)
                .unwrap()
                .status
        }
    }

    fn group_stop(a: &AttemptRow) -> Effect {
        Effect::Stop(Stop {
            row: RowRef::Attempt(a.id.clone()),
            target: Target::Process {
                pid: AGENT.pid,
                pid_start: AGENT.pid_start.unwrap(),
                scope: ProcessScope::LeaderGroup,
            },
        })
    }

    #[test]
    fn waiting_approval_ends_the_attempt_and_opens_1_dialog_item_with_the_excerpt() {
        let mut f = fixture();
        let a = f.attempt();
        let effects = f.apply(&Decision::NeedsYou {
            excerpt: "Do you trust the files in this folder? (y/n)".into(),
        });
        assert_eq!(effects, vec![group_stop(&a)]);
        assert_eq!(f.outcome(), ("ended".into(), Some("stalled".into())));
        assert_eq!(f.task().status, "stalled");
        let items = needs_you::open_items(f.store.conn()).unwrap();
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].key, format!("dialog:{}", a.id));
        assert_eq!(items[0].kind, "dialog");
        assert!(items[0]
            .text
            .ends_with("Do you trust the files in this folder? (y/n)"));
    }

    #[test]
    fn asked_moves_the_task_to_blocked_on_user_and_keeps_the_attempt_count() {
        let mut f = fixture();
        let before = f.task();
        let effects = f.apply(&Decision::End {
            outcome: Outcome::Asked,
            stop: false,
        });
        assert_eq!(effects, Vec::new());
        assert_eq!(f.outcome(), ("ended".into(), Some("asked".into())));
        let after = f.task();
        assert_eq!(after.status, "blocked_on_user");
        assert_eq!(
            (after.attempts, after.attempt_floor),
            (before.attempts, before.attempt_floor)
        );
    }

    #[test]
    fn stalled_attempt_returns_a_stop_of_its_process_group() {
        let mut f = fixture();
        let a = f.attempt();
        let effects = f.apply(&Decision::End {
            outcome: Outcome::Stalled,
            stop: true,
        });
        assert_eq!(effects, vec![group_stop(&a)]);
        assert_eq!(f.outcome(), ("ended".into(), Some("stalled".into())));
        assert_eq!(f.task().status, "stalled");
        assert_eq!(f.worker_status(), "incomplete");
    }

    #[test]
    fn quota_and_no_result_end_the_attempt_and_leave_the_task_to_its_rule() {
        for (outcome, stop) in [(Outcome::Quota, true), (Outcome::NoResult, false)] {
            let mut f = fixture();
            let a = f.attempt();
            let effects = f.apply(&Decision::End { outcome, stop });
            let want = if stop {
                vec![group_stop(&a)]
            } else {
                Vec::new()
            };
            assert_eq!(effects, want);
            assert_eq!(f.outcome(), ("ended".into(), Some(outcome.as_str().into())));
            assert_eq!(f.task().status, "running");
        }
    }

    #[test]
    fn doorbell_records_1_event_and_keeps_the_attempt_live() {
        let mut f = fixture();
        let effects = f.apply(&Decision::Doorbell {
            pane: "w0:p1".into(),
            rung: 1,
        });
        assert_eq!(
            effects,
            vec![Effect::Doorbell {
                pane: "w0:p1".into()
            }]
        );
        assert_eq!(f.outcome(), ("live".into(), None));
        let rung_events: Vec<_> = status::events(f.store.conn(), "worker_id", &f.worker)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "doorbell")
            .collect();
        assert_eq!(rung_events.len(), 1, "{rung_events:?}");
    }

    #[test]
    fn an_attempt_with_no_start_time_has_no_stop() {
        let mut a = fixture().attempt();
        a.process = Some(Recorded {
            pid: 1,
            pid_start: None,
        });
        assert_eq!(stop_of(&a), Vec::new());
        a.process = None;
        assert_eq!(stop_of(&a), Vec::new());
    }

    #[test]
    fn apply_refuses_autocommit() {
        let f = fixture();
        let a = f.attempt();
        assert!(apply(f.store.conn(), None, &a, 0, &Decision::None).is_err());
    }
}
