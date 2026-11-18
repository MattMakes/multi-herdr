//! Push dispatch: 1 function binds a ready task to a new attempt before its
//! pane exists, and fleetd chooses which ready tasks to bind
//! (docs/specs/fleetd/design.md, "The task state machine" rows `ready` and
//! `leased`, FR-5, FM-2, FM-14, "Holds on dispatch are visible", I4; rows
//! L1, L2, L3 and L6 of preservation-04-behavior.md;
//! docs/tickets/fleetd-p4/08-push-dispatch-bind.md).
//!
//! Each test makes its own temporary git repository, ledger and fleet
//! root. No test starts a `horch` binary, and no test reads the load, the
//! limits or the clock of this machine: the machine facts come from a
//! machine file, the floors from a test `fleet.toml`, and the time from
//! the fleet clock.

#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::holds::{missing_fast_gate, readings, Counts};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::{self, Finding, Floors, Reading};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::fleet_runtime::domain::pass::{observe_launch, Launch, LaunchStep, Proc};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::project_file;
use horch_core::fleetd::task_execution::adapters::leases_store;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, choices, dispatch_due, write_missing_task_files, write_task_files, Bound, CheckSandboxes,
    DispatchCtx, Dispatched,
};
use horch_core::fleetd::task_execution::application::lease;
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Hold, Holds};
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use horch_core::fleetd::worktree_custody::application::create;
use horch_core::runtime::machine::{self, ProbeBins};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, TaskKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use rusqlite::Connection;
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";
const SNAPSHOT: &str = "# A ticket\n\nDo the work.\n";

// ─── the world of a test ────────────────────────────────────────────────

/// Each persona has a check sandbox.
struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

/// Only the personas of the list have a check sandbox.
struct Only(&'static [&'static str]);

impl CheckSandboxes for Only {
    fn supported(&self, persona: &str) -> bool {
        self.0.contains(&persona)
    }
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    fleet_root: PathBuf,
    ledger: PathBuf,
    store: Store,
    run: Run,
    git: Git,
    config: FleetConfig,
}

/// Run git in `dir` for the test itself, with no hook and no config of the
/// user, and give its trimmed output.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "test",
    }
}

fn task(title: &str) -> NewTask {
    let mut t = NewTask::work(title, &format!("docs/{title}.md"), "opus");
    t.ticket_snapshot = Some(SNAPSHOT.into());
    t
}

impl World {
    /// A repository with 1 commit on `main` and the run branch, a ledger,
    /// a fleet root and a run of the repository with no task.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        let fleet_root = top.join("fleet");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&fleet_root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "base\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        git(&root, &["add", "README.md", ".gitignore"]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        git(&root, &["branch", RUN_BRANCH]);
        let ledger = top.join("ledger.sqlite3");
        let mut store = Store::open(&ledger).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        World {
            _tmp: tmp,
            root,
            fleet_root,
            ledger,
            store,
            run,
            git: Git::new("git"),
            config: FleetConfig::default(),
        }
    }

    /// Queue `t` in the run; gives its id.
    fn add(&mut self, t: NewTask) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id))
            .unwrap()
    }

    fn ctx(&self) -> DispatchCtx<'_> {
        DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &self.git,
        }
    }

    /// Bind `task_id` to a new attempt of `opus` in 1 transaction.
    fn bind(&mut self, task_id: &str) -> anyhow::Result<Bound> {
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &self.git,
        };
        self.store
            .immediate(|tx| bind(tx, &ctx, task_id, "opus", &NewWorker::default()))
    }

    /// 1 dispatch pass with `holds` and `sandboxes`, committed.
    fn dispatch(&mut self, holds: &Holds, sandboxes: &dyn CheckSandboxes) -> Dispatched {
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(holds.clock),
            git: &self.git,
        };
        self.store
            .immediate(|tx| dispatch_due(tx, &ctx, holds, sandboxes, 100))
            .unwrap()
    }

    /// The attempt `id` ends, and rung 6 queues its task again first in
    /// line, as the pass does after a crash.
    fn crash(&mut self, attempt: &str, task_id: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Attempt,
                    attempt,
                    "ended",
                    cause(),
                    &NeedsYouList,
                )?;
                commands::schedule_retry(tx, task_id, "test crash")
            })
            .unwrap();
    }

    fn task_status(&self, task_id: &str) -> String {
        tasks::get(self.store.conn(), task_id).unwrap().status
    }

    fn count(&self, sql: &str) -> i64 {
        self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Each attempt of `task_id`: id, state and `via`.
    fn attempts(&self, task_id: &str) -> Vec<(String, String, String)> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT id, state, via FROM attempts WHERE task_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn task_yaml(&self, task_id: &str, attempt: &str) -> PathBuf {
        attempt_dir(&self.fleet_root, task_id, attempt).join("task.yaml")
    }

    /// Run the `worktree_add` job of the `reserved` tree `id` and observe
    /// it, as the pass does.
    fn add_tree(&self, id: &str) {
        let mut conn = Connection::open(&self.ledger).unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        create::finish_reserved(&mut conn, &ctx, &TestJobs, id).unwrap();
    }
}

fn bound_ids(d: &Dispatched) -> Vec<String> {
    d.bound.iter().map(|b| b.task_id.clone()).collect()
}

fn held(d: &Dispatched, task_id: &str) -> Option<Hold> {
    d.held
        .iter()
        .find(|(t, _)| t == task_id)
        .map(|(_, h)| h.clone())
}

fn bind_order(c: &[Choice]) -> Vec<String> {
    c.iter()
        .filter_map(|c| match c {
            Choice::Bind { task_id, .. } => Some(task_id.clone()),
            Choice::Held { .. } => None,
        })
        .collect()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// The bind: `ready -> leased`, 1 worker row of the persona, 1 attempt in
/// `launching` with `via = push`, and a `reserved` tree with its
/// `worktree_add` job, all in 1 transaction. No pane and no process yet.
#[test]
fn bind_moves_ready_to_leased_with_1_launching_attempt() {
    let mut w = World::new();
    let id = w.add(task("first"));
    let b = w.bind(&id).unwrap();

    assert_eq!(w.task_status(&id), "leased");
    assert_eq!(b.attempt, 1);
    assert_eq!(
        w.attempts(&id),
        vec![(b.attempt_id.clone(), "launching".into(), "push".into())]
    );
    let t = tasks::get(w.store.conn(), &id).unwrap();
    assert_eq!(t.attempts, 1);
    assert_eq!(t.current_worker_id.as_deref(), Some(b.worker_id.as_str()));

    let worker = workers::get(w.store.conn(), &b.worker_id).unwrap();
    assert_eq!(worker.persona.as_deref(), Some("opus"));
    assert_eq!(worker.status, "spawning");
    assert_eq!(worker.task_id.as_deref(), Some(id.as_str()));
    assert_eq!(worker.agent_pid, None);
    assert_eq!(worker.pane_id, None);

    let tree = b.worktree.clone().expect("a worktree task gets a tree");
    let row = trees::get(w.store.conn(), &tree.worktree_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.state, WorktreeState::Reserved);
    assert_eq!(row.owner_attempt.as_deref(), Some(b.attempt_id.as_str()));
    assert_eq!(row.task_id.as_deref(), Some(id.as_str()));
    assert!(
        tree.add_job.is_some(),
        "the reserved row has its worktree_add job"
    );
    assert_eq!(row.path, tree.path);
    assert_eq!(row.branch, tree.branch);
    assert!(
        !Path::new(&tree.path).exists(),
        "the bind runs no git worktree add"
    );

    // The 2 files come after the commit, and they name the tree.
    let files = write_task_files(w.store.conn(), &w.fleet_root, &b.attempt_id, &[]).unwrap();
    let yaml = std::fs::read_to_string(&files.task_yaml).unwrap();
    assert!(
        yaml.contains(&format!("worktree: {}\n", tree.path)),
        "{yaml}"
    );
    assert!(
        yaml.contains(&format!("branch: {}\n", tree.branch)),
        "{yaml}"
    );
    assert!(yaml.contains("attempt: A1\n"), "{yaml}");
    assert!(yaml.contains("profile: opus\n"), "{yaml}");
    assert!(files.start_md.is_file());
}

/// FM-14 and L1: the bind is a compare-and-set on `ready`. 2 binds of 1
/// task at the same time give 1 attempt and 1 worker row.
#[test]
fn fm14_2_binds_of_1_task_give_1_attempt() {
    let mut w = World::new();
    let id = w.add(task("race"));
    let barrier = std::sync::Barrier::new(2);
    let results: Vec<anyhow::Result<Bound>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (ledger, barrier, config, git, id) =
                    (&w.ledger, &barrier, &w.config, &w.git, &id);
                s.spawn(move || {
                    let mut store = Store::open(ledger).unwrap();
                    let ctx = DispatchCtx {
                        jobs: JobCtx {
                            guard: None,
                            config,
                            needs_you: &NeedsYouList,
                        },
                        clock: FleetSeconds(0),
                        git,
                    };
                    barrier.wait();
                    store.immediate(|tx| bind(tx, &ctx, id, "opus", &NewWorker::default()))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let ok = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(ok, 1, "{results:?}");
    let refused = results.iter().find_map(|r| r.as_ref().err()).unwrap();
    assert!(refused.to_string().contains("not ready"), "{refused:#}");
    assert_eq!(w.attempts(&id).len(), 1);
    assert_eq!(w.count("SELECT count(*) FROM workers"), 1);
    assert_eq!(w.count("SELECT count(*) FROM worktrees"), 1);
    // A third bind after the commit is refused too.
    assert!(w.bind(&id).is_err());
    assert_eq!(w.attempts(&id).len(), 1);
}

/// L2 and L3: the target task goes first, then a retry (rung 6 puts it
/// first in line), then priority and seq.
#[test]
fn pres_04_l2_target_and_retry_go_first() {
    let mut w = World::new();
    let t1 = w.add(task("one"));
    let t2 = w.add(task("two"));
    let t3 = w.add(task("three"));
    let b = w.bind(&t2).unwrap();
    w.crash(&b.attempt_id, &t2);
    assert_eq!(w.task_status(&t2), "ready");

    let holds = Holds {
        clock: 1_000_000,
        ..Holds::default()
    };
    let order = choices(w.store.conn(), &holds, &AllSandboxes, None).unwrap();
    assert_eq!(bind_order(&order), vec![t2.clone(), t1.clone(), t3.clone()]);
    let order = choices(w.store.conn(), &holds, &AllSandboxes, Some(&t3)).unwrap();
    assert_eq!(bind_order(&order), vec![t3.clone(), t2.clone(), t1.clone()]);

    // The pass binds in that order: the retry gets attempt 2.
    let d = w.dispatch(&holds, &AllSandboxes);
    assert_eq!(bound_ids(&d), vec![t2.clone(), t1, t3]);
    assert_eq!(d.bound[0].attempt, 2);
}

/// L6: 2 tasks of 1 run with overlapping paths do not run together; an
/// exclusive task runs alone; shared-checkout tasks go 1 by 1; a review
/// task never waits on these rules.
#[test]
fn pres_04_l6_overlapping_paths_are_not_dispatched_together() {
    let mut w = World::new();
    let mut a = task("a");
    a.own = vec!["src/a".into()];
    let a = w.add(a);
    let mut b = task("b");
    b.own = vec!["src/a/b.rs".into()];
    let b = w.add(b);
    let mut c = task("c");
    c.own = vec!["src/c.rs".into()];
    let c = w.add(c);
    let mut review = task("review");
    review.kind = TaskKind::Review;
    review.own = vec!["src/a".into()];
    let review = w.add(review);
    let mut excl = task("exclusive");
    excl.parallel = false;
    let excl = w.add(excl);
    let mut s1 = task("shared-1");
    s1.worktree_mode = WorktreeMode::Shared;
    let s1 = w.add(s1);
    let mut s2 = task("shared-2");
    s2.worktree_mode = WorktreeMode::Shared;
    let s2 = w.add(s2);

    let d = w.dispatch(&Holds::default(), &AllSandboxes);
    assert_eq!(
        bound_ids(&d),
        vec![a.clone(), c.clone(), review, s1.clone()]
    );
    assert!(
        matches!(held(&d, &b), Some(Hold::PathHeld { ref path, .. }) if path == "src/a/b.rs"),
        "{:?}",
        held(&d, &b)
    );
    assert!(matches!(held(&d, &excl), Some(Hold::Exclusive { .. })));
    assert!(matches!(held(&d, &s2), Some(Hold::SharedCheckout { .. })));
    let shared = d.bound.iter().find(|x| x.task_id == s1).unwrap();
    assert_eq!(shared.worktree, None, "a shared-checkout task has no tree");

    // The next pass sees the bound tasks as running: nothing more binds.
    let d = w.dispatch(&Holds::default(), &AllSandboxes);
    assert!(d.bound.is_empty(), "{d:?}");
    assert!(matches!(held(&d, &b), Some(Hold::PathHeld { .. })));
    assert_eq!(w.task_status(&a), "leased");
    assert_eq!(w.task_status(&c), "leased");
}

/// A hold binds nothing, and each held task shows its cause: a health
/// floor from the machine file and the test `fleet.toml`, the missing fast
/// gate of the project, and `not_before` on the fleet clock.
#[test]
fn held_task_is_not_bound_and_shows_its_hold() {
    let mut w = World::new();
    let id = w.add(task("held"));

    // A health floor: 1 GB free on the disk of the machine file, and a
    // floor of 10 GB in the test fleet.toml.
    let machine_file = w.fleet_root.join("machine.json");
    std::fs::write(
        &machine_file,
        r#"{"os":"linux","arch":"x86_64","cpus":8,"mem_total_bytes":34359738368,
            "mem_available_bytes":17179869184,"disk_free_bytes":1000000000,
            "disk_total_bytes":500000000000,"max_open_files":10240,"max_processes":8000,
            "load_average_1m":1.0,"gpu":"none"}"#,
    )
    .unwrap();
    std::fs::write(
        w.fleet_root.join("fleet.toml"),
        "health_min_free_disk_gb = 10\n",
    )
    .unwrap();
    let config = config_file::load(&w.fleet_root).unwrap();
    let snapshot = machine::probe(&w.root, &ProbeBins::default(), Some(&machine_file));
    let counts = Counts {
        user_processes: Reading::Known(100),
        system_open_files: Reading::Known(100),
        system_open_file_limit: Reading::Known(100_000),
    };
    let global: Vec<String> = health::health(&readings(&snapshot, &counts), &Floors::from(&config))
        .into_iter()
        .filter_map(|f| match f {
            Finding::Hold(h) if h.global().is_some() => Some(h.cause().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(global.len(), 1, "{global:?}");
    let holds = Holds {
        global: global.clone(),
        ..Holds::default()
    };
    let d = w.dispatch(&holds, &AllSandboxes);
    assert!(d.bound.is_empty());
    assert_eq!(held(&d, &id), Some(Hold::Global(global[0].clone())));
    assert_eq!(w.task_status(&id), "ready");
    assert_eq!(w.attempts(&id).len(), 0);

    // The project has no [gates] in its fleet.toml.
    let settings = project_file::parse("").unwrap();
    let gate = missing_fast_gate(&w.run.project_id, &settings).expect("no fast gate");
    let holds = Holds {
        projects: BTreeMap::from([(gate.project.clone(), gate.cause.clone())]),
        ..Holds::default()
    };
    let d = w.dispatch(&holds, &AllSandboxes);
    assert!(d.bound.is_empty());
    let hold = held(&d, &id).unwrap();
    assert!(hold.to_string().contains(&gate.cause), "{hold}");

    // A retry backoff: the task waits for its not_before on the fleet clock.
    // fleetd runs at fleet clock 100 s, so rung 6 sets a not_before.
    w.store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s,
                clock_ticked_at)
             VALUES (1, 1, ?1, 0, '2026-01-01T00:00:00Z', 100, '2026-01-01T00:00:00Z')",
            [std::process::id()],
        )
        .unwrap();
    let b = w.bind(&id).unwrap();
    w.crash(&b.attempt_id, &id);
    let not_before = tasks::get(w.store.conn(), &id).unwrap().not_before.unwrap();
    let holds = Holds {
        clock: not_before - 7,
        ..Holds::default()
    };
    let d = w.dispatch(&holds, &AllSandboxes);
    assert_eq!(held(&d, &id), Some(Hold::NotBefore { seconds: 7 }));
    assert_eq!(held(&d, &id).unwrap().to_string(), "retry after 7 s");
    let holds = Holds {
        clock: not_before,
        ..Holds::default()
    };
    assert_eq!(bound_ids(&w.dispatch(&holds, &AllSandboxes)), vec![id]);
}

/// A persona whose check sandbox is NotSupported gets no dispatch; the
/// task of another persona binds.
#[test]
fn not_supported_check_sandbox_persona_is_not_bound() {
    let mut w = World::new();
    let opus = w.add(task("opus-task"));
    let mut t = task("codex-task");
    t.primary = "codex".into();
    let codex = w.add(t);

    let d = w.dispatch(&Holds::default(), &Only(&["codex"]));
    assert_eq!(bound_ids(&d), vec![codex.clone()]);
    assert_eq!(d.bound[0].persona, "codex");
    match held(&d, &opus) {
        Some(Hold::Persona { persona, cause }) => {
            assert_eq!(persona, "opus");
            assert!(cause.contains("check sandbox"), "{cause}");
        }
        other => panic!("expected the hold of the persona, got {other:?}"),
    }
    assert_eq!(w.task_status(&opus), "ready");
    assert_eq!(w.attempts(&opus).len(), 0);
}

/// The next attempt of a task is bound to the tree of the last attempt:
/// no new row, no new `worktree_add` job, and the new attempt owns it.
#[test]
fn next_attempt_is_bound_to_the_same_worktree() {
    let mut w = World::new();
    let id = w.add(task("again"));
    let first = w.bind(&id).unwrap();
    let tree = first.worktree.clone().unwrap();
    w.add_tree(&tree.worktree_id);
    let row = trees::get(w.store.conn(), &tree.worktree_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.state, WorktreeState::Active);
    std::fs::write(Path::new(&tree.path).join("work.txt"), "kept\n").unwrap();

    w.crash(&first.attempt_id, &id);
    let holds = Holds {
        clock: 1_000_000,
        ..Holds::default()
    };
    let d = w.dispatch(&holds, &AllSandboxes);
    assert_eq!(bound_ids(&d), vec![id.clone()]);
    let second = &d.bound[0];
    assert_eq!(second.attempt, 2);
    assert_ne!(second.worker_id, first.worker_id);
    let again = second.worktree.clone().unwrap();
    assert_eq!(again.worktree_id, tree.worktree_id);
    assert_eq!(again.path, tree.path);
    assert_eq!(again.branch, tree.branch);
    assert_eq!(
        again.add_job, None,
        "the tree exists: no new worktree_add job"
    );
    assert_eq!(w.count("SELECT count(*) FROM worktrees"), 1);
    let row = trees::get(w.store.conn(), &tree.worktree_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        row.owner_attempt.as_deref(),
        Some(second.attempt_id.as_str())
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&tree.path).join("work.txt")).unwrap(),
        "kept\n"
    );
    let files = write_task_files(w.store.conn(), &w.fleet_root, &second.attempt_id, &[]).unwrap();
    let yaml = std::fs::read_to_string(files.task_yaml).unwrap();
    assert!(
        yaml.contains(&format!("worktree: {}\n", tree.path)),
        "{yaml}"
    );
    assert!(yaml.contains("attempt: A2\n"), "{yaml}");
}

/// The crash test. A fault before the commit leaves no row: the task stays
/// `ready`, and the next pass binds it 1 time. A fault after the commit and
/// before the 2 files: the next pass writes them 1 time, and a pass after
/// that writes nothing and binds nothing.
#[test]
fn task_files_of_a_launching_attempt_are_written_after_a_crash() {
    let mut w = World::new();
    let id = w.add(task("crash"));

    // Fault point 1: before the commit of the intended rows.
    let ctx = w.ctx();
    let mut store = Store::open(&w.ledger).unwrap();
    let crashed: anyhow::Result<()> = store.immediate(|tx| {
        bind(tx, &ctx, &id, "opus", &NewWorker::default())?;
        anyhow::bail!("fleetd dies before the commit")
    });
    assert!(crashed.is_err());
    assert_eq!(w.task_status(&id), "ready");
    assert_eq!(w.attempts(&id).len(), 0);
    assert_eq!(w.count("SELECT count(*) FROM workers"), 0);
    assert_eq!(w.count("SELECT count(*) FROM worktrees"), 0);
    assert_eq!(w.count("SELECT count(*) FROM jobs"), 0);

    // The next pass binds it 1 time. Fault point 2: fleetd dies after the
    // commit and before it writes the 2 files.
    let d = w.dispatch(&Holds::default(), &AllSandboxes);
    assert_eq!(bound_ids(&d), vec![id.clone()]);
    let attempt = d.bound[0].attempt_id.clone();
    assert!(!w.task_yaml(&id, "A1").exists());

    // The next pass writes the 2 files of the launching attempt 1 time.
    let written = write_missing_task_files(w.store.conn(), &w.fleet_root, &[]).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, attempt);
    let files = written[0].1.as_ref().unwrap();
    assert_eq!(files.task_yaml, w.task_yaml(&id, "A1"));
    let bytes = std::fs::read(&files.task_yaml).unwrap();
    assert!(files.start_md.is_file());

    // A pass after that writes nothing, binds nothing and queues no job.
    let again = write_missing_task_files(w.store.conn(), &w.fleet_root, &[]).unwrap();
    assert!(again.is_empty(), "{again:?}");
    assert_eq!(std::fs::read(w.task_yaml(&id, "A1")).unwrap(), bytes);
    let d = w.dispatch(&Holds::default(), &AllSandboxes);
    assert!(d.bound.is_empty());
    assert_eq!(w.attempts(&id).len(), 1);
    assert_eq!(
        w.count("SELECT count(*) FROM jobs WHERE kind = 'worktree_add'"),
        1
    );
    // A write after the files exist is the same write: it changes nothing.
    write_task_files(w.store.conn(), &w.fleet_root, &attempt, &[]).unwrap();
    assert_eq!(std::fs::read(w.task_yaml(&id, "A1")).unwrap(), bytes);
}

/// The bind starts no lease and no start timeout: they start at the
/// launch. A bound attempt with no launch waits for its launch however
/// long its `worktree_add` job takes; the start timeout counts from the
/// launch, and the lease that the launch takes counts from the launch.
#[test]
fn lease_starts_at_the_launch_not_at_the_bind() {
    let mut w = World::new();
    let id = w.add(task("lease"));
    let b = w.bind(&id).unwrap();
    assert_eq!(
        leases_store::get(w.store.conn(), &id).unwrap(),
        None,
        "no lease at the bind"
    );
    let launched_at: Option<i64> = w
        .store
        .conn()
        .query_row(
            "SELECT pid FROM attempts WHERE id = ?1",
            [&b.attempt_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(launched_at, None, "no process at the bind");

    // Long after the bind, an attempt with no launch is due to launch,
    // not timed out.
    let late = FleetSeconds(10_000);
    assert_eq!(observe_launch(Proc::Absent, None, late), LaunchStep::Launch);
    // The start timeout counts from the launch.
    let launch = Launch {
        at: late,
        process: Proc::Absent,
    };
    assert_eq!(
        observe_launch(Proc::Absent, Some(launch), FleetSeconds(late.0 + 89)),
        LaunchStep::Wait
    );
    // The lease that the launch takes counts from the launch.
    w.store
        .immediate(|tx| lease::take(tx, late, &id, &b.attempt_id, lease::claim_lease_s()))
        .unwrap();
    let row = leases_store::get(w.store.conn(), &id).unwrap().unwrap();
    assert_eq!(row.attempt_id, b.attempt_id);
    assert_eq!(row.expires_at, late.0 + lease::claim_lease_s());
}
