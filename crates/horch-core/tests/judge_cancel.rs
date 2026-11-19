//! The cancel of a judge pair (docs/specs/fleetd/design.md, "Supervision,
//! liveness and reconciliation", A cancel; docs/tickets/fleetd-p6/
//! 43-judge-cancel.md).
//!
//! A cancel never leaves a judgment task with no judge task:
//!
//! - The cancel of a judged task cancels its judge task in the same
//!   transaction. With a live attempt, the judge task waits for the stop
//!   of its process, as each cancel does.
//! - The cancel of a judge task while its judged task is not `cancelled`
//!   is refused with the reason `cancel the judged task <id>; its judge
//!   follows`.
//! - The cancel of each task of an idea cancels the pair, in each order of
//!   the tasks.
//!
//! A judge task is a task of kind `work` whose `parent_task_id` names the
//! task it judges. Each test makes its own temporary ledger. No test starts
//! a process or reads the clock of this machine: a recorded process is a
//! number, and the test gives what the pass sees of it.

use horch_core::fleetd::task_execution::application::cancel::{self, Cancel};
use horch_core::fleetd::task_execution::domain::task::{Facts, Recorded, Seen};
use horch_core::taskledger::model::{Actor, TaskKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

/// The recorded agent of the worker of the judge task. No process has it.
const AGENT: Recorded = Recorded {
    pid: 4_000_001,
    pid_start: Some(7),
};

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    run: Run,
}

impl World {
    fn new() -> World {
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
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        World {
            _dir: dir,
            store,
            run,
        }
    }

    fn add(&mut self, t: NewTask) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id))
            .unwrap()
    }

    /// A judged task `W` in `verifying` and its judge task `J` in
    /// `judge_status`. Gives (W, J).
    fn pair(&mut self, judge_status: &str) -> (String, String) {
        let w = self.add(NewTask::work("doc", "tickets/doc.md", "dev"));
        let mut j = NewTask::work("doc-judge", "tickets/doc-judge.md", "judge");
        j.parent_task_id = Some(w.clone());
        let j = self.add(j);
        self.set_status(&w, "verifying");
        self.set_status(&j, judge_status);
        (w, j)
    }

    fn set_status(&self, task_id: &str, status: &str) {
        self.store
            .conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                [task_id, status],
            )
            .unwrap();
    }

    fn task(&self, task_id: &str) -> Task {
        tasks::get(self.store.conn(), task_id).unwrap()
    }

    fn status(&self, task_id: &str) -> String {
        self.task(task_id).status
    }

    fn cancel_requested(&self, task_id: &str) -> Option<String> {
        tasks::cancel_requested(self.store.conn(), task_id).unwrap()
    }

    /// `horch task cancel <task_id>`: the use case in 1 transaction.
    fn request(&mut self, task_id: &str, reason: &str) -> anyhow::Result<Cancel> {
        let t = self.task(task_id);
        self.store.immediate(|tx| cancel::request(tx, &t, reason))
    }

    /// A worker of `judge` claims the ready judge task: 1 live attempt.
    fn judge_claimed(&mut self) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "judge".into(),
            pane_id: Some("w0:p2".into()),
            ..Default::default()
        };
        let w = self
            .store
            .immediate(|tx| {
                let id = workers::insert(tx, &new)?;
                workers::agent_started(tx, &id, AGENT.pid, AGENT.pid_start)?;
                Ok(id)
            })
            .unwrap();
        let out = self.store.immediate(|tx| tasks::claim(tx, &w)).unwrap();
        assert!(matches!(out, ClaimOutcome::Claimed(_)), "{out:?}");
        w
    }
}

/// What the pass sees: the agent of the judge is `seen`.
fn facts(seen: Seen) -> Facts {
    let mut f = Facts::default();
    f.processes.insert(AGENT, seen);
    f
}

#[test]
fn cancel_of_a_judged_task_cancels_its_judge_task() {
    // The judge task waits for its judged task: both are cancelled at once.
    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    let out = w.request(&judged, "the idea changed").unwrap();
    let Cancel::Cancelled { task, judges, .. } = out else {
        panic!("{out:?}");
    };
    assert_eq!(task.status, "cancelled");
    assert_eq!(w.status(&judged), "cancelled");
    assert_eq!(w.status(&judge), "cancelled");
    assert_eq!(judges.len(), 1, "{judges:?}");
    assert!(
        matches!(&judges[0], Cancel::Cancelled { task, .. } if task.task_id == judge),
        "{judges:?}"
    );

    // The judge task runs: its cancel stops the process first, in the same
    // transaction as the cancel of the judged task.
    let mut w = World::new();
    let (judged, judge) = w.pair("ready");
    let worker = w.judge_claimed();
    assert_eq!(w.status(&judge), "running");
    let out = w.request(&judged, "the idea changed").unwrap();
    let Cancel::Cancelled { judges, .. } = out else {
        panic!("{out:?}");
    };
    assert_eq!(w.status(&judged), "cancelled");
    let [Cancel::Stopping { task, held, .. }] = judges.as_slice() else {
        panic!("{judges:?}");
    };
    assert_eq!(task.task_id, judge);
    assert_eq!(*held, vec![worker]);
    assert!(w.cancel_requested(&judge).is_some());

    // The pass completes the cancel of the judge when its process is gone.
    let still = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &facts(Seen::Alive)))
        .unwrap();
    assert!(still.is_empty());
    let done = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &facts(Seen::Dead)))
        .unwrap();
    assert_eq!(done, vec![judge.clone()]);
    assert_eq!(w.status(&judge), "cancelled");

    // A judge task that is already terminal stays as it is.
    let mut w = World::new();
    let (judged, judge) = w.pair("done");
    let out = w.request(&judged, "stop it").unwrap();
    assert!(
        matches!(&out, Cancel::Cancelled { judges, .. } if judges.is_empty()),
        "{out:?}"
    );
    assert_eq!(w.status(&judge), "done");
}

#[test]
fn cancel_of_a_judge_task_alone_is_refused() {
    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    let err = w.request(&judge, "not needed").unwrap_err();
    let want = format!(
        "cancel the judged task {}; its judge follows",
        w.task(&judged).short()
    );
    assert!(format!("{err:#}").contains(&want), "{err:#}");
    assert_eq!(w.status(&judge), "pending");
    assert_eq!(w.status(&judged), "verifying");

    // A running judge task alone is refused too, and its attempt goes on.
    let mut w = World::new();
    let (judged, judge) = w.pair("ready");
    w.judge_claimed();
    assert!(w.request(&judge, "not needed").is_err());
    assert_eq!(w.status(&judge), "running");
    assert!(w.cancel_requested(&judge).is_none());
    assert_eq!(w.status(&judged), "verifying");

    // Once the judged task is cancelled, the cancel of its judge task is
    // allowed.
    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    w.set_status(&judged, "cancelled");
    let out = w.request(&judge, "left over").unwrap();
    assert!(matches!(out, Cancel::Cancelled { .. }), "{out:?}");
    assert_eq!(w.status(&judge), "cancelled");

    // A review task of today also has a parent task, but it is not a judge
    // task: its cancel is not refused.
    let mut w = World::new();
    let parent = w.add(NewTask::work("code", "tickets/code.md", "dev"));
    w.set_status(&parent, "verifying");
    let mut review = NewTask::work("review", "tickets/review.md", "reviewer");
    review.kind = TaskKind::Review;
    review.parent_task_id = Some(parent.clone());
    let review = w.add(review);
    w.request(&review, "not needed").unwrap();
    assert_eq!(w.status(&review), "cancelled");
    assert_eq!(w.status(&parent), "verifying");
}

#[test]
fn cancel_of_an_idea_cancels_both() {
    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    let other = w.add(NewTask::work("other", "tickets/other.md", "dev"));
    let done = w.add(NewTask::work("old", "tickets/old.md", "dev"));
    w.set_status(&done, "done");
    // The cancel of an idea cancels each of its tasks. The judge task
    // comes first here: it is not refused, as its judged task is cancelled
    // in the same call.
    let ids = [&judge, &other, &judged, &done];
    let each: Vec<Task> = ids.iter().map(|id| w.task(id)).collect();
    let out = w
        .store
        .immediate(|tx| cancel::request_each(tx, &each, "the idea is cancelled"))
        .unwrap();
    for id in [&judged, &judge, &other] {
        assert_eq!(w.status(id), "cancelled", "{id}");
    }
    assert_eq!(w.status(&done), "done", "a terminal task stays as it is");
    let named: Vec<&str> = out.iter().map(|c| c.task().task_id.as_str()).collect();
    assert_eq!(named, vec![other.as_str(), judged.as_str()]);

    // A judge task whose judged task is not in the set is still refused,
    // and the whole set stays as it is.
    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    let other = w.add(NewTask::work("other", "tickets/other.md", "dev"));
    let each = vec![w.task(&other), w.task(&judge)];
    let err = w
        .store
        .immediate(|tx| cancel::request_each(tx, &each, "the idea is cancelled"))
        .unwrap_err();
    assert!(format!("{err:#}").contains("its judge follows"), "{err:#}");
    assert_eq!(w.status(&other), "ready");
    assert_eq!(w.status(&judge), "pending");
    assert_eq!(w.status(&judged), "verifying");
}
