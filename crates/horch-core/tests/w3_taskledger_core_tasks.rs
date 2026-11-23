use std::path::Path;

use horch_core::taskledger::model::{Actor, TaskKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task, Wait};
use horch_core::taskledger::testing;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;

fn ledger() -> (tempfile::TempDir, Store, Run) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                Path::new("/repo"),
                &NewRun {
                    slug: "r1".into(),
                    ..Default::default()
                },
            )?;
            horch_core::taskledger::schema::run_idea(
                tx,
                &run.run_id,
                &horch_core::taskledger::util::now(),
                true,
            )?;
            Ok(run)
        })
        .unwrap();
    (dir, store, run)
}

fn add(store: &mut Store, run: &Run, title: &str, f: impl FnOnce(&mut NewTask)) -> Task {
    let mut t = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
    f(&mut t);
    store
        .immediate(|tx| tasks::add(tx, run, &t, Actor::Orchestrator))
        .unwrap()
}

fn sql(store: &Store, statement: &str) {
    store.conn().execute(statement, []).unwrap();
}

fn waits_of(store: &Store, run: &Run) -> Vec<(String, Wait)> {
    tasks::waits(store.conn(), &run.run_id)
        .unwrap()
        .into_iter()
        .map(|(t, w)| (t.title, w))
        .collect()
}

#[test]
fn wait_blocked_by_an_unfinished_blocker() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    add(&mut s, &run, "b", |t| t.blocked_by = vec!["T-1".into()]);
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "b".to_string(),
            Wait::BlockedBy {
                task: "T-1".into(),
                status: "queued".into()
            }
        )]
    );
}

#[test]
fn wait_blocker_cancelled() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    add(&mut s, &run, "b", |t| t.blocked_by = vec!["T-1".into()]);
    sql(
        &s,
        "UPDATE tasks SET status = 'cancelled' WHERE title = 'a'",
    );
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "b".to_string(),
            Wait::BlockerCancelled { task: "T-1".into() }
        )]
    );
}

#[test]
fn wait_exclusive_for_a_running_exclusive_task() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "x", |t| t.parallel = false);
    add(&mut s, &run, "b", |_| {});
    sql(&s, "UPDATE tasks SET status = 'running' WHERE title = 'x'");
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "b".to_string(),
            Wait::Exclusive {
                running: "T-1".into()
            }
        )]
    );
}

#[test]
fn wait_exclusive_task_waits_for_a_running_one() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    add(&mut s, &run, "x", |t| t.parallel = false);
    sql(&s, "UPDATE tasks SET status = 'running' WHERE title = 'a'");
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "x".to_string(),
            Wait::Exclusive {
                running: "T-1".into()
            }
        )]
    );
}

#[test]
fn wait_shared_checkout() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |t| {
        t.worktree_mode = WorktreeMode::Shared
    });
    add(&mut s, &run, "b", |t| {
        t.worktree_mode = WorktreeMode::Shared
    });
    sql(&s, "UPDATE tasks SET status = 'running' WHERE title = 'a'");
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "b".to_string(),
            Wait::SharedCheckout {
                running: "T-1".into()
            }
        )]
    );
}

#[test]
fn wait_file_held() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |t| t.own = vec!["src".into()]);
    add(&mut s, &run, "b", |t| t.own = vec!["src/lib.rs".into()]);
    sql(&s, "UPDATE tasks SET status = 'running' WHERE title = 'a'");
    assert_eq!(
        waits_of(&s, &run),
        vec![(
            "b".to_string(),
            Wait::FileHeld {
                path: "src/lib.rs".into(),
                by: "T-1".into()
            }
        )]
    );
}

#[test]
fn a_review_task_never_waits_on_files_or_exclusive_tasks() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "x", |t| {
        t.parallel = false;
        t.own = vec!["src".into()];
    });
    add(&mut s, &run, "rev", |t| {
        t.kind = TaskKind::Review;
        t.own = vec!["src".into()];
    });
    sql(&s, "UPDATE tasks SET status = 'running' WHERE title = 'x'");
    assert_eq!(waits_of(&s, &run), vec![]);
}

#[test]
fn add_many_with_a_cycle_names_it_and_queues_nothing() {
    let (_d, mut s, run) = ledger();
    let mut a = NewTask::work("a", "a.md", "dev");
    a.blocked_by = vec!["02".into()];
    let mut b = NewTask::work("b", "b.md", "dev");
    b.blocked_by = vec!["01".into()];
    let items = vec![("01".to_string(), a), ("02".to_string(), b)];
    let err = s
        .immediate(|tx| tasks::add_many(tx, &run, &items, Actor::Orchestrator))
        .unwrap_err()
        .to_string();
    assert_eq!(err, "the map has a dependency cycle: 01 -> 02 -> 01");
    assert!(tasks::of_run(s.conn(), &run.run_id).unwrap().is_empty());
}

#[test]
fn add_many_refuses_a_repeated_number_an_unknown_blocker_and_an_invalid_task() {
    let (_d, mut s, run) = ledger();
    let item = |k: &str, f: &dyn Fn(&mut NewTask)| {
        let mut t = NewTask::work(k, &format!("{k}.md"), "dev");
        f(&mut t);
        (k.to_string(), t)
    };
    let run_many = |s: &mut Store, items: Vec<(String, NewTask)>| {
        s.immediate(|tx| tasks::add_many(tx, &run, &items, Actor::Orchestrator))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        run_many(&mut s, vec![item("01", &|_| {}), item("01", &|_| {})]),
        "2 tickets in the map share a number"
    );
    assert!(run_many(
        &mut s,
        vec![item("01", &|t| t.blocked_by = vec!["T-9".into()])]
    )
    .starts_with("ticket 01 is blocked by unknown 'T-9'"));
    assert_eq!(
        run_many(&mut s, vec![item("01", &|t| t.title = " ".into())]),
        "a task needs a title"
    );
}

#[test]
fn add_many_queues_in_dependency_order_and_returns_them_by_seq() {
    let (_d, mut s, run) = ledger();
    let mut late = NewTask::work("late", "l.md", "dev");
    late.blocked_by = vec!["02".into()];
    let early = NewTask::work("early", "e.md", "dev");
    let items = vec![("01".to_string(), late), ("02".to_string(), early)];
    let out = s
        .immediate(|tx| tasks::add_many(tx, &run, &items, Actor::Orchestrator))
        .unwrap();
    let titles: Vec<_> = out.iter().map(|t| (t.seq, t.title.as_str())).collect();
    assert_eq!(titles, vec![(1, "early"), (2, "late")]);
    let text: String = s
        .conn()
        .query_row(
            "SELECT text FROM events WHERE kind = 'added' AND task_id = ?1",
            [&out[1].task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(text, "T-2 queued from ticket 01 for dev: late.");
}

fn fresh(store: &Store, task: &Task) -> Task {
    tasks::get(store.conn(), &task.task_id).unwrap()
}

fn task_count(store: &Store) -> i64 {
    store
        .conn()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn add_refuses_a_missing_title_ticket_or_primary_and_a_fallback_equal_to_it() {
    let (_d, mut s, run) = ledger();
    let refused = |s: &mut Store, edit: &dyn Fn(&mut NewTask)| {
        let mut t = NewTask::work("t", "tickets/t.md", "dev");
        edit(&mut t);
        s.immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refused(&mut s, &|t| t.title = "  ".into()),
        "a task needs a title"
    );
    assert_eq!(
        refused(&mut s, &|t| t.task_file = " ".into()),
        "a task needs a ticket file"
    );
    assert_eq!(
        refused(&mut s, &|t| t.primary = "".into()),
        "a task needs a primary persona"
    );
    assert_eq!(
        refused(&mut s, &|t| t.fallback = Some("dev".into())),
        "the fallback persona is the primary persona"
    );
    assert_eq!(task_count(&s), 0, "a refused task writes nothing");

    let ok = add(&mut s, &run, "good", |t| t.fallback = Some("other".into()));
    assert_eq!(ok.fallback_persona.as_deref(), Some("other"));
}

#[test]
fn unblock_refuses_a_blocker_the_task_does_not_wait_for() {
    let (_d, mut s, run) = ledger();
    let a = add(&mut s, &run, "a", |_| {});
    let b = add(&mut s, &run, "b", |t| t.blocked_by = vec!["T-1".into()]);

    let err = tasks::unblock(s.conn(), &a, &b).unwrap_err().to_string();
    assert_eq!(err, "T-1 does not wait for T-2");
    assert_eq!(b.status, "pending");
    tasks::unblock(s.conn(), &b, &a).unwrap();
    let b = tasks::get(s.conn(), &b.task_id).unwrap();
    assert_eq!(b.status, "ready", "T-2 waits for nothing now");
    let err = tasks::unblock(s.conn(), &b, &a).unwrap_err().to_string();
    assert_eq!(err, "T-2 does not wait for T-1", "the wait is gone");
}

#[test]
fn fallback_moves_only_a_queued_task_that_has_a_fallback() {
    let (_d, mut s, run) = ledger();
    let plain = add(&mut s, &run, "plain", |_| {});
    let err = tasks::fallback(s.conn(), &plain, "slow")
        .unwrap_err()
        .to_string();
    assert_eq!(err, "T-1 has no fallback persona");

    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "t", |t| {
        t.fallback = Some("fb".into());
        t.worktree_mode = WorktreeMode::Shared;
    });
    let new = NewWorker {
        run_id: run.run_id.clone(),
        persona: "dev".into(),
        ..Default::default()
    };
    testing::bind_live(s.conn(), &t.task_id, &new, None).unwrap();
    let running = fresh(&s, &t);
    let err = tasks::fallback(s.conn(), &running, "slow")
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "T-1 is running; only a queued task moves to its fallback"
    );
    assert_eq!(fresh(&s, &t).claimable_by, t.claimable_by, "not moved");

    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "t", |t| t.fallback = Some("fb".into()));
    let moved = s.immediate(|tx| tasks::fallback(tx, &t, "slow")).unwrap();
    assert_eq!(moved.claimable_by.as_deref(), Some("fb"));
}

#[test]
fn cancel_and_requeue_take_only_the_states_they_are_for() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "t", |_| {});

    let err = tasks::requeue_by_operator(s.conn(), &t, "again")
        .unwrap_err()
        .to_string();
    assert_eq!(err, "T-1 is queued; only a held task is requeued");

    let cancelled = s
        .immediate(|tx| tasks::cancel(tx, &t, "no longer needed"))
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    let err = tasks::cancel(s.conn(), &cancelled, "twice")
        .unwrap_err()
        .to_string();
    assert_eq!(err, "T-1 is already cancelled");

    let err = s
        .immediate(|tx| tasks::requeue_by_operator(tx, &cancelled, "again"))
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("illegal transition of task ") && err.ends_with(": cancelled -> ready"),
        "{err}"
    );
    assert_eq!(
        tasks::get(s.conn(), &t.task_id).unwrap().status,
        "cancelled"
    );
}
