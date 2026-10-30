//! The task ledger's queue: the claimable rule, claims, fallback, cycles
//! (#32, #33). The CLI seam has its own tests in horch-e2e; these pin the
//! rule clause by clause.

use std::path::Path;

use horch_core::taskledger::model::{Actor, WorkerStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask, Wait};
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

fn add(
    store: &mut Store,
    run: &Run,
    title: &str,
    persona: &str,
    f: impl FnOnce(&mut NewTask),
) -> tasks::Task {
    let mut t = NewTask::work(title, &format!("tickets/{title}.md"), persona);
    f(&mut t);
    store
        .immediate(|tx| tasks::add(tx, run, &t, Actor::Orchestrator))
        .unwrap()
}

fn worker(store: &mut Store, run: &Run, persona: &str) -> String {
    store
        .immediate(|tx| {
            let id = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: persona.into(),
                    role: Some(format!("{persona}-x")),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &id, 4242, Some(1))?;
            workers::agent_started(tx, &id, 4243, Some(2))?;
            Ok(id)
        })
        .unwrap()
}

fn claim(store: &mut Store, w: &str) -> ClaimOutcome {
    store.immediate(|tx| tasks::claim(tx, w)).unwrap()
}

fn claimed_seq(o: &ClaimOutcome) -> Option<i64> {
    match o {
        ClaimOutcome::Claimed(c) => Some(c.task.seq),
        ClaimOutcome::NoTask { .. } => None,
    }
}

fn finish(store: &mut Store, task_id: &str) {
    store
        .immediate(|tx| {
            tx.execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [task_id],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_blocked_task_waits_for_its_blocker() {
    let (_d, mut s, run) = ledger();
    let a = add(&mut s, &run, "a", "opus", |_| {});
    let b = add(&mut s, &run, "b", "opus", |t| {
        t.blocked_by = vec!["T-1".into()]
    });
    let w1 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w1)), Some(1));
    let w2 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w2)), None, "B is blocked");
    let waits = tasks::waits(s.conn(), &run.run_id).unwrap();
    assert!(matches!(&waits[0].1, Wait::BlockedBy { task, .. } if task == "T-1"));
    finish(&mut s, &a.task_id);
    let w3 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w3)), Some(b.seq));
}

#[test]
fn an_exclusive_task_runs_alone() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |_| {});
    add(&mut s, &run, "x", "opus", |t| t.parallel = false);
    add(&mut s, &run, "c", "opus", |_| {});
    let w1 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w1)), Some(1));
    // X waits for A; C may start beside A.
    let w2 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w2)), Some(3));
    let w3 = worker(&mut s, &run, "opus");
    assert_eq!(
        claimed_seq(&claim(&mut s, &w3)),
        None,
        "X waits for A and C"
    );
}

#[test]
fn nothing_starts_beside_a_running_exclusive_task() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "x", "opus", |t| t.parallel = false);
    add(&mut s, &run, "b", "opus", |_| {});
    let w1 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w1)), Some(1));
    let w2 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w2)), None);
}

#[test]
fn two_tasks_never_hold_one_file() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |t| {
        t.own = vec!["src/parse".into()]
    });
    add(&mut s, &run, "b", "opus", |t| {
        t.own = vec!["src/parse/json.rs".into()]
    });
    add(&mut s, &run, "c", "opus", |t| {
        t.own = vec!["src/parser.rs".into()]
    });
    let w1 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w1)), Some(1));
    let w2 = worker(&mut s, &run, "opus");
    assert_eq!(
        claimed_seq(&claim(&mut s, &w2)),
        Some(3),
        "src/parser.rs does not overlap"
    );
    let plan = tasks::plan_workers(s.conn(), &run.run_id).unwrap();
    assert!(plan.is_empty() || plan[0].startable == 0, "{plan:?}");
}

#[test]
fn a_queued_task_behind_a_task_with_its_file_waits_on_the_file() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |t| {
        t.own = vec!["crates/horch/src/cmd/taskledger.rs".into()]
    });
    add(&mut s, &run, "b", "opus", |t| {
        t.own = vec!["crates/horch/src/cmd/taskledger.rs".into()]
    });
    let waits = tasks::waits(s.conn(), &run.run_id).unwrap();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0].0.short(), "T-2");
    assert_eq!(
        waits[0].1,
        Wait::FileHeld {
            path: "crates/horch/src/cmd/taskledger.rs".into(),
            by: "T-1".into()
        }
    );
}

#[test]
fn only_the_named_persona_claims_until_fallback() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "a", "opus", |t| {
        t.fallback = Some("sonnet".into())
    });
    let ws = worker(&mut s, &run, "sonnet");
    assert!(matches!(claim(&mut s, &ws), ClaimOutcome::NoTask { .. }));
    let st = workers::get(s.conn(), &ws).unwrap();
    assert_eq!(st.status, WorkerStatus::NoTask.as_str());
    s.immediate(|tx| tasks::fallback(tx, &t, "opus pool refused"))
        .unwrap();
    let ws2 = worker(&mut s, &run, "sonnet");
    assert_eq!(claimed_seq(&claim(&mut s, &ws2)), Some(1));
}

#[test]
fn no_task_names_the_supervisor_pid() {
    let (_d, mut s, run) = ledger();
    let w = worker(&mut s, &run, "opus");
    match claim(&mut s, &w) {
        ClaimOutcome::NoTask { tell, .. } => {
            assert!(
                tell.contains("NOTE: No task for opus. I exit. PID 4242."),
                "{tell}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_map_with_a_cycle_queues_nothing() {
    let (_d, mut s, run) = ledger();
    let mut a = NewTask::work("a", "a.md", "opus");
    a.blocked_by = vec!["02".into()];
    let mut b = NewTask::work("b", "b.md", "opus");
    b.blocked_by = vec!["01".into()];
    let err = s
        .immediate(|tx| {
            tasks::add_many(
                tx,
                &run,
                &[("01".into(), a.clone()), ("02".into(), b.clone())],
                Actor::Orchestrator,
            )
        })
        .unwrap_err();
    assert!(format!("{err:#}").contains("cycle"), "{err:#}");
    assert!(tasks::of_run(s.conn(), &run.run_id).unwrap().is_empty());
}

#[test]
fn a_map_queues_with_its_blockers() {
    let (_d, mut s, run) = ledger();
    let a = NewTask::work("a", "a.md", "opus");
    let mut b = NewTask::work("b", "b.md", "opus");
    b.blocked_by = vec!["01".into()];
    let c = NewTask::work("c", "c.md", "sonnet");
    let out = s
        .immediate(|tx| {
            tasks::add_many(
                tx,
                &run,
                &[("02".into(), b), ("01".into(), a), ("03".into(), c)],
                Actor::Orchestrator,
            )
        })
        .unwrap();
    assert_eq!(out.len(), 3);
    let b = out.iter().find(|t| t.title == "b").unwrap();
    let blockers = tasks::blockers(s.conn(), &b.task_id).unwrap();
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].title, "a");
    let plan = tasks::plan_workers(s.conn(), &run.run_id).unwrap();
    let opus = plan.iter().find(|p| p.persona == "opus").unwrap();
    assert_eq!((opus.startable, opus.spawn), (1, 1));
}

#[test]
fn an_unknown_blocker_is_refused() {
    let (_d, mut s, run) = ledger();
    let mut t = NewTask::work("a", "a.md", "opus");
    t.blocked_by = vec!["T-9".into()];
    assert!(s
        .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
        .is_err());
}

#[test]
fn plan_workers_subtracts_workers_on_their_way() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |_| {});
    add(&mut s, &run, "b", "opus", |_| {});
    worker(&mut s, &run, "opus"); // claiming, not yet claimed
    let plan = tasks::plan_workers(s.conn(), &run.run_id).unwrap();
    assert_eq!(plan[0].startable, 2);
    assert_eq!(plan[0].on_the_way, 1);
    assert_eq!(plan[0].spawn, 1);
}

#[test]
fn eight_racing_claims_take_exactly_three_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    let mut s = Store::open(&path).unwrap();
    let run = s
        .immediate(|tx| {
            runs::create(
                tx,
                Path::new("/repo"),
                &NewRun {
                    slug: "race".into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    for n in 0..3 {
        add(&mut s, &run, &format!("t{n}"), "opus", |_| {});
    }
    let ids: Vec<String> = (0..8).map(|_| worker(&mut s, &run, "opus")).collect();
    drop(s);
    let handles: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut s = Store::open(&path).unwrap();
                s.immediate(|tx| tasks::claim(tx, &id)).unwrap()
            })
        })
        .collect();
    let outcomes: Vec<ClaimOutcome> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let mut won: Vec<i64> = outcomes.iter().filter_map(claimed_seq).collect();
    won.sort();
    assert_eq!(won, vec![1, 2, 3]);
    let s = Store::open(&path).unwrap();
    for t in tasks::of_run(s.conn(), &run.run_id).unwrap() {
        let working: i64 = s
            .conn()
            .query_row(
                "SELECT count(*) FROM workers WHERE task_id = ?1 AND status = 'working'",
                [&t.task_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(working, 1, "{}", t.short());
    }
}

#[test]
fn a_cancelled_task_is_finished_and_not_claimable() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "a", "opus", |_| {});
    s.immediate(|tx| tasks::cancel(tx, &t, "dropped")).unwrap();
    let w = worker(&mut s, &run, "opus");
    assert!(matches!(claim(&mut s, &w), ClaimOutcome::NoTask { .. }));
    assert!(tasks::all_finished(s.conn(), &run.run_id).unwrap());
}

#[test]
fn shared_checkout_tasks_run_one_at_a_time() {
    use horch_core::taskledger::model::WorktreeMode;
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |t| {
        t.worktree_mode = WorktreeMode::Shared
    });
    add(&mut s, &run, "b", "opus", |t| {
        t.worktree_mode = WorktreeMode::Shared
    });
    add(&mut s, &run, "c", "opus", |_| {});
    let plan = tasks::plan_workers(s.conn(), &run.run_id).unwrap();
    assert_eq!(
        plan[0].startable, 2,
        "T-1 and the worktree task T-3: {plan:?}"
    );
    let waits = tasks::waits(s.conn(), &run.run_id).unwrap();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0].0.short(), "T-2");
    assert!(
        waits[0]
            .1
            .to_string()
            .starts_with("exclusive: shared checkout"),
        "{}",
        waits[0].1
    );
    let w1 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w1)), Some(1));
    let w2 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w2)), Some(3));
    let w3 = worker(&mut s, &run, "opus");
    assert_eq!(claimed_seq(&claim(&mut s, &w3)), None, "T-2 waits for T-1");
}
