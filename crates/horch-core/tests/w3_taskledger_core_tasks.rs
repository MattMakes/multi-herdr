//! `tasks::claim`, `tasks::waits` and `tasks::add_many` (#90). The queue's
//! rule clauses have their own tests in `taskledger_queue.rs`; these pin the
//! branches of `claim` and the 1 test per `Wait` kind.

use std::path::Path;

use horch_core::taskledger::model::{Actor, TaskKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask, Task, Wait};
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
                    ..Default::default()
                },
            )
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

fn worker_after(store: &mut Store, run: &Run, previous: Option<&str>) -> String {
    store
        .immediate(|tx| {
            let id = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "dev".into(),
                    role: Some("dev-x".into()),
                    previous_worker_id: previous.map(str::to_string),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &id, 4242, Some(1))?;
            workers::agent_started(tx, &id, 4243, Some(2))?;
            Ok(id)
        })
        .unwrap()
}

fn worker(store: &mut Store, run: &Run) -> String {
    worker_after(store, run, None)
}

fn claim(store: &mut Store, w: &str) -> anyhow::Result<ClaimOutcome> {
    store.immediate(|tx| tasks::claim(tx, w))
}

fn claimed_title(out: ClaimOutcome) -> Option<String> {
    match out {
        ClaimOutcome::Claimed(c) => Some(c.task.title),
        ClaimOutcome::NoTask { .. } => None,
    }
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
fn a_second_claim_gives_back_the_held_task() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    add(&mut s, &run, "b", |_| {});
    let w = worker(&mut s, &run);
    let first = claim(&mut s, &w).unwrap();
    let again = claim(&mut s, &w).unwrap();
    assert_eq!(first, again);
    let (title, attempts): (String, i64) = s
        .conn()
        .query_row(
            "SELECT title, attempts FROM tasks WHERE status = 'running'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((title.as_str(), attempts), ("a", 1));
}

#[test]
fn a_replacement_takes_its_predecessors_task_first() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "front", |t| t.priority = 50);
    add(&mut s, &run, "mine", |_| {});
    let old = worker(&mut s, &run);
    // The predecessor holds "mine" when it dies; the task goes back to the queue.
    sql(
        &s,
        &format!(
            "UPDATE workers SET task_id = (SELECT task_id FROM tasks WHERE title = 'mine'), \
             status = 'stopped' WHERE worker_id = '{old}'"
        ),
    );
    let fresh = worker_after(&mut s, &run, Some(&old));
    assert_eq!(
        claimed_title(claim(&mut s, &fresh).unwrap()).as_deref(),
        Some("mine")
    );
    let other = worker(&mut s, &run);
    assert_eq!(
        claimed_title(claim(&mut s, &other).unwrap()).as_deref(),
        Some("front")
    );
}

#[test]
fn a_worker_without_a_predecessor_takes_the_first_in_queue_order() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "front", |t| t.priority = 50);
    add(&mut s, &run, "mine", |_| {});
    let w = worker(&mut s, &run);
    assert_eq!(
        claimed_title(claim(&mut s, &w).unwrap()).as_deref(),
        Some("front")
    );
}

#[test]
fn no_task_ends_the_worker_and_records_why() {
    let (_d, mut s, run) = ledger();
    let w = worker(&mut s, &run);
    assert!(matches!(
        claim(&mut s, &w).unwrap(),
        ClaimOutcome::NoTask { .. }
    ));
    let (status, reason): (String, String) = s
        .conn()
        .query_row(
            "SELECT status, end_reason FROM workers WHERE worker_id = ?1",
            [&w],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (status.as_str(), reason.as_str()),
        ("no_task", "no task for persona dev")
    );
    let note: String = s
        .conn()
        .query_row(
            "SELECT text FROM events WHERE kind = 'no_task' AND worker_id = ?1",
            [&w],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, "No task for persona dev.");
}

#[test]
fn an_ended_worker_cannot_claim() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    let w = worker(&mut s, &run);
    sql(
        &s,
        &format!("UPDATE workers SET status = 'stopped' WHERE worker_id = '{w}'"),
    );
    let err = claim(&mut s, &w).unwrap_err().to_string();
    assert_eq!(
        err,
        "worker dev-x is stopped; a worker that ended cannot claim"
    );
}

#[test]
fn the_orchestrator_cannot_claim() {
    let (_d, mut s, run) = ledger();
    let w = worker(&mut s, &run);
    sql(
        &s,
        &format!("UPDATE workers SET kind = 'orchestrator' WHERE worker_id = '{w}'"),
    );
    let err = claim(&mut s, &w).unwrap_err().to_string();
    assert_eq!(err, "the orchestrator does not claim tasks");
}

#[test]
fn a_lost_race_is_an_error_and_changes_no_worker() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", |_| {});
    let w = worker(&mut s, &run);
    // Another writer took the task between the read and the write: the
    // guarded UPDATE changes 0 rows. A trigger stands in for that writer.
    sql(
        &s,
        "CREATE TRIGGER lose_the_race BEFORE UPDATE ON tasks BEGIN SELECT RAISE(IGNORE); END",
    );
    let err = claim(&mut s, &w).unwrap_err().to_string();
    assert_eq!(err, "T-1 was claimed by another worker");
    let status: String = s
        .conn()
        .query_row(
            "SELECT status FROM workers WHERE worker_id = ?1",
            [&w],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(status, "working");
}

#[test]
fn a_claim_hands_the_open_worktree_to_the_new_worker() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "a", |_| {});
    sql(
        &s,
        &format!(
            "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, origin, created_at) \
             VALUES ('wt1', '{}', '/repo', '/repo/.worktrees/a', 'fleet/a', 'main', 'open', 'native', '2000-01-01T00:00:00Z')",
            t.task_id
        ),
    );
    let w = worker(&mut s, &run);
    let ClaimOutcome::Claimed(c) = claim(&mut s, &w).unwrap() else {
        panic!("expected a claim");
    };
    assert_eq!(c.worktree.as_deref(), Some("/repo/.worktrees/a"));
    assert_eq!(c.worktree_branch.as_deref(), Some("fleet/a"));
    let owner: String = s
        .conn()
        .query_row("SELECT worker_id FROM worktrees", [], |r| r.get(0))
        .unwrap();
    assert_eq!(owner, w);
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
    // The blocker is inserted first, so it gets seq 1.
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

/// Each check of `add` refuses alone, with its own message, and writes
/// nothing; a task that passes them all is still taken.
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

    // T-2 waits for T-1, not the other way round.
    let err = tasks::unblock(s.conn(), &a, &b).unwrap_err().to_string();
    assert_eq!(err, "T-1 does not wait for T-2");
    tasks::unblock(s.conn(), &b, &a).unwrap();
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
    let t = add(&mut s, &run, "t", |t| t.fallback = Some("fb".into()));
    let w = worker(&mut s, &run);
    assert!(matches!(claim(&mut s, &w), Ok(ClaimOutcome::Claimed(_))));
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

    // A queued task that nobody holds is not requeued: there is nothing to undo.
    let err = tasks::requeue_by_operator(s.conn(), &t, "again")
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "T-1 is queued; only a held or cancelled task is requeued"
    );

    let cancelled = s
        .immediate(|tx| tasks::cancel(tx, &t, "no longer needed"))
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    let err = tasks::cancel(s.conn(), &cancelled, "twice")
        .unwrap_err()
        .to_string();
    assert_eq!(err, "T-1 is already cancelled");

    let back = s
        .immediate(|tx| tasks::requeue_by_operator(tx, &cancelled, "again"))
        .unwrap();
    assert_eq!(back.status, "queued");
    assert_eq!(back.finished_at, None);
}
