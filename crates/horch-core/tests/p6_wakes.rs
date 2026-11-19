//! The wakes of today's orchestrator go to their new owner: fleetd acts,
//! the sub-orchestrator of the feature gets a doorbell, and you get the
//! needs-you list (docs/specs/fleetd/design.md, FM-16, OR-3;
//! docs/tickets/fleetd-p6/46-wakes.md).
//!
//! Each test makes its own temporary ledger. No test reads the clock, the
//! load or the limits of the machine, and no test starts a process.

use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::wakes::{self, NewOwner, Routed, Today};
use horch_core::taskledger::watchdog::Checked;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

struct World {
    _dir: tempfile::TempDir,
    store: Store,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        World { _dir: dir, store }
    }

    /// A new run. Its first task gives it the feature `F-<run rowid>`.
    fn run(&mut self, slug: &str) -> Run {
        let root = self._dir.path().join(slug);
        let new = NewRun {
            slug: slug.into(),
            ..Default::default()
        };
        self.store
            .immediate(|tx| {
                let run = runs::create(tx, &root, &new)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap()
    }

    /// A new task `title` of `run`, blocked by `blocked_by`.
    fn task(&mut self, run: &Run, title: &str, blocked_by: &[&str]) -> String {
        let mut t = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        t.blocked_by = blocked_by.iter().map(|b| b.to_string()).collect();
        self.store
            .immediate(|tx| Ok(tasks::add(tx, run, &t, Actor::Orchestrator)?.task_id))
            .unwrap()
    }

    fn move_task(&mut self, task: &str, to: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Task,
                    task,
                    to,
                    cause("test"),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn sql(&self, sql: &str, arg: &str) {
        self.store.conn().execute(sql, [arg]).unwrap();
    }

    fn feature_of(&self, task: &str) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT i.feature_id FROM tasks t JOIN ideas i ON i.id = t.idea_id
                 WHERE t.task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn open_items(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT key, text FROM needs_you WHERE state = 'open' ORDER BY key")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn open_all(&mut self, items: &[wakes::Item]) {
        self.store
            .immediate(|tx| {
                for i in items {
                    i.open(tx)?;
                }
                Ok(())
            })
            .unwrap();
    }

    /// A worker of `run` on `task`.
    fn worker(&mut self, run: &Run, task: &str) -> String {
        let new = NewWorker {
            run_id: run.run_id.clone(),
            role: Some("opus-1".into()),
            persona: "dev".into(),
            target_task_id: Some(task.to_string()),
            ..Default::default()
        };
        let id = self
            .store
            .immediate(|tx| workers::insert(tx, &new))
            .unwrap();
        self.store
            .conn()
            .execute(
                "UPDATE workers SET task_id = ?1 WHERE worker_id = ?2",
                [task, id.as_str()],
            )
            .unwrap();
        id
    }
}

fn checked(worker_id: &str, verdict: &str) -> Checked {
    Checked {
        worker_id: worker_id.into(),
        role: Some("opus-1".into()),
        task: Some("T-1".into()),
        verdict: verdict.into(),
        round: 1,
        note: "no new output".into(),
        changed: true,
        first_stall: true,
        pane_id: Some("w1:p1".into()),
    }
}

#[test]
fn each_wake_of_today_has_1_new_owner_and_none_is_an_orchestrator() {
    assert_eq!(Today::ALL.len(), 13);
    for t in Today::ALL {
        let owner = t.owner();
        assert!(
            matches!(
                owner,
                NewOwner::Fleetd | NewOwner::You | NewOwner::SubOrchestrator
            ),
            "{t:?}"
        );
    }
}

#[test]
fn startable_wake_goes_to_fleetd() {
    assert_eq!(Today::Startable.owner(), NewOwner::Fleetd);
}

#[test]
fn spawn_advice_goes_to_fleetd() {
    assert_eq!(Today::SpawnAdvice.owner(), NewOwner::Fleetd);
}

#[test]
fn reviews_in_wake_goes_to_fleetd() {
    assert_eq!(Today::ReviewsIn.owner(), NewOwner::Fleetd);
}

#[test]
fn reaped_wake_goes_to_fleetd() {
    assert_eq!(Today::Reaped.owner(), NewOwner::Fleetd);
}

#[test]
fn preclaim_wakes_go_to_fleetd() {
    assert_eq!(Today::Preclaim.owner(), NewOwner::Fleetd);
}

#[test]
fn stuck_wake_is_1_needs_you_item_of_the_feature() {
    assert_eq!(Today::Stuck.owner(), NewOwner::You);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let _b = w.task(&run, "b", &[&a]);
    w.move_task(&a, "cancelled");
    let feature = w.feature_of(&a);
    let item = wakes::stuck_item(w.store.conn(), &run)
        .unwrap()
        .expect("the run is stuck");
    assert_eq!(item.key, format!("stuck:{feature}"));
    assert!(item.text.contains("stuck"), "{}", item.text);
    // A repeat pass writes the same key: 1 item.
    w.open_all(&[item.clone(), item]);
    assert_eq!(w.open_items().len(), 1);
}

#[test]
fn held_wake_is_1_needs_you_item_of_the_task() {
    assert_eq!(Today::Held.owner(), NewOwner::You);
    assert_eq!(Today::HeldQuestion.owner(), NewOwner::You);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    w.sql(
        "UPDATE tasks SET status = 'escalated', held_reason = 'attempt_limit' WHERE task_id = ?1",
        &a,
    );
    let t = tasks::get(w.store.conn(), &a).unwrap();
    let item = wakes::held_item(w.store.conn(), &t)
        .unwrap()
        .expect("the task is held");
    assert_eq!(item.key, format!("held:{a}"));
    assert!(item.text.contains("horch task requeue"), "{}", item.text);
    assert!(!item.text.contains("horch ledger"), "{}", item.text);
    w.open_all(&[item]);
    assert_eq!(w.open_items().len(), 1);
}

#[test]
fn blocker_wake_is_1_needs_you_item_of_the_blocker() {
    assert_eq!(Today::Blocker.owner(), NewOwner::You);
    assert_eq!(Today::Dependents.owner(), NewOwner::You);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let b = w.task(&run, "b", &[&a]);
    // No item while the blocker is live.
    assert!(wakes::blocker_item(w.store.conn(), &a).unwrap().is_none());
    w.move_task(&a, "cancelled");
    let item = wakes::blocker_item(w.store.conn(), &a)
        .unwrap()
        .expect("b waits for a cancelled task");
    assert_eq!(item.key, format!("blocker:{a}"));
    let short_b = tasks::get(w.store.conn(), &b).unwrap().short();
    assert!(item.text.contains(&short_b), "{}", item.text);
    assert!(!item.text.contains("horch ledger"), "{}", item.text);
}

#[test]
fn settled_wake_is_a_needs_you_item_only_for_a_ready_feature() {
    assert_eq!(Today::Settled.owner(), NewOwner::You);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let feature = w.feature_of(&a);
    w.move_task(&a, "cancelled");
    // The run is settled, but the feature is `open`: no item.
    assert!(wakes::settled_item(w.store.conn(), &run).unwrap().is_none());
    w.sql(
        "UPDATE features SET state = 'ready' WHERE id = ?1",
        &feature,
    );
    let item = wakes::settled_item(w.store.conn(), &run)
        .unwrap()
        .expect("a ready feature");
    assert_eq!(item.key, format!("feature_ready:{feature}"));
    assert!(item.text.contains(&feature), "{}", item.text);
}

#[test]
fn lost_wakes_are_the_needs_you_items_of_the_reconcile_pass() {
    assert_eq!(Today::Lost.owner(), NewOwner::You);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let _b = w.task(&run, "b", &[&a]);
    w.move_task(&a, "cancelled");
    let items = wakes::lost_items(w.store.conn(), &run).unwrap();
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, vec![format!("blocker:{a}").as_str()]);
}

#[test]
fn verdict_wake_rings_the_sub_orchestrator_of_the_feature_of_its_task() {
    assert_eq!(Today::Verdict.owner(), NewOwner::SubOrchestrator);
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let worker = w.worker(&run, &a);
    let feature = w.feature_of(&a);
    let routed = wakes::verdict_doorbell(w.store.conn(), &checked(&worker, "no_progress"))
        .unwrap()
        .expect("a first stall");
    match routed {
        Routed::Doorbell { feature_id, text } => {
            assert_eq!(feature_id, feature);
            assert!(text.contains("no progress"), "{text}");
            assert!(!text.contains("[orchestrator]"), "{text}");
        }
        other => panic!("not a doorbell: {other:?}"),
    }
    // Another verdict wakes nobody.
    assert!(
        wakes::verdict_doorbell(w.store.conn(), &checked(&worker, "ok"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn verdict_of_a_task_with_no_feature_goes_to_you() {
    let mut w = World::new();
    let run = w.run("r1");
    let a = w.task(&run, "a", &[]);
    let worker = w.worker(&run, &a);
    w.sql("UPDATE tasks SET idea_id = NULL WHERE task_id = ?1", &a);
    let routed = wakes::verdict_doorbell(w.store.conn(), &checked(&worker, "suspect"))
        .unwrap()
        .expect("a suspect");
    match routed {
        Routed::NeedsYou(item) => assert_eq!(item.key, format!("stall:{a}")),
        other => panic!("not a needs-you item: {other:?}"),
    }
}

#[test]
fn wake_of_one_feature_never_reaches_another() {
    let mut w = World::new();
    let run_1 = w.run("r1");
    let run_2 = w.run("r2");
    let a = w.task(&run_1, "a", &[]);
    let b = w.task(&run_2, "b", &[]);
    let (f1, f2) = (w.feature_of(&a), w.feature_of(&b));
    assert_ne!(f1, f2);
    let wa = w.worker(&run_1, &a);
    let wb = w.worker(&run_2, &b);
    for (worker, own, other) in [(&wa, &f1, &f2), (&wb, &f2, &f1)] {
        for verdict in ["no_progress", "suspect"] {
            let Some(Routed::Doorbell { feature_id, .. }) =
                wakes::verdict_doorbell(w.store.conn(), &checked(worker, verdict)).unwrap()
            else {
                panic!("{verdict} of {worker}: no doorbell");
            };
            assert_eq!(&feature_id, own);
            assert_ne!(&feature_id, other);
        }
    }
}
