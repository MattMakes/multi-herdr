//! The `push` job (docs/specs/fleetd/design.md, "Push and pull request
//! results are facts on the feature row"; "The kinds of job": `push`; I9;
//! FM-43; F11; docs/tickets/fleetd-p4/38-push-job.md).
//!
//! Each test makes its own temporary ledger, a test `fleet.toml` beside it,
//! a local bare repository as the remote, and a clone of it as the
//! repository of the project. The fleet clock is the `fleetd` row of the
//! ledger, set by the test. A job process is `true`, started through the
//! launcher; the executor runs in the test process with the recorded pid,
//! as `horch job run` runs it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, FeatureFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::remote;
use horch_core::fleetd::integration::domain::project::Remote;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use rusqlite::{Connection, TransactionBehavior};

const FEATURE: &str = "F-0001";
const BRANCH: &str = "fleet/run/x";
const FLEET_TOML: &str = "push_failure_limit = 3\njob_backoff_s = [30, 120, 480]\n";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    /// The bare repository that is the remote `origin`.
    remote: PathBuf,
    /// The clone: the repository of the project, with the feature branch.
    repo: PathBuf,
    /// A second clone that writes commits to the remote as someone else.
    writer: PathBuf,
}

/// A temp ledger of the current schema with the fleet clock at 0, a
/// `fleet.toml`, the feature and its branch row, a remote with `main`, and
/// a local feature branch 1 commit past `main` that the remote does not
/// have. The feature has an open pull request, so the push rule wants it.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    std::fs::create_dir_all(dir.path().join("fleet")).unwrap();
    std::fs::write(dir.path().join("fleet/fleet.toml"), FLEET_TOML).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, 1, 1, 'now', 0, 'now')",
        [],
    )
    .unwrap();

    let remote = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", path(&remote)],
    );
    // A reflog on the remote counts each update of each of its refs.
    git(&remote, &["config", "core.logAllRefUpdates", "always"]);
    let writer = dir.path().join("writer");
    git(dir.path(), &["clone", "-q", path(&remote), path(&writer)]);
    commit(&writer, "a.txt", "a\n");
    git(&writer, &["push", "-q", "origin", "HEAD:main"]);
    let repo = dir.path().join("repo");
    git(dir.path(), &["clone", "-q", path(&remote), path(&repo)]);
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["checkout", "-q", "-b", BRANCH]);
    commit(&repo, "f.txt", "f\n");

    let config = FleetConfig {
        push_failure_limit: 3,
        job_backoff_s: vec![30, 120, 480],
        ..FleetConfig::default()
    };
    let mut w = World {
        dir,
        conn,
        config,
        remote,
        repo,
        writer,
    };
    w.tx(|tx| {
        let new = NewFeature {
            id: FEATURE,
            name: Some("x"),
            branch: Some(BRANCH),
            primary_branch: "main",
        };
        rows::insert_feature(tx, None, &new, cause("feature accepted"))?;
        let branch = NewBranch {
            name: BRANCH,
            feature_id: FEATURE,
            task_id: None,
            owner: "fleetd",
            due_at: 0,
        };
        rows::insert_branch(tx, None, &branch, cause("feature branch"))?;
        rows::set_feature_fact(
            tx,
            None,
            FEATURE,
            FeatureFact::PrState("open"),
            cause("the pull request is open"),
        )
    });
    w.see_local_tip();
    w
}

impl World {
    fn repo(&self) -> &str {
        path(&self.repo)
    }

    fn tx(&mut self, f: impl FnOnce(&Connection) -> anyhow::Result<()>) {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        f(&tx).unwrap();
        tx.commit().unwrap();
    }

    /// Set the fleet clock of the `fleetd` row.
    fn clock(&self, clock: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
            .unwrap();
    }

    /// The local tip of the feature branch.
    fn local_tip(&self) -> String {
        git(&self.repo, &["rev-parse", &format!("refs/heads/{BRANCH}")])
    }

    /// Record the local tip of the feature branch as `seen_tip`, as the
    /// observation of a pass does.
    fn see_local_tip(&mut self) {
        let tip = self.local_tip();
        self.tx(|tx| {
            rows::set_branch_fact(
                tx,
                None,
                BRANCH,
                BranchFact::SeenTip(&tip),
                cause("the pass saw the tip"),
            )
        });
    }

    /// The tip of `branch` on the remote, or `None`.
    fn remote_tip(&self, branch: &str) -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.remote)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("refs/heads/{branch}"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The count of updates of the feature branch on the remote.
    fn remote_updates(&self) -> usize {
        if self.remote_tip(BRANCH).is_none() {
            return 0;
        }
        git(
            &self.remote,
            &[
                "reflog",
                "show",
                "--format=%H",
                &format!("refs/heads/{BRANCH}"),
            ],
        )
        .lines()
        .count()
    }

    /// Rewrite the local feature branch: a new commit in place of its tip.
    fn rewrite_local(&mut self) {
        git(&self.repo, &["commit", "-q", "--amend", "-m", "rewritten"]);
        self.see_local_tip();
    }

    /// A commit of someone else on the feature branch of the remote.
    fn someone_else_pushes(&self) {
        git(&self.writer, &["fetch", "-q", "origin"]);
        git(
            &self.writer,
            &["checkout", "-q", "-B", BRANCH, &format!("origin/{BRANCH}")],
        );
        commit(&self.writer, "other.txt", "other\n");
        git(
            &self.writer,
            &["push", "-q", "origin", &format!("HEAD:{BRANCH}")],
        );
    }

    /// Make the remote unreachable, or reachable again.
    fn remote_down(&self, down: bool) {
        let off = self.dir.path().join("remote.off");
        let (from, to) = if down {
            (&self.remote, &off)
        } else {
            (&off, &self.remote)
        };
        std::fs::rename(from, to).unwrap();
    }

    /// Queue the push of the feature with the push rule at `clock`.
    fn queue(&mut self, clock: i64) -> Option<Queued> {
        let config = self.config.clone();
        let repo = self.repo().to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let queued = remote::queue_push(
            &ctx(&config),
            &tx,
            &repo,
            FEATURE,
            &Remote::default(),
            false,
            FleetSeconds(clock),
        )
        .unwrap();
        tx.commit().unwrap();
        queued
    }

    /// Queue the push at `clock` and give the id of the new run.
    fn queue_new(&mut self, clock: i64) -> i64 {
        match self.queue(clock) {
            Some(Queued::New(id)) => id,
            other => panic!("no new push run at {clock} s: {other:?}"),
        }
    }

    /// Start the process of the run `id` at `clock`: `true`, through the
    /// launcher. Give its pid.
    fn start(&mut self, id: i64, clock: i64) -> u32 {
        let config = self.config.clone();
        let job = jobs_store::get(&self.conn, id).unwrap().unwrap();
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        assert!(JobRunner::new(&Launcher)
            .start(
                &ctx(&config),
                &mut self.conn,
                &job,
                &spec,
                FleetSeconds(clock)
            )
            .unwrap());
        jobs_store::get(&self.conn, id)
            .unwrap()
            .unwrap()
            .pid
            .unwrap()
    }

    /// Start the run `id` at `clock` and run its executor to its end, with
    /// the fleet clock at `clock`. Give the ended row.
    fn run(&mut self, id: i64, clock: i64) -> JobRow {
        self.clock(clock);
        let pid = self.start(id, clock);
        let config = self.config.clone();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        jobs::run_job(&ctx(&config), &mut self.conn, id, pid, &checks).unwrap();
        jobs::end_from_files(&ctx(&config), &mut self.conn).unwrap();
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Queue a push at `clock` and run it to its end.
    fn push(&mut self, clock: i64) -> JobRow {
        let id = self.queue_new(clock);
        self.run(id, clock)
    }

    /// The run that the rule of abandoned runs queued after the abandoned
    /// run `lost` at 11 s: the same key, the next run number, and the
    /// backoff of 30 s. The push rule queues no second run beside it.
    fn next_after_abandoned(&mut self, lost: i64) -> i64 {
        let lost = jobs_store::get(&self.conn, lost).unwrap().unwrap();
        let Some(Queued::Active(next)) = self.queue(11) else {
            panic!("no run after the abandoned run {}", lost.id);
        };
        assert_eq!(next.input_key, lost.input_key);
        assert_eq!(next.run, lost.run + 1);
        assert_eq!(next.not_before, Some(FleetSeconds(41)));
        next.id
    }

    fn feature(&self) -> rows::FeatureRow {
        rows::feature(&self.conn, FEATURE).unwrap().unwrap()
    }

    fn branch(&self) -> rows::BranchRow {
        rows::branch(&self.conn, BRANCH).unwrap().unwrap()
    }

    /// The keys of the open needs-you items.
    fn open_items(&self) -> Vec<String> {
        needs_you::open_items(&self.conn)
            .unwrap()
            .into_iter()
            .map(|i| i.key)
            .collect()
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn text(row: &JobRow) -> String {
    jobs::outcome(row).map(|o| o.text).unwrap_or_default()
}

/// No worker of a test has a sandbox profile.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, body: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn push_sets_ok_and_the_seen_remote_tip() {
    let mut w = world();
    let tip = w.local_tip();
    let row = w.push(10);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));
    let f = w.feature();
    assert_eq!(f.push_state.as_deref(), Some("ok"));
    assert_eq!(f.push_failures, 0);
    assert_eq!(f.push_not_before, None);
    assert_eq!(w.branch().seen_remote_tip.as_deref(), Some(tip.as_str()));
    // The remote has the tip that fleetd saw: nothing more to push.
    assert_eq!(w.queue(20), None);
}

#[test]
fn rewrite_uses_force_with_lease_on_the_seen_remote_tip() {
    let mut w = world();
    let first = w.local_tip();
    assert_eq!(w.push(10).state, State::Passed);
    w.rewrite_local();
    let second = w.local_tip();
    let id = w.queue_new(20);
    let job = jobs_store::get(&w.conn, id).unwrap().unwrap();
    let input = remote::PushInput::parse(&job.input_key).unwrap();
    assert_eq!(input.tip, second);
    assert_eq!(input.seen.as_deref(), Some(first.as_str()));
    let row = w.run(id, 20);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(second.as_str()));
    assert_eq!(w.branch().seen_remote_tip.as_deref(), Some(second.as_str()));
}

#[test]
fn remote_moved_by_someone_else_is_not_overwritten() {
    let mut w = world();
    let first = w.local_tip();
    assert_eq!(w.push(10).state, State::Passed);
    w.someone_else_pushes();
    let theirs = w.remote_tip(BRANCH).unwrap();
    assert_ne!(theirs, first);
    w.rewrite_local();
    let row = w.push(20);
    assert_eq!(row.state, State::Failed);
    assert!(text(&row).contains("stale info"), "{}", text(&row));
    // The commit of someone else stays, and fleetd still knows only its
    // own last push.
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(theirs.as_str()));
    assert_eq!(w.branch().seen_remote_tip.as_deref(), Some(first.as_str()));
    let f = w.feature();
    assert_eq!(f.push_state.as_deref(), Some("failed"));
    assert_eq!(f.push_failures, 1);
}

#[test]
fn transient_failure_is_followed_by_a_new_run_after_the_backoff() {
    let mut w = world();
    let tip = w.local_tip();
    w.remote_down(true);
    let failed = w.push(100);
    assert_eq!(failed.state, State::Failed);
    let f = w.feature();
    assert_eq!(f.push_state.as_deref(), Some("failed"));
    assert_eq!(f.push_failures, 1);
    assert_eq!(f.push_not_before, Some(130));

    // No run before the end of the backoff on the fleet clock.
    assert_eq!(w.queue(129), None);
    w.remote_down(false);
    let id = w.queue_new(130);
    let again = jobs_store::get(&w.conn, id).unwrap().unwrap();
    // A new run of the same key: the failed result is not used again.
    assert_ne!(again.id, failed.id);
    assert_eq!(again.input_key, failed.input_key);
    assert_eq!(again.run, failed.run + 1);
    let row = w.run(id, 130);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));
    let f = w.feature();
    assert_eq!(f.push_state.as_deref(), Some("ok"));
    assert_eq!(f.push_failures, 0);
    assert_eq!(f.push_not_before, None);
}

#[test]
fn _3_failures_open_1_needs_you_item() {
    let mut w = world();
    let key = remote::push_failed_key(FEATURE);
    w.remote_down(true);
    let mut clock = 0;
    for (n, wait) in [(1, 30), (2, 120)] {
        assert_eq!(w.push(clock).state, State::Failed);
        assert_eq!(w.feature().push_failures, n);
        assert!(w.open_items().is_empty(), "an item after {n} failures");
        clock += wait;
        assert_eq!(w.feature().push_not_before, Some(clock));
    }
    assert_eq!(w.push(clock).state, State::Failed);
    assert_eq!(w.feature().push_failures, 3);
    assert_eq!(w.open_items(), vec![key.clone()]);
    // A 4th failure keeps the 1 item; the backoff stays at its last value.
    clock += 480;
    assert_eq!(w.push(clock).state, State::Failed);
    assert_eq!(w.open_items(), vec![key]);
    assert_eq!(w.feature().push_not_before, Some(clock + 480));

    // A passed push ends the item.
    w.remote_down(false);
    clock += 480;
    assert_eq!(w.push(clock).state, State::Passed);
    assert!(w.open_items().is_empty());
    assert_eq!(w.feature().push_failures, 0);
}

#[test]
fn push_runs_no_hook_or_program_of_the_project() {
    let mut w = world();
    let marker = w.dir.path().join("hook-ran");
    let hook = w.repo.join(".git/hooks/pre-push");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    horch_core::runtime::process::make_executable(&hook).unwrap();
    let tip = w.local_tip();
    let row = w.push(10);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert!(!marker.exists(), "the pre-push hook of the project ran");
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));

    // A key of the repository config that starts a program ends the job.
    let ssh_marker = w.dir.path().join("ssh-ran");
    git(
        &w.repo,
        &[
            "config",
            "core.sshCommand",
            &format!("touch '{}'", ssh_marker.display()),
        ],
    );
    w.rewrite_local();
    let row = w.push(20);
    assert_eq!(row.state, State::Failed);
    assert!(
        text(&row).to_ascii_lowercase().contains("core.sshcommand"),
        "{}",
        text(&row)
    );
    assert!(!ssh_marker.exists());
    // The job changed nothing on the remote.
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));
    assert_eq!(w.feature().push_state.as_deref(), Some("failed"));
}

/// Step 3 of the ticket: a fault before the commit of the queued run, and
/// a fault after that commit and before the push. In each case the next
/// pass pushes 1 time, and no tip is lost.
#[test]
fn a_crash_before_or_after_the_queue_commit_pushes_1_time() {
    // Fault 1: the transaction of the queued run never commits.
    let mut w = world();
    let tip = w.local_tip();
    {
        let config = w.config.clone();
        let repo = w.repo().to_string();
        let tx = w
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let queued = remote::queue_push(
            &ctx(&config),
            &tx,
            &repo,
            FEATURE,
            &Remote::default(),
            false,
            FleetSeconds(10),
        )
        .unwrap();
        assert!(matches!(queued, Some(Queued::New(_))));
        drop(tx);
    }
    let count: i64 = w
        .conn
        .query_row("SELECT COUNT(*) FROM jobs WHERE kind = 'push'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(w.push(20).state, State::Passed);
    assert_eq!(w.remote_updates(), 1);
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));

    // Fault 2: the queued run commits, and its process ends before the
    // push. The pass sees the process gone: the run is abandoned, and the
    // next run comes after the backoff of an abandoned run.
    let mut w = world();
    let tip = w.local_tip();
    let id = w.queue_new(10);
    w.start(id, 10);
    let config = w.config.clone();
    let facts = [ProcessFact {
        key: id.to_string(),
        liveness: Liveness::Dead,
    }];
    w.tx(|tx| jobs::observe(&ctx(&config), tx, &facts, FleetSeconds(11)).map(|_| ()));
    let lost = jobs_store::get(&w.conn, id).unwrap().unwrap();
    assert_eq!(lost.state, State::Abandoned);
    assert_eq!(w.remote_updates(), 0);
    // An abandoned run is not a failed push.
    assert_eq!(w.feature().push_failures, 0);
    let next = w.next_after_abandoned(id);
    let row = w.run(next, 41);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert_eq!(w.remote_updates(), 1);
    assert_eq!(w.remote_tip(BRANCH).as_deref(), Some(tip.as_str()));
    assert_eq!(w.branch().seen_remote_tip.as_deref(), Some(tip.as_str()));
    assert_eq!(w.local_tip(), tip);

    // Fault 3: the push reached the remote, and the process ended before
    // it recorded the push. The next run of the same key finds the remote
    // at its tip: it passes with no second update.
    let mut w = world();
    let tip = w.local_tip();
    let id = w.queue_new(10);
    w.start(id, 10);
    git(
        &w.repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{tip}:refs/heads/{BRANCH}"),
        ],
    );
    let config = w.config.clone();
    let facts = [ProcessFact {
        key: id.to_string(),
        liveness: Liveness::Dead,
    }];
    w.tx(|tx| jobs::observe(&ctx(&config), tx, &facts, FleetSeconds(11)).map(|_| ()));
    let next = w.next_after_abandoned(id);
    let row = w.run(next, 41);
    assert_eq!(row.state, State::Passed, "{}", text(&row));
    assert_eq!(w.remote_updates(), 1);
    assert_eq!(w.branch().seen_remote_tip.as_deref(), Some(tip.as_str()));
}
