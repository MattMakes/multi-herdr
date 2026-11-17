//! The jobs table at work: the queue, the job process, the observation of
//! each run, the bound on abandoned runs and `requeue`
//! (docs/specs/fleetd/design.md, "Long work is a job. A pass never waits
//! for it" and "The kinds of job"; docs/tickets/fleetd-p1/17-jobs.md).
//!
//! Each test uses a temp ledger. A job process is a small command (`true`,
//! `sleep`) started through the launcher.

use std::path::Path;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{self, ProcessFact, ScanClock};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, JobCtx, JobKey, JobRunner, Queued, Stop,
};
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
}

/// A temp ledger of the current schema with the default `fleet.toml`.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        dir,
        conn,
        config: FleetConfig::default(),
    }
}

impl World {
    /// Run `f` in 1 committed transaction, as part 2 of a pass.
    fn tx<T>(&mut self, f: impl FnOnce(&JobCtx<'_>, &Transaction<'_>) -> T) -> T {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let out = f(&ctx, &tx);
        tx.commit().unwrap();
        out
    }

    fn queue(&mut self, kind: Kind, subject: &str, input_key: &str, clock: i64) -> Queued {
        let key = JobKey {
            kind,
            subject,
            input_key,
        };
        self.tx(|ctx, tx| jobs::queue(ctx, tx, &key, Some("/repo"), FleetSeconds(clock)))
            .unwrap()
    }

    fn requeue(&mut self, kind: Kind, subject: &str, input_key: &str) -> Queued {
        let key = JobKey {
            kind,
            subject,
            input_key,
        };
        self.tx(|ctx, tx| jobs::requeue(ctx, tx, &key)).unwrap()
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn rows(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
            .unwrap()
    }

    fn due(&self, clock: i64) -> Vec<i64> {
        jobs::due_to_start(&self.conn, FleetSeconds(clock))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    /// Part 3: start each due run as `program args`, at `clock`.
    fn start_due(&mut self, runner: &mut JobRunner<'_>, clock: i64, program: &str, args: &[&str]) {
        let spec = LaunchSpec {
            program: program.into(),
            args: args.iter().map(Into::into).collect(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let due = jobs::due_to_start(&self.conn, FleetSeconds(clock)).unwrap();
        assert!(!due.is_empty(), "a run is due at {clock}");
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        for job in due {
            let started = runner
                .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(clock))
                .unwrap();
            assert!(started);
        }
    }

    /// Part 1: the process facts of the running runs.
    fn facts(&self) -> Vec<ProcessFact> {
        let items = jobs::processes(&self.conn).unwrap();
        let start = Instant::now();
        let mut now = Instant::now;
        let mut clock = ScanClock {
            deadline: Deadline::pass(start, 20).scan(start, 2000),
            now: &mut now,
            stamp: "pass",
        };
        scans::scan_processes(&items, &mut Cursor::new("pass"), &mut clock).facts
    }

    /// Reap and scan until the process of the run `id` is gone.
    fn wait_gone(&self, runner: &mut JobRunner<'_>, id: i64) -> Vec<ProcessFact> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            runner.reap();
            let facts = self.facts();
            let key = id.to_string();
            let fact = facts.iter().find(|f| f.key == key).expect("a fact");
            if fact.liveness == scans::Liveness::Dead {
                return facts;
            }
            assert!(Instant::now() < until, "the process of job {id} ended");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn observe(&mut self, facts: &[ProcessFact], clock: i64) -> jobs::Observed {
        self.tx(|ctx, tx| jobs::observe(ctx, tx, facts, FleetSeconds(clock)))
            .unwrap()
    }

    /// 1 run of the due run `id` whose process is `true`: it ends with no
    /// result, so a pass at `clock` finds it gone.
    fn lose_run(&mut self, runner: &mut JobRunner<'_>, id: i64, clock: i64) {
        self.start_due(runner, clock, "true", &[]);
        assert_eq!(self.row(id).state, State::Running);
        let facts = self.wait_gone(runner, id);
        let seen = self.observe(&facts, clock);
        assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    }

    /// The queued run of `kind` and `subject`.
    fn active(&self, kind: Kind, subject: &str) -> Option<JobRow> {
        jobs_store::active(&self.conn, kind, subject).unwrap()
    }

    /// A task with the check `check` and a worktree folder, held by a
    /// worker of the Claude harness.
    fn task(&mut self, check: &str) -> String {
        self.task_of(check, Some("claude"))
    }

    /// A task with the check `check` and a worktree folder, held by a
    /// worker of `harness`, or by no worker.
    fn task_of(&mut self, check: &str, harness: Option<&str>) -> String {
        let run = runs::create(
            &self.conn,
            Path::new("/repo"),
            &NewRun {
                slug: format!("r-{check}"),
                ..Default::default()
            },
        )
        .unwrap();
        let mut new = NewTask::work("check", "check.md", "dev");
        new.land_check = Some(check.into());
        let task = tasks::add(&self.conn, &run, &new, Actor::Orchestrator).unwrap();
        if let Some(harness) = harness {
            let worker = NewWorker {
                run_id: run.run_id.clone(),
                persona: "dev".into(),
                harness: Some(harness.into()),
                ..Default::default()
            };
            let worker = workers::insert(&self.conn, &worker).unwrap();
            self.conn
                .execute(
                    "UPDATE tasks SET current_worker_id = ?2 WHERE task_id = ?1",
                    params![task.task_id, worker],
                )
                .unwrap();
        }
        let wt = self.dir.path().join(format!("wt-{}", task.task_id));
        std::fs::create_dir_all(&wt).unwrap();
        self.conn
            .execute(
                "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base,
                    status, origin, created_at)
                 VALUES (?1, ?1, '/repo', ?2, 'fleet/t', 'main', 'open', 'native', ?3)",
                params![task.task_id, wt.to_string_lossy(), util::now()],
            )
            .unwrap();
        task.task_id
    }

    /// The job process of the run `id`: started as `true` through the
    /// launcher, then `run_job` in this process with the recorded pid.
    fn run_job(&mut self, runner: &mut JobRunner<'_>, id: i64, clock: i64) -> State {
        self.run_job_in(runner, id, clock, &CodexSandbox::default())
    }

    /// [`World::run_job`] with the Codex sandbox `codex`.
    fn run_job_in(
        &mut self,
        runner: &mut JobRunner<'_>,
        id: i64,
        clock: i64,
        codex: &CodexSandbox,
    ) -> State {
        self.start_due(runner, clock, "true", &[]);
        let me = self.row(id).pid.unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, me, codex).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        self.row(id).state
    }
}

fn new_id(q: Queued) -> i64 {
    match q {
        Queued::New(id) => id,
        other => panic!("expected a new run, got {other:?}"),
    }
}

#[test]
fn pure_job_result_is_used_again_and_effect_job_runs_again() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);

    // A pure run that passed is used again for its input key.
    let green = w.task("true");
    let id = new_id(w.queue(Kind::TaskCheck, &green, "tip1", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Passed);
    let Queued::Reused(row) = w.queue(Kind::TaskCheck, &green, "tip1", 5) else {
        panic!("the passed check is used again");
    };
    assert_eq!(row.id, id);
    assert!(outcome(&row).unwrap().passed);

    // A pure run that failed with a verdict is used again too.
    let red = w.task("false");
    let id = new_id(w.queue(Kind::TaskCheck, &red, "tip1", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Failed);
    assert!(outcome(&w.row(id)).unwrap().verdict);
    assert!(matches!(
        w.queue(Kind::TaskCheck, &red, "tip1", 5),
        Queued::Reused(r) if r.id == id
    ));

    // An effect run that passed is never used again: the next need is the
    // next run of the same key.
    let id = new_id(w.queue(Kind::Push, "F-1", "tipA", 0));
    w.start_due(&mut runner, 0, "true", &[]);
    let facts = w.wait_gone(&mut runner, id);
    let ok = r#"{"passed":true,"verdict":true,"text":"pushed"}"#;
    jobs_store::set_result(&w.conn, id, ok).unwrap();
    assert_eq!(w.observe(&facts, 1).ended, vec![(id, State::Passed)]);
    let again = new_id(w.queue(Kind::Push, "F-1", "tipA", 2));
    assert_eq!(w.row(again).run, 2);
    assert_eq!(w.row(again).state, State::Queued);

    // A kind with no executor in Phase 1 fails with no verdict, so even a
    // pure kind runs again.
    let id = new_id(w.queue(Kind::Fetch, "repo", "origin", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Failed);
    let o = outcome(&w.row(id)).unwrap();
    assert!(!o.verdict);
    assert_eq!(o.text, "no executor before phase 4 for fetch");
}

#[test]
fn job_abandoned_3_times_goes_on_the_needs_you_list_and_requeue_resets_it() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let item = "job_abandoned:push:F-1:tipA";

    let first = new_id(w.queue(Kind::Push, "F-1", "tipA", 0));
    w.lose_run(&mut runner, first, 0);
    let second = w.active(Kind::Push, "F-1").unwrap();
    assert_eq!((second.run, second.abandoned_runs), (2, 1));
    assert_eq!(second.not_before, Some(FleetSeconds(30)));
    assert!(w.due(29).is_empty(), "the backoff holds the run");
    assert_eq!(w.due(30), vec![second.id]);

    w.lose_run(&mut runner, second.id, 30);
    let third = w.active(Kind::Push, "F-1").unwrap();
    assert_eq!((third.run, third.abandoned_runs), (3, 2));
    assert_eq!(third.not_before, Some(FleetSeconds(150)));

    w.lose_run(&mut runner, third.id, 150);
    assert_eq!(w.active(Kind::Push, "F-1"), None, "no new run starts");
    assert_eq!(w.row(third.id).abandoned_runs, 3);
    let open = open_items(&w.conn).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].key, item);
    assert_eq!(open[0].entity_id, third.id.to_string());
    assert!(open[0]
        .text
        .contains(&format!("horch job requeue {}", third.id)));
    assert!(matches!(
        w.queue(Kind::Push, "F-1", "tipA", 500),
        Queued::Held(r) if r.id == third.id
    ));
    assert!(w.due(10_000).is_empty());

    let fourth = new_id(w.requeue(Kind::Push, "F-1", "tipA"));
    let row = w.row(fourth);
    assert_eq!((row.run, row.abandoned_runs, row.not_before), (4, 0, None));
    assert!(
        open_items(&w.conn).unwrap().is_empty(),
        "requeue resolves the item"
    );
    assert_eq!(w.due(150), vec![fourth]);
    // The count starts again: the next lost run waits 30 s, not 480 s.
    w.lose_run(&mut runner, fourth, 150);
    let fifth = w.active(Kind::Push, "F-1").unwrap();
    assert_eq!(fifth.abandoned_runs, 1);
    assert_eq!(fifth.not_before, Some(FleetSeconds(180)));
}

#[test]
fn running_job_with_a_lost_process_runs_again_after_restart() {
    let mut w = world();
    let launcher = Launcher;
    let id = new_id(w.queue(Kind::WorktreeList, "/repo", "/repo", 0));
    let row = {
        let mut runner = JobRunner::new(&launcher);
        w.start_due(&mut runner, 0, "sleep", &["60"]);
        let row = w.row(id);
        assert_eq!(row.state, State::Running);
        // fleetd stops, and the job process is lost with it.
        let lost = Stop {
            job_id: id,
            pid: row.pid.unwrap(),
            pid_start: row.pid_start,
            kill: true,
        };
        assert!(jobs::stop(&lost));
        w.wait_gone(&mut runner, id);
        row
    };
    assert_eq!(row.deadline, Some(FleetSeconds(600)));

    // A new fleetd: a new connection and a new runner.
    let path = w.dir.path().join("ledger.sqlite3");
    w.conn = Connection::open(&path).unwrap();
    let facts = w.facts();
    let seen = w.observe(&facts, 7);
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    let next = w.active(Kind::WorktreeList, "/repo").unwrap();
    assert_eq!((next.run, next.abandoned_runs), (2, 1));
    assert_eq!(next.not_before, Some(FleetSeconds(37)));
    assert!(w.due(36).is_empty());
    assert_eq!(w.due(37), vec![next.id]);
}

#[test]
fn job_past_its_deadline_is_marked_to_stop_and_ends_abandoned() {
    let mut w = world();
    w.config.job_limit_s.worktree_list = 10;
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = new_id(w.queue(Kind::WorktreeList, "/repo", "/repo", 0));
    w.start_due(&mut runner, 0, "sleep", &["60"]);
    assert_eq!(w.row(id).deadline, Some(FleetSeconds(10)));

    let facts = w.facts();
    let seen = w.observe(&facts, 9);
    assert!(seen.stops.is_empty() && seen.ended.is_empty());
    let seen = w.observe(&facts, 10);
    let stop = seen.stops.clone();
    assert_eq!(stop.len(), 1);
    assert_eq!((stop[0].job_id, stop[0].kill), (id, false));
    assert_eq!(w.row(id).state, State::Running, "the stop is an effect");
    let late = w.observe(&facts, 30);
    assert!(late.stops[0].kill, "past stop_grace_s the group is killed");

    // Part 3 stops the group; the next pass finds the process gone.
    assert!(jobs::stop(&stop[0]));
    let facts = w.wait_gone(&mut runner, id);
    let seen = w.observe(&facts, 31);
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    assert_eq!(w.row(id).state, State::Abandoned);
    let next = w.active(Kind::WorktreeList, "/repo").unwrap();
    assert_eq!(next.not_before, Some(FleetSeconds(61)));
}

#[test]
fn second_queue_of_1_kind_and_subject_while_1_runs_adds_no_row() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = new_id(w.queue(Kind::TaskCheck, "T-1", "tip1", 0));
    assert!(matches!(w.queue(Kind::TaskCheck, "T-1", "tip1", 0), Queued::Active(r) if r.id == id));
    assert!(matches!(w.queue(Kind::TaskCheck, "T-1", "tip2", 0), Queued::Active(r) if r.id == id));
    w.start_due(&mut runner, 0, "sleep", &["60"]);
    assert!(matches!(w.queue(Kind::TaskCheck, "T-1", "tip2", 1), Queued::Active(r) if r.id == id));
    assert_eq!(w.rows(), 1);
    // Another subject or another kind is another job.
    new_id(w.queue(Kind::TaskCheck, "T-2", "tip1", 1));
    new_id(w.queue(Kind::Fetch, "T-1", "tip1", 1));
    assert_eq!(w.rows(), 3);

    let row = w.row(id);
    let end = Stop {
        job_id: id,
        pid: row.pid.unwrap(),
        pid_start: row.pid_start,
        kill: true,
    };
    assert!(jobs::stop(&end));
    w.wait_gone(&mut runner, id);
}

#[test]
fn new_input_key_starts_a_new_count() {
    let mut w = world();
    w.config.job_backoff_s = vec![30];
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let mut id = new_id(w.queue(Kind::WorktreeRemove, "wt-1", "head1", 0));
    for clock in [0, 30, 60] {
        w.lose_run(&mut runner, id, clock);
        if let Some(next) = w.active(Kind::WorktreeRemove, "wt-1") {
            id = next.id;
        }
    }
    assert_eq!(w.active(Kind::WorktreeRemove, "wt-1"), None);
    assert!(matches!(
        w.queue(Kind::WorktreeRemove, "wt-1", "head1", 90),
        Queued::Held(_)
    ));

    let other = new_id(w.queue(Kind::WorktreeRemove, "wt-1", "head2", 90));
    let row = w.row(other);
    assert_eq!((row.run, row.abandoned_runs, row.not_before), (1, 0, None));
    w.lose_run(&mut runner, other, 90);
    let next = w.active(Kind::WorktreeRemove, "wt-1").unwrap();
    assert_eq!((next.run, next.abandoned_runs), (2, 1));
    assert_eq!(next.not_before, Some(FleetSeconds(120)));
}

#[test]
fn kind_with_no_time_limit_is_not_queued() {
    let mut w = world();
    let key = JobKey {
        kind: Kind::FastGate,
        subject: "B-1",
        input_key: "tips",
    };
    let err = w
        .tx(|ctx, tx| jobs::queue(ctx, tx, &key, Some("/repo"), FleetSeconds(0)))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "a fast_gate job has no time limit before phase 4"
    );
    assert_eq!(w.rows(), 0);
}

/// A Codex worker wrote the code that its check runs, so the check runs in
/// the sandbox of its pane, as `finish` runs it (#181). A task with no
/// worker gets no check.
#[cfg(unix)]
#[test]
fn check_of_a_codex_worker_runs_in_its_sandbox() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    // A stand-in for `codex`: it records its args and runs nothing.
    let args = w.dir.path().join("codex-args");
    let bin = w.dir.path().join("codex");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", args.display());
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let codex = CodexSandbox {
        bin,
        state_root: w.dir.path().join("state"),
        personas: Vec::new(),
    };

    let task = w.task_of("touch ran-outside", Some("codex"));
    let id = new_id(w.queue(Kind::TaskCheck, &task, "tip1", 0));
    assert_eq!(w.run_job_in(&mut runner, id, 0, &codex), State::Passed);
    let wt = w.dir.path().join(format!("wt-{task}"));
    let seen = std::fs::read_to_string(&args).expect("the check ran through codex");
    let seen: Vec<&str> = seen.lines().collect();
    assert_eq!(seen[0], "sandbox");
    let dir = seen
        .iter()
        .position(|a| *a == "-C")
        .expect("a project root");
    assert_eq!(seen[dir + 1], wt.to_string_lossy());
    assert_eq!(seen[seen.len() - 2..], ["-c", "touch ran-outside"]);
    assert!(
        !wt.join("ran-outside").exists(),
        "no check ran outside the sandbox"
    );

    let orphan = w.task_of("true", None);
    let id = new_id(w.queue(Kind::TaskCheck, &orphan, "tip1", 0));
    assert_eq!(w.run_job_in(&mut runner, id, 0, &codex), State::Failed);
    let o = outcome(&w.row(id)).unwrap();
    assert!(!o.verdict);
    assert!(o.text.contains("has no worker"), "{}", o.text);
}
