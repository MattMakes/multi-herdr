//! The task ledger's queue: the startable rule, fallback, cycles (#32,
//! #33). No worker finds a task by a search: each test takes the first
//! startable task of a persona and binds it as fleetd does (FR-5). The CLI
//! seam has its own tests in horch-e2e; these pin the rule clause by
//! clause.

use std::path::Path;

use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Wait};
use horch_core::taskledger::testing;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

fn ledger() -> (tempfile::TempDir, Store, Run) {
    ledger_with(runs::Thresholds::default())
}

/// A ledger with 1 run on `main` of a git repository: the bind of a
/// worktree task reads the commit of its base.
fn ledger_with(thresholds: runs::Thresholds) -> (tempfile::TempDir, Store, Run) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                &repo,
                &NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    thresholds,
                    ..Default::default()
                },
            )?;
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
    (dir, store, run)
}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
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

/// The first startable task of `persona` in board order, bound to a new
/// live worker of `persona` as fleetd binds it: its seq and the worker.
/// `None` when no task of `persona` can start.
fn take(store: &mut Store, run: &Run, persona: &str) -> Option<(i64, String)> {
    let task = tasks::startable(store.conn(), &run.run_id)
        .unwrap()
        .into_iter()
        .find(|t| t.claimable_by.as_deref() == Some(persona))?;
    let new = NewWorker {
        run_id: run.run_id.clone(),
        persona: persona.into(),
        role: Some(format!("{persona}-x")),
        ..Default::default()
    };
    let agent = Recorded {
        pid: 4243,
        pid_start: Some(2),
    };
    let bound = testing::bind_live(store.conn(), &task.task_id, &new, Some(agent)).unwrap();
    testing::record_supervisor(store.conn(), &bound.worker_id, 4242, Some(1)).unwrap();
    Some((task.seq, bound.worker_id))
}

fn taken_seq(store: &mut Store, run: &Run, persona: &str) -> Option<i64> {
    take(store, run, persona).map(|(seq, _)| seq)
}

fn finish(store: &mut Store, task_id: &str) {
    store
        .immediate(|tx| {
            tx.execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [task_id],
            )?;
            // As `horch ledger done` does: each dependent that waits for
            // nothing else is ready.
            tasks::ready_dependents(tx, task_id)
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
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(1));
    assert_eq!(taken_seq(&mut s, &run, "opus"), None, "B is blocked");
    let waits = tasks::waits(s.conn(), &run.run_id).unwrap();
    assert!(matches!(&waits[0].1, Wait::BlockedBy { task, .. } if task == "T-1"));
    finish(&mut s, &a.task_id);
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(b.seq));
}

#[test]
fn an_exclusive_task_runs_alone() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "a", "opus", |_| {});
    add(&mut s, &run, "x", "opus", |t| t.parallel = false);
    add(&mut s, &run, "c", "opus", |_| {});
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(1));
    // X waits for A; C may start beside A.
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(3));
    assert_eq!(taken_seq(&mut s, &run, "opus"), None, "X waits for A and C");
}

#[test]
fn nothing_starts_beside_a_running_exclusive_task() {
    let (_d, mut s, run) = ledger();
    add(&mut s, &run, "x", "opus", |t| t.parallel = false);
    add(&mut s, &run, "b", "opus", |_| {});
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(1));
    assert_eq!(taken_seq(&mut s, &run, "opus"), None);
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
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(1));
    assert_eq!(
        taken_seq(&mut s, &run, "opus"),
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
    // A slot is held only for a persona with a live worker (TRF-31).
    worker(&mut s, &run, "opus");
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
fn only_the_named_persona_takes_the_task_until_fallback() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "a", "opus", |t| {
        t.fallback = Some("sonnet".into())
    });
    assert_eq!(taken_seq(&mut s, &run, "sonnet"), None);
    s.immediate(|tx| tasks::fallback(tx, &t, "opus pool refused"))
        .unwrap();
    assert_eq!(taken_seq(&mut s, &run, "sonnet"), Some(1));
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
    let err = s
        .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
        .unwrap_err();
    assert_eq!(err.to_string(), "no task 'T-9' in this run");
    let queued: i64 = s
        .conn()
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(queued, 0, "the refused task was not queued");
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
fn a_cancelled_task_is_finished_and_not_claimable() {
    let (_d, mut s, run) = ledger();
    let t = add(&mut s, &run, "a", "opus", |_| {});
    s.immediate(|tx| tasks::cancel(tx, &t, "dropped")).unwrap();
    assert_eq!(taken_seq(&mut s, &run, "opus"), None);
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
    // A slot is held only for a persona with a live worker (TRF-31).
    worker(&mut s, &run, "opus");
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
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(1));
    assert_eq!(taken_seq(&mut s, &run, "opus"), Some(3));
    assert_eq!(taken_seq(&mut s, &run, "opus"), None, "T-2 waits for T-1");
}

#[test]
fn an_agent_that_exits_on_its_last_attempt_fails_the_task() {
    use horch_core::taskledger::supervisor::{self, Ended};
    let (dir, mut store, run) = ledger_with(runs::Thresholds {
        max_attempts: Some(2),
        ..Default::default()
    });
    let db = dir.path().join("ledger.sqlite3");
    let t = add(&mut store, &run, "a", "opus", |_| {});
    let (seq, first) = take(&mut store, &run, "opus").unwrap();
    assert_eq!(seq, 1);
    assert_eq!(
        horch_core::taskledger::Store::open(&db)
            .and_then(|mut s| s.immediate(|tx| {
                let ended = supervisor::settle(tx, &first, Some(1))?;
                // The pass of fleetd ends the attempt of the exited agent.
                commands::end_own_attempt(tx, &first, "crashed")?;
                Ok(ended)
            }))
            .unwrap(),
        Ended::Requeued {
            task: "T-1".into(),
            persona: "opus".into()
        }
    );
    let (seq, second) = take(&mut store, &run, "opus").unwrap();
    assert_eq!(seq, 1);
    assert_eq!(
        horch_core::taskledger::Store::open(&db)
            .and_then(|mut s| s.immediate(|tx| {
                let ended = supervisor::settle(tx, &second, Some(1))?;
                // The pass of fleetd ends the attempt of the exited agent.
                commands::end_own_attempt(tx, &second, "crashed")?;
                Ok(ended)
            }))
            .unwrap(),
        Ended::Failed { task: "T-1".into() }
    );
    let held = tasks::get(store.conn(), &t.task_id).unwrap();
    // Rung 6: no attempt left, so the task is failed and waits for its 1
    // replan by the orchestrator. It is escalated after that.
    assert_eq!(held.status, "failed");
    assert_eq!(held.held_reason, None);
    assert!(tasks::startable(store.conn(), &run.run_id)
        .unwrap()
        .is_empty());
    assert_eq!(taken_seq(&mut store, &run, "opus"), None);
    // The replan, then 2 more exits: escalated, held for the operator.
    store
        .immediate(|tx| tasks::requeue_by_operator(tx, &held, "replan"))
        .unwrap();
    for (n, ended) in [
        (
            "fourth",
            Ended::Requeued {
                task: "T-1".into(),
                persona: "opus".into(),
            },
        ),
        ("fifth", Ended::Escalated { task: "T-1".into() }),
    ] {
        let (seq, w) = take(&mut store, &run, "opus").unwrap();
        assert_eq!(seq, 1, "{n}");
        assert_eq!(
            horch_core::taskledger::Store::open(&db)
                .and_then(|mut s| s.immediate(|tx| {
                    let ended = supervisor::settle(tx, &w, Some(1))?;
                    // The pass of fleetd ends the attempt of the exited agent.
                    commands::end_own_attempt(tx, &w, "crashed")?;
                    Ok(ended)
                }))
                .unwrap(),
            ended
        );
    }
    let escalated = tasks::get(store.conn(), &t.task_id).unwrap();
    assert_eq!(escalated.status, "escalated");
    assert_eq!(escalated.held_reason.as_deref(), Some("2 attempts failed"));
}

#[test]
fn a_schema_2_ledger_migrates_forward_and_keeps_its_rows() {
    // A schema 2 file, built by the real steps, with 1 run, 1 claimed task
    // and its worker.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    horch_core::taskledger::schema::migrate_to(&mut conn, 2).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/repo', 'now');
         INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r', 'p', 'r1', 'active', 'now');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                status, attempts, current_worker_id, created_at, updated_at)
            VALUES ('T-1', 'r', 1, 'a', 'tickets/a.md', 'opus', 'opus', 'running', 1, 'w', 'now', 'now');
         INSERT INTO workers (worker_id, run_id, persona, host, task_id, attempt, status)
            VALUES ('w', 'r', 'opus', 'h', 'T-1', 1, 'working');",
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();
    let v: i64 = store
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, horch_core::taskledger::schema::VERSION);
    let migrated = runs::get(store.conn(), "r").unwrap();
    assert_eq!(migrated.slug, "r1");
    assert_eq!(migrated.max_attempts, 3);
    let task = tasks::get(store.conn(), "T-1").unwrap();
    assert_eq!(task.status, "running");
    assert_eq!(task.attempts, 1);
    assert_eq!(task.current_worker_id.as_deref(), Some("w"));
    assert_eq!(task.held_reason, None);
    assert_eq!(task.attempt_floor, 0);
    let idea: Option<String> = store
        .conn()
        .query_row("SELECT idea_id FROM tasks WHERE task_id = 'T-1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        idea.as_deref(),
        Some("I-0001"),
        "the release gives the run 1 idea"
    );
}
