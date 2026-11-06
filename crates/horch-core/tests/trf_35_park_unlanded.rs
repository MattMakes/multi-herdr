//! TRF-35: `done --unlanded` parks the task with its worktree; dependents
//! stay blocked on it; `requeue` and triage `FINISH` resume it on the same
//! worktree (trace-fixes D16, ticket 21).

mod i308_support;

use std::path::PathBuf;

use horch_core::taskledger::finish;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask, Wait};
use horch_core::taskledger::triage::apply::{self, Env};
use horch_core::taskledger::triage::{self, reply, Measure};
use horch_core::taskledger::workers::{self, NewWorker};
use i308_support::{commit, World};

/// A run on `main` with T-1 and T-2 (blocked by T-1), and a worker that
/// claimed T-1 and committed 1 file in its worktree `feat`.
fn parked_world() -> (World, Run, String, PathBuf) {
    let mut w = World::new();
    let root = w.root.clone();
    let run = w
        .store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                &root,
                &NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some("main".into()),
                    ..Default::default()
                },
            )?;
            let t1 = tasks::add(tx, &run, &NewTask::work("a", "a.md", "opus"), Actor::Orchestrator)?;
            let mut b = NewTask::work("b", "b.md", "opus");
            b.blocked_by = vec![t1.short()];
            tasks::add(tx, &run, &b, Actor::Orchestrator)?;
            Ok(run)
        })
        .unwrap();
    let worker = new_worker(&mut w, &run);
    let ClaimOutcome::Claimed(c) = w.store.immediate(|tx| tasks::claim(tx, &worker)).unwrap()
    else {
        panic!("T-1 is not claimed");
    };
    assert_eq!(c.task.short(), "T-1");
    let wt = finish::worktree_new(&mut w.store, &w.git, &worker, "feat").unwrap();
    let path = PathBuf::from(&wt.path);
    commit(&path, "f.txt", "work\n");
    (w, run, worker, path)
}

fn new_worker(w: &mut World, run: &Run) -> String {
    w.store
        .immediate(|tx| {
            let id = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "opus".into(),
                    role: Some("opus-x".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &id, 4242, Some(1))?;
            workers::agent_started(tx, &id, 4243, Some(2))?;
            Ok(id)
        })
        .unwrap()
}

fn t1(w: &World, run: &Run) -> tasks::Task {
    tasks::resolve(w.store.conn(), &run.run_id, "T-1").unwrap()
}

/// The status, task and worker of the worktree at `path`.
fn worktree(w: &World, path: &PathBuf) -> (String, Option<String>) {
    w.store
        .conn()
        .query_row(
            "SELECT status, task_id FROM worktrees WHERE path = ?1",
            [path.to_string_lossy()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

fn wait_of(w: &World, run: &Run, task: &str) -> Option<Wait> {
    tasks::waits(w.store.conn(), &run.run_id)
        .unwrap()
        .into_iter()
        .find(|(t, _)| t.short() == task)
        .map(|(_, wait)| wait)
}

fn task_count(w: &World) -> i64 {
    w.store
        .conn()
        .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn trf_35_done_unlanded_parks_the_task_and_requeue_resumes_it_on_the_same_worktree() {
    let (mut w, run, worker, path) = parked_world();
    let c = finish::complete(&mut w.store, &worker, "tried", Some("a red check")).unwrap();
    assert!(c.unlanded);

    // Parked: queued and held, no worker, not finished; the worktree stays.
    let t = t1(&w, &run);
    assert!(t.is_parked(), "{t:?}");
    assert_eq!(t.status, "queued");
    assert_eq!(t.held_reason.as_deref(), Some("unlanded"));
    assert_eq!(t.current_worker_id, None);
    assert_eq!(t.finished_at, None);
    assert_eq!(t.summary.as_deref(), Some("UNLANDED: a red check | tried"));
    assert_eq!(worktree(&w, &path), ("unlanded".into(), Some(t.task_id.clone())));
    assert!(path.exists());

    // A retried done reports the same park.
    let again = finish::complete(&mut w.store, &worker, "tried", Some("a red check")).unwrap();
    assert!(again.already && again.unlanded);

    // Nothing starts: T-1 is held, and T-2 stays blocked on it.
    assert!(tasks::startable(w.store.conn(), &run.run_id).unwrap().is_empty());
    let wait = wait_of(&w, &run, "T-2").unwrap();
    assert_eq!(wait, Wait::BlockerParked { task: "T-1".into() });
    assert_eq!(
        wait.to_string(),
        "T-1 is parked unlanded. Run horch ledger requeue T-1 to resume it"
    );

    // requeue resumes T-1 on the same worktree; T-2 still waits for it.
    let back = w
        .store
        .immediate(|tx| tasks::requeue_by_operator(tx, &t, "fixed the check"))
        .unwrap();
    assert_eq!((back.status.as_str(), back.held_reason.as_deref()), ("queued", None));
    assert_eq!(worktree(&w, &path).0, "open");
    assert!(matches!(
        wait_of(&w, &run, "T-2"),
        Some(Wait::BlockedBy { task, .. }) if task == "T-1"
    ));

    let next = new_worker(&mut w, &run);
    let ClaimOutcome::Claimed(c) = w.store.immediate(|tx| tasks::claim(tx, &next)).unwrap()
    else {
        panic!("T-1 is not claimed again");
    };
    assert_eq!(c.task.short(), "T-1");
    assert_eq!(c.worktree.as_deref(), Some(path.to_string_lossy().as_ref()));
    assert_eq!(task_count(&w), 2);
}

#[test]
fn trf_35_triage_finish_resumes_the_parked_task_instead_of_queuing_a_new_one() {
    let (mut w, run, worker, path) = parked_world();
    finish::complete(&mut w.store, &worker, "tried", Some("a red check")).unwrap();
    let git = w.git.clone();
    let rows = triage::collect(w.store.conn(), &git, &w.root, Measure::Ids).unwrap();
    let here = path.to_string_lossy();
    let row = rows
        .iter()
        .find(|r| r.path == here)
        .unwrap_or_else(|| panic!("the parked worktree is not a triage row: {rows:?}"));
    let text = format!(
        "| ID | Worktree | Suggested | Decision | Direction |\n|---|---|---|---|---|\n| {} | {} | | FINISH | Fix the check, then land |\n",
        row.id, row.name
    );
    let decided = reply::parse(&text, &rows, false).unwrap();
    let evidence = w.top.join("evidence");
    let env = Env {
        git: &git,
        root: &w.root,
        evidence_root: &evidence,
        gh: None,
        remote: None,
        primary: "opus",
    };
    let a = apply::apply(&mut w.store, &env, &rows, &decided).unwrap().remove(0);
    assert!(a.ok, "{}", a.line());

    let t = t1(&w, &run);
    let q = a.queued.unwrap();
    assert_eq!((q.task.as_str(), q.task_id.as_str()), ("T-1", t.task_id.as_str()));
    assert_eq!(task_count(&w), 2, "FINISH queued a new task");
    assert_eq!((t.status.as_str(), t.held_reason.as_deref()), ("queued", None));
    assert_eq!(worktree(&w, &path), ("open".into(), Some(t.task_id.clone())));
    assert!(!w.root.join("ai_docs/triage").exists());
    assert!(matches!(
        wait_of(&w, &run, "T-2"),
        Some(Wait::BlockedBy { task, .. }) if task == "T-1"
    ));
}
