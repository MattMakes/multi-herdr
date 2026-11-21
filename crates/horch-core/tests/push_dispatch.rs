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

use horch_core::fleetd::fleet_runtime::adapters::{config_file, effects};
use horch_core::fleetd::fleet_runtime::application::holds::{missing_fast_gate, readings, Counts};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::{self, Finding, Floors, Reading};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::fleet_runtime::domain::pass::{observe_launch, Launch, LaunchStep, Proc};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::project_file;
use horch_core::fleetd::task_execution::adapters::leases_store;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, choices, dispatch_due, read_base_tips, write_missing_task_files, write_task_files,
    BaseTips, Bound, CheckSandboxes, DispatchCtx, Dispatched,
};
use horch_core::fleetd::task_execution::application::launch as push_launch;
use horch_core::fleetd::task_execution::application::lease;
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Hold, Holds};
use horch_core::fleetd::worker_profiles::application::ports::{
    EnvRule, LaunchError, LaunchSpec, Launched, ProcessHost,
};
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
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
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

fn tips_ctx<'a>(config: &'a FleetConfig, tips: &'a BaseTips) -> DispatchCtx<'a> {
    DispatchCtx {
        jobs: JobCtx {
            guard: None,
            config,
            needs_you: &NeedsYouList,
        },
        clock: FleetSeconds(0),
        git: tips,
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

#[test]
fn bind_uses_pre_read_sandboxed_base_tips_and_runs_no_git_in_write_tx() {
    let mut w = World::new();
    w.git = Git::new("/nonexistent/git-must-not-run-inside-tx");
    let id = w.add(task("pre-read"));
    let gw = CliGitGateway::new(effects::integration_git(&w.fleet_root, &[w.root.clone()]));

    git(&w.root, &["config", "core.fsmonitor", "evil"]);
    let refused = read_base_tips(w.store.conn(), &gw).unwrap_err();
    assert!(
        format!("{refused:#}").contains("core.fsmonitor"),
        "{refused:#}"
    );
    git(&w.root, &["config", "--unset", "core.fsmonitor"]);

    let none = BaseTips::new();
    let ctx = tips_ctx(&w.config, &none);
    let d = w
        .store
        .immediate(|tx| dispatch_due(tx, &ctx, &Holds::default(), &AllSandboxes, 100))
        .unwrap();
    assert_eq!(d.failed.len(), 1, "{:?}", d.failed);
    assert_eq!(d.failed[0].0, id);
    assert!(d.failed[0].1.contains(RUN_BRANCH), "{}", d.failed[0].1);
    assert_eq!(w.task_status(&id), "ready");
    assert!(w.attempts(&id).is_empty());

    let tip = git(&w.root, &["rev-parse", RUN_BRANCH]);
    let tips = read_base_tips(w.store.conn(), &gw).unwrap();
    let key = (w.run.project_root.clone(), RUN_BRANCH.to_string());
    assert_eq!(tips, BaseTips::from([(key, tip.clone())]));

    git(
        &w.root,
        &["commit", "--quiet", "--allow-empty", "-m", "later"],
    );
    git(&w.root, &["branch", "--force", RUN_BRANCH, "HEAD"]);
    let ctx = tips_ctx(&w.config, &tips);
    let dot_git = w.root.join(".git");
    let hidden = w.root.join(".git-hidden-in-tx");
    let b = w
        .store
        .immediate(|tx| {
            std::fs::rename(&dot_git, &hidden)?;
            let b = bind(tx, &ctx, &id, "opus", &NewWorker::default());
            std::fs::rename(&hidden, &dot_git)?;
            b
        })
        .unwrap();

    assert_eq!(w.task_status(&id), "leased");
    let row = trees::of_task(w.store.conn(), &id).unwrap().unwrap();
    assert_eq!(Some(row.worktree_id), b.worktree.map(|t| t.worktree_id));
    assert_eq!(row.base, RUN_BRANCH);
    assert_eq!(row.base_commit, Some(tip));
}

#[derive(Default)]
struct RefusingHost {
    calls: std::cell::Cell<usize>,
}

impl push_launch::Launches for RefusingHost {
    fn spec(&self, l: &push_launch::PushLaunch) -> anyhow::Result<LaunchSpec> {
        Ok(LaunchSpec {
            program: "horch".into(),
            args: vec!["spawn".into(), "--attempt".into(), l.attempt.clone().into()],
            cwd: None,
            env_rule: EnvRule::default(),
            detach: false,
            output: None,
        })
    }
}

impl ProcessHost for RefusingHost {
    fn launch(&self, _: &LaunchSpec) -> Result<Launched, LaunchError> {
        self.calls.set(self.calls.get() + 1);
        Err(LaunchError::Io(std::io::Error::other("the OS refused")))
    }
}

/// Rung 6 (design.md, "The stall ladder", rung 6): a launch whose host
/// call fails ends its attempt `crashed` in the same pass, as a reported
/// failed spawn does (T-254). It does not wait for the start timeout. The
/// next attempt follows the backoff of rung 6 (30 s, 120 s on the fleet
/// clock), and the `max_attempts` of the run bounds the loop: the last
/// failure moves the task to `failed`, and no pass launches it again.
#[test]
fn failed_host_launch_ends_the_attempt_at_once_and_rung_6_bounds_the_retries() {
    let mut w = World::new();
    w.store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
            rusqlite::params![std::process::id(), horch_core::taskledger::util::now()],
        )
        .unwrap();
    let set_clock = |w: &World, s: i64| {
        w.store
            .conn()
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    };
    let mut t = task("refused");
    t.worktree_mode = WorktreeMode::Shared;
    let id = w.add(t);
    let limit = w.run.max_attempts;
    assert!(limit >= 2, "the run allows a retry: max_attempts {limit}");
    let backoff = [30_i64, 120, 480];
    let host = RefusingHost::default();
    let mut clock = 0_i64;
    for n in 1..=limit {
        set_clock(&w, clock);
        let d = w.dispatch(
            &Holds {
                clock,
                ..Holds::default()
            },
            &AllSandboxes,
        );
        assert_eq!(bound_ids(&d), vec![id.clone()], "attempt {n} at {clock}");
        let attempt = d.bound[0].attempt_id.clone();
        let worker = d.bound[0].worker_id.clone();

        let r = push_launch::launch(
            w.store.conn(),
            &w.fleet_root,
            FleetSeconds(clock),
            &attempt,
            &host,
            &host,
        );
        assert!(r.is_err(), "the pass reports the failed launch");
        assert_eq!(host.calls.get(), n as usize);

        assert_eq!(
            w.attempts(&id).last().map(|a| (a.0.clone(), a.1.clone())),
            Some((attempt.clone(), "ended".to_string()))
        );
        let outcome: Option<String> = w
            .store
            .conn()
            .query_row(
                "SELECT outcome FROM attempts WHERE id = ?1",
                [&attempt],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome.as_deref(), Some("crashed"));
        assert_eq!(
            workers::get(w.store.conn(), &worker).unwrap().status,
            "launch_failed"
        );
        let row = tasks::get(w.store.conn(), &id).unwrap();
        assert_eq!(row.attempts, n);
        if n == limit {
            assert_eq!(row.status, "failed", "max_attempts {limit} ends the loop");
            break;
        }
        let wait = backoff[(n as usize - 1).min(backoff.len() - 1)];
        assert_eq!(row.status, "ready");
        assert_eq!(row.not_before, Some(clock + wait), "the backoff of rung 6");

        let early = w.dispatch(
            &Holds {
                clock: clock + wait - 1,
                ..Holds::default()
            },
            &AllSandboxes,
        );
        assert!(early.bound.is_empty(), "attempt {} waits", n + 1);
        clock += wait;
    }

    set_clock(&w, clock + 10_000);
    let d = w.dispatch(
        &Holds {
            clock: clock + 10_000,
            ..Holds::default()
        },
        &AllSandboxes,
    );
    assert!(d.bound.is_empty());
    assert_eq!(host.calls.get(), limit as usize);
    assert_eq!(w.attempts(&id).len(), limit as usize);
}

// ─── a large ready queue (T-313) ────────────────────────────────────────

/// `text` with each task id of the ledger of `w` as its `T-<seq>`.
fn with_shorts(w: &World, text: String) -> String {
    let mut stmt = w
        .store
        .conn()
        .prepare("SELECT task_id, seq FROM tasks")
        .unwrap();
    let ids: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    ids.iter().fold(text, |t, (id, seq)| {
        t.replace(id.as_str(), &format!("T-{seq}"))
    })
}

/// A world with `n` ready tasks, each with its own Own file.
fn ready_queue(n: usize) -> World {
    let w = World::new();
    let run = w.run.clone();
    let items: Vec<(String, NewTask)> = (0..n)
        .map(|i| {
            let mut t = task(&format!("q{i}"));
            t.own = vec![format!("src/q{i}.rs")];
            (format!("q{i}"), t)
        })
        .collect();
    let mut w = w;
    w.store
        .immediate(|tx| Ok(tasks::add_many(tx, &run, &items, Actor::Orchestrator)?.len()))
        .unwrap();
    w
}

/// A32 (T-313): the choice and the read of the base tips run the same
/// number of statements for 30 and for 3000 ready tasks.
#[test]
fn dispatch_reads_of_a_large_ready_queue_run_a_fixed_number_of_statements() {
    let count = |n: usize| {
        let w = ready_queue(n);
        let conn = w.store.conn();
        let (chosen, choice_statements) =
            horch_core::taskledger::testing::count_statements(conn, || {
                choices(conn, &Holds::default(), &AllSandboxes, None).unwrap()
            });
        assert_eq!(bind_order(&chosen).len(), n);
        let gw = CliGitGateway::new(effects::integration_git(
            &w.fleet_root,
            std::slice::from_ref(&w.root),
        ));
        let (tips, tip_statements) =
            horch_core::taskledger::testing::count_statements(conn, || {
                read_base_tips(conn, &gw).unwrap()
            });
        assert_eq!(tips.len(), 1);
        (choice_statements, tip_statements)
    };
    assert_eq!(count(30), count(3000));
}

/// T-313: on a mixed queue the choices, the binds of a pass of 2 slots
/// and the base tips are the values of the code that read each ready task
/// by itself, recorded before the change.
#[test]
fn dispatch_choices_of_a_mixed_queue_are_the_recorded_choices() {
    let mut w = World::new();
    let cancelled = w.add(task("cancelled"));
    let parked = w.add(task("parked"));
    let open = w.add(task("open"));
    let rescue = w.add(task("rescue"));
    let mut same_1 = task("same-1");
    same_1.own = vec!["src/x.rs".into()];
    w.add(same_1);
    let mut same_2 = task("same-2");
    same_2.own = vec!["src/x.rs".into()];
    w.add(same_2);
    let mut codex_1 = task("codex-1");
    codex_1.primary = "codex".into();
    codex_1.own = vec!["src/c1.rs".into()];
    w.add(codex_1);
    let mut codex_2 = task("codex-2");
    codex_2.primary = "codex".into();
    codex_2.own = vec!["src/c2.rs".into()];
    let codex_2 = w.add(codex_2);

    let b = w.bind(&open).unwrap();
    let b2 = w.bind(&rescue).unwrap();
    w.crash(&b2.attempt_id, &rescue);
    let conn = w.store.conn();
    conn.execute(
        "UPDATE tasks SET cancel_requested = 'test cancel' WHERE task_id = ?1",
        [&cancelled],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
         SELECT 'I-parked', feature_id, 'parked', 'now', 'now' FROM ideas LIMIT 1",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET idea_id = 'I-parked' WHERE task_id = ?1",
        [&parked],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET status = 'ready' WHERE task_id = ?1",
        [&open],
    )
    .unwrap();
    conn.execute(
        "UPDATE worktrees SET state = 'orphaned' WHERE task_id = ?1",
        [&rescue],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET priority = -1 WHERE task_id = ?1",
        [&codex_2],
    )
    .unwrap();
    assert!(b.worktree.is_some());

    let holds = Holds {
        clock: 1_000_000,
        ..Holds::default()
    };
    let chosen = choices(w.store.conn(), &holds, &AllSandboxes, None).unwrap();
    let chosen: Vec<String> = chosen.iter().map(|c| format!("{c:?}")).collect();
    assert_eq!(
        with_shorts(&w, chosen.join("\n")),
        [
            r#"Held { task_id: "T-2", hold: IdeaParked { idea: "I-parked" } }"#,
            r#"Held { task_id: "T-4", hold: TreeRescue }"#,
            r#"Bind { task_id: "T-8", persona: "codex" }"#,
            r#"Held { task_id: "T-3", hold: OpenAttempt { attempt: "T-3/A1" } }"#,
            r#"Bind { task_id: "T-5", persona: "opus" }"#,
            r#"Held { task_id: "T-6", hold: PathHeld { path: "src/x.rs", by: "T-5" } }"#,
            r#"Bind { task_id: "T-7", persona: "codex" }"#,
        ]
        .join("\n")
    );

    let gw = CliGitGateway::new(effects::integration_git(
        &w.fleet_root,
        std::slice::from_ref(&w.root),
    ));
    let tips = read_base_tips(w.store.conn(), &gw).unwrap();
    let bases: Vec<&str> = tips.keys().map(|(_, base)| base.as_str()).collect();
    assert_eq!(bases, [RUN_BRANCH]);

    let ctx = tips_ctx(&w.config, &tips);
    let d = w
        .store
        .immediate(|tx| dispatch_due(tx, &ctx, &holds, &AllSandboxes, 2))
        .unwrap();
    let bound: Vec<_> = d.bound.iter().map(|b| (&b.task_id, &b.persona)).collect();
    assert_eq!(
        with_shorts(&w, format!("{bound:?}\n{:?}\n{:?}", d.held, d.failed)),
        [
            r#"[("T-8", "codex"), ("T-5", "opus")]"#,
            r#"[("T-2", IdeaParked { idea: "I-parked" }), ("T-4", TreeRescue), ("T-3", OpenAttempt { attempt: "T-3/A1" }), ("T-6", PathHeld { path: "src/x.rs", by: "T-5" })]"#,
            "[]",
        ]
        .join("\n")
    );
}
