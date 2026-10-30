//! `horch ledger status`: the board `report` builds and `render` prints (#82).

use std::path::Path;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::status;
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

fn ledger() -> (tempfile::TempDir, Store, Run) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                Path::new("/repo"),
                &NewRun {
                    slug: "r1".into(),
                    run_branch: Some("fleet/r1".into()),
                    base_branch: Some("main".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    (dir, store, run)
}

fn add(store: &mut Store, run: &Run, title: &str) -> tasks::Task {
    let t = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
    store
        .immediate(|tx| tasks::add(tx, run, &t, Actor::Orchestrator))
        .unwrap()
}

/// 1 task of each status, 1 working worker whose last sign of life is old,
/// and 1 open worktree.
fn busy_board() -> (tempfile::TempDir, Store, Run) {
    let (dir, mut store, run) = ledger();
    let first = add(&mut store, &run, "alpha");
    let worker = store
        .immediate(|tx| {
            let id = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "dev".into(),
                    role: Some("dev-x".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &id, 4242, Some(1))?;
            workers::agent_started(tx, &id, 4243, Some(2))?;
            Ok(id)
        })
        .unwrap();
    let out = store.immediate(|tx| tasks::claim(tx, &worker)).unwrap();
    assert!(matches!(out, ClaimOutcome::Claimed(_)));
    for title in ["beta", "gamma", "delta", "eps"] {
        add(&mut store, &run, title);
    }
    for (title, status) in [
        ("gamma", "landing"),
        ("delta", "done"),
        ("eps", "cancelled"),
    ] {
        store
            .conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE title = ?1",
                [title, status],
            )
            .unwrap();
    }
    store
        .conn()
        .execute(
            "UPDATE tasks SET held_reason = 'two failures', attempts = 3 WHERE title = 'beta'",
            [],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE workers SET claimed_at = '2000-01-01T00:00:00Z', last_checkin_at = NULL, \
             last_progress_at = NULL WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, origin, created_at) \
             VALUES ('wt1', ?1, '/repo', '/repo/.worktrees/a', 'fleet/a', 'main', 'open', 'native', '2000-01-01T00:00:00Z')",
            [&first.task_id],
        )
        .unwrap();
    (dir, store, run)
}

#[test]
fn report_counts_every_status_and_flags_the_unwatched_worker() {
    let (_d, store, run) = busy_board();
    let r = status::report(store.conn(), &run).unwrap();
    let c = &r.counts;
    assert_eq!(
        (c.queued, c.running, c.landing, c.done, c.cancelled),
        (1, 1, 1, 1, 1)
    );
    assert!(!r.complete);
    assert_eq!(r.workers.len(), 1);
    assert!(r.workers[0].unwatched);
    assert_eq!(r.workers[0].task.as_deref(), Some("T-1"));
    assert_eq!(r.worktrees.len(), 1);
    assert_eq!(r.worktrees[0].task.as_deref(), Some("T-1"));
}

#[test]
fn render_prints_each_section() {
    let (_d, store, run) = busy_board();
    let text = status::render(&status::report(store.conn(), &run).unwrap());
    assert_eq!(
        text,
        "Run r1 (active). Branch: fleet/r1 -> main. PR: none.\n\
         T-1   running   dev                dev-x                alpha\n\
         T-2   queued    dev                -                    beta  [held: two failures. Ask the operator; release it with horch ledger requeue T-2 \"<reason>\"]  [3 attempts]\n\
         T-3   landing   dev                -                    gamma\n\
         T-4   done      dev                -                    delta\n\
         T-5   cancelled dev                -                    eps\n\
         Live workers:\n\
         \x20 dev-x                working   T-1   checkin - ago, progress - ago  UNWATCHED\n\
         Worktrees:\n\
         \x20 T-1   open           /repo/.worktrees/a (base main)\n\
         5 tasks: 1 done, 1 running, 1 landing, 1 queued, 1 cancelled. Not complete.\n"
    );
}

#[test]
fn an_empty_run_renders_no_tasks_and_is_complete() {
    let (_d, store, run) = ledger();
    let text = status::render(&status::report(store.conn(), &run).unwrap());
    assert_eq!(
        text,
        "Run r1 (active). Branch: fleet/r1 -> main. PR: none.\n\
         No tasks.\n\
         0 tasks: 0 done, 0 running, 0 landing, 0 queued, 0 cancelled. Complete.\n"
    );
}

#[test]
fn events_and_stops_read_their_rows_in_order() {
    let (_d, store, run) = busy_board();
    let events = status::events(store.conn(), "run_id", &run.run_id).unwrap();
    assert!(events.len() >= 2);
    assert_eq!(events[0].kind, "run_created");
    assert!(events.windows(2).all(|w| w[0].at <= w[1].at));
    store
        .conn()
        .execute(
            "INSERT INTO stops (worker_id, requested_by, reason, target, pid, pid_started, signal, pane_id, result, at) \
             SELECT worker_id, 'operator', 'why', 'agent', 7, 8, 'TERM', 'p1', 'sent', '2000-01-01T00:00:00Z' FROM workers",
            [],
        )
        .unwrap();
    let w: String = store
        .conn()
        .query_row("SELECT worker_id FROM workers", [], |r| r.get(0))
        .unwrap();
    let stops = status::stops(store.conn(), &w).unwrap();
    assert_eq!(stops.len(), 1);
    let s = &stops[0];
    assert_eq!(
        (
            s.requested_by.as_str(),
            s.reason.as_str(),
            s.target.as_str(),
            s.pid,
            s.pid_started
        ),
        ("operator", "why", "agent", Some(7), Some(8))
    );
    assert_eq!(
        (
            s.signal.as_deref(),
            s.pane_id.as_deref(),
            s.result.as_str(),
            s.at.as_str()
        ),
        (Some("TERM"), Some("p1"), "sent", "2000-01-01T00:00:00Z")
    );
}
