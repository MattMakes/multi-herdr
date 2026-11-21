use std::path::Path;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{self, ProcessFact, ScanClock};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobKey, JobRunner, Queued, SandboxProfiles, Stop,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State, KINDS};
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::integration::adapters::executors as integration_executors;
use horch_core::fleetd::integration::adapters::sqlite::{self as integration_rows, NewFeature};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
}

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

    fn lose_run(&mut self, runner: &mut JobRunner<'_>, id: i64, clock: i64) {
        self.start_due(runner, clock, "true", &[]);
        assert_eq!(self.row(id).state, State::Running);
        let facts = self.wait_gone(runner, id);
        let seen = self.observe(&facts, clock);
        assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    }

    fn active(&self, kind: Kind, subject: &str) -> Option<JobRow> {
        jobs_store::active(&self.conn, kind, subject).unwrap()
    }

    fn task(&mut self, check: &str) -> String {
        self.task_of(check, Some("claude"))
    }

    fn task_of(&mut self, check: &str, harness: Option<&str>) -> String {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM runs", [], |r| r.get(0))
            .unwrap();
        let run = runs::create(
            &self.conn,
            Path::new("/repo"),
            &NewRun {
                slug: format!("r{n}"),
                ..Default::default()
            },
        )
        .unwrap();
        horch_core::taskledger::schema::run_idea(
            &self.conn,
            &run.run_id,
            &horch_core::taskledger::util::now(),
            true,
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

    fn run_job(&mut self, runner: &mut JobRunner<'_>, id: i64, clock: i64) -> State {
        self.run_job_in(runner, id, clock, &TestProfile)
    }

    fn run_job_in(
        &mut self,
        runner: &mut JobRunner<'_>,
        id: i64,
        clock: i64,
        profiles: &dyn SandboxProfiles,
    ) -> State {
        self.start_due(runner, clock, "true", &[]);
        let me = self.row(id).pid.unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let written = jobs::run_job(
            &ctx,
            &self.conn,
            id,
            me,
            &CheckRunner {
                host: &Launcher,
                profiles,
                sweep: None,
            },
        )
        .unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.row(id).state
    }
}

struct TestProfile;

impl SandboxProfiles for TestProfile {
    fn check_launch(
        &self,
        _worker: &Worker,
        dir: &Path,
        check: &str,
    ) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(Some(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), check.into()],
            cwd: Some(dir.to_path_buf()),
            env_rule: Default::default(),
            detach: false,
            output: None,
        }))
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

    let green = w.task("true");
    let id = new_id(w.queue(Kind::TaskCheck, &green, "tip1", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Passed);
    let Queued::Reused(row) = w.queue(Kind::TaskCheck, &green, "tip1", 5) else {
        panic!("the passed check is used again");
    };
    assert_eq!(row.id, id);
    assert!(outcome(&row).unwrap().passed);

    let red = w.task("false");
    let id = new_id(w.queue(Kind::TaskCheck, &red, "tip1", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Failed);
    assert!(outcome(&w.row(id)).unwrap().verdict);
    assert!(matches!(
        w.queue(Kind::TaskCheck, &red, "tip1", 5),
        Queued::Reused(r) if r.id == id
    ));

    let id = new_id(w.queue(Kind::Push, "F-1", "tipA", 0));
    w.start_due(&mut runner, 0, "true", &[]);
    let facts = w.wait_gone(&mut runner, id);
    let ok = r#"{"passed":true,"verdict":true,"text":"pushed"}"#;
    jobs_store::set_result(&w.conn, id, ok).unwrap();
    assert_eq!(w.observe(&facts, 1).ended, vec![(id, State::Passed)]);
    let again = new_id(w.queue(Kind::Push, "F-1", "tipA", 2));
    assert_eq!(w.row(again).run, 2);
    assert_eq!(w.row(again).state, State::Queued);

    let id = new_id(w.queue(Kind::DocsSync, "claude", "1.0", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Failed);
    let o = outcome(&w.row(id)).unwrap();
    assert!(!o.verdict);
    assert_eq!(o.text, "no executor before phase 7 for docs_sync");
    let again = new_id(w.queue(Kind::DocsSync, "claude", "1.0", 2));
    assert_eq!(w.row(again).run, 2);
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
fn each_job_kind_has_a_time_limit() {
    let config = FleetConfig::default();
    let none: Vec<Kind> = KINDS
        .iter()
        .map(|r| r.kind)
        .filter(|&k| jobs::time_limit_s(k, &config).is_none())
        .collect();
    assert_eq!(none, Vec::<Kind>::new(), "kinds with no time limit");
    let limit = |k| jobs::time_limit_s(k, &config).unwrap();
    assert_eq!(limit(Kind::TaskCheck), 1200);
    assert_eq!(limit(Kind::Fetch), 180);
    assert_eq!(limit(Kind::Push), 600);
    assert_eq!(limit(Kind::FastGate), 1200);
    assert_eq!(limit(Kind::FullGate), 3600);
    assert_eq!(limit(Kind::TrialMerge), 180);
    assert_eq!(limit(Kind::BundleBuild), 600);
    assert_eq!(limit(Kind::Rebase), 600);

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(config_file::FILE_NAME),
        "[job_limit_s]\nfast_gate = 11\nfull_gate = 12\ntrial_merge = 13\n\
         bundle_build = 14\nrebase = 15\n",
    )
    .unwrap();
    let file = config_file::load(dir.path()).unwrap();
    let got: Vec<u64> = [
        Kind::FastGate,
        Kind::FullGate,
        Kind::TrialMerge,
        Kind::BundleBuild,
        Kind::Rebase,
    ]
    .map(|k| jobs::time_limit_s(k, &file).unwrap())
    .to_vec();
    assert_eq!(got, vec![11, 12, 13, 14, 15]);
}

#[test]
fn integration_kinds_route_to_the_integration_router() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    w.tx(|_, tx| {
        let new = NewFeature {
            id: "F-1",
            name: None,
            branch: Some("fleet/f-1"),
            primary_branch: "main",
        };
        let cause = Cause {
            actor: Actor::Horch,
            text: "feature accepted",
        };
        integration_rows::insert_feature(tx, None, &new, cause)
    })
    .unwrap();
    let routed = [
        Kind::BundleBuild,
        Kind::FastGate,
        Kind::FullGate,
        Kind::Fetch,
        Kind::Push,
        Kind::Rebase,
    ];
    for kind in routed {
        let id = new_id(w.queue(kind, "F-1", "tip1", 0));
        w.run_job(&mut runner, id, 0);
        let row = w.row(id);
        let want = integration_executors::run(&w.conn, &row).unwrap().outcome;
        assert_eq!(outcome(&row), Some(want), "{kind}");
    }
    let id = new_id(w.queue(Kind::TrialMerge, "B-1", "tips", 0));
    assert_eq!(w.run_job(&mut runner, id, 0), State::Failed);
    let got = outcome(&w.row(id)).unwrap();
    assert!(!got.verdict);
    assert_eq!(got.text, "the input key \"tips\" is not 2 tips");
}

#[cfg(unix)]
#[test]
fn check_runs_only_in_the_sandbox_profile_of_its_worker() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
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

    for (harness, why) in [
        (None, "has no worker"),
        (Some("claude"), "has no sandbox profile"),
    ] {
        let task = w.task_of("touch ran-outside", harness);
        let id = new_id(w.queue(Kind::TaskCheck, &task, "tip1", 0));
        assert_eq!(w.run_job_in(&mut runner, id, 0, &codex), State::Failed);
        let o = outcome(&w.row(id)).unwrap();
        assert!(!o.verdict);
        assert!(
            o.text.starts_with("SR-4: ") && o.text.contains(why),
            "{}",
            o.text
        );
        let wt = w.dir.path().join(format!("wt-{task}"));
        assert!(!wt.join("ran-outside").exists(), "no check ran");
    }
}
