use horch_core::fleetd::task_execution::application::cancel::{self, Cancel};
use horch_core::fleetd::task_execution::domain::task::{Facts, Recorded, Seen};
use horch_core::taskledger::model::{Actor, TaskKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;

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

    fn request(&mut self, task_id: &str, reason: &str) -> anyhow::Result<Cancel> {
        let t = self.task(task_id);
        self.store.immediate(|tx| cancel::request(tx, &t, reason))
    }

    fn judge_claimed(&mut self) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "judge".into(),
            pane_id: Some("w0:p2".into()),
            ..Default::default()
        };
        let conn = self.store.conn();
        let task: String = conn
            .query_row(
                "SELECT task_id FROM tasks WHERE run_id = ?1 AND status = 'ready'
                 AND claimable_by = ?2 ORDER BY seq LIMIT 1",
                [&self.run.run_id, &new.persona],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        bind_live(conn, &task, &new, Some(AGENT)).unwrap().worker_id
    }
}

fn facts(seen: Seen) -> Facts {
    let mut f = Facts::default();
    f.processes.insert(AGENT, seen);
    f
}

#[test]
fn cancel_of_a_judged_task_cancels_its_judge_task() {
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

    let mut w = World::new();
    let (judged, judge) = w.pair("ready");
    w.judge_claimed();
    assert!(w.request(&judge, "not needed").is_err());
    assert_eq!(w.status(&judge), "running");
    assert!(w.cancel_requested(&judge).is_none());
    assert_eq!(w.status(&judged), "verifying");

    let mut w = World::new();
    let (judged, judge) = w.pair("pending");
    w.set_status(&judged, "cancelled");
    let out = w.request(&judge, "left over").unwrap();
    assert!(matches!(out, Cancel::Cancelled { .. }), "{out:?}");
    assert_eq!(w.status(&judge), "cancelled");

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
