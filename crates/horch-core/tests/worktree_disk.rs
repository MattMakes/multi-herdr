//! The worktree disk budget: over it, fleetd takes the `landed` and
//! `rescued` trees of finished runs, oldest first, never an `active` tree,
//! and holds new dispatch with 1 needs-you item when that is not enough
//! (docs/specs/fleetd/design.md, "Worktrees: leased, reconciled, never
//! lost" ("Disk"), FM-28, OR-1; docs/tickets/fleetd-p3/16-worktree-disk-budget.md).
//!
//! The budget queues 1 `worktree_remove` job for each tree that it removes
//! (MAP.md of fleetd-p3, Q9; docs/tickets/fleetd-p3/18-reconciler-acts.md).
//!
//! Each test has its own temp ledger, fleet root and git repository. The
//! sizes are values of `bytes_on_disk`, as the `worktree_list` job stores
//! them: no test writes gigabytes.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::adapters::scans::{FolderItem, Liveness, ProcessFact};
use horch_core::fleetd::fleet_runtime::application::holds::worktree_disk;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::fleetd::worktree_custody::application::disk::{decision, disk_budget, Report};
use horch_core::fleetd::worktree_custody::application::remove::queue_remove;
use horch_core::fleetd::worktree_custody::domain::disk::GB;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, TransactionBehavior};

const CLOCK: FleetSeconds = FleetSeconds(1_000);

// ─── the world ──────────────────────────────────────────────────────────

/// A temp ledger at generation 1, a fleet root, and a git repository with
/// 1 commit on `main`.
struct World {
    _temp: tempfile::TempDir,
    top: PathBuf,
    repo: PathBuf,
    conn: Connection,
    config: FleetConfig,
    runs: u32,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let top = temp.path().canonicalize().unwrap();
    let mut conn = Connection::open(top.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    std::fs::create_dir_all(top.join("fleet")).unwrap();
    let repo = top.join("repo");
    std::fs::create_dir_all(repo.join(".worktrees")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join(".gitignore"), ".worktrees/\n").unwrap();
    git(&repo, &["add", "a.txt", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    World {
        _temp: temp,
        top,
        repo,
        conn,
        config: FleetConfig::default(),
        runs: 0,
    }
}

/// Run git in `dir` with no config of the user; the trimmed output. A
/// failed call fails the test.
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "a test",
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

impl World {
    fn sql(&self, sql: &str, args: &[&str]) {
        self.conn
            .execute(sql, rusqlite::params_from_iter(args))
            .unwrap();
    }

    /// A run of the repository in `status`. A run is made `active` and
    /// then moved, so the runs of 1 test do not refuse each other.
    fn run(&mut self, status: &str) -> Run {
        self.runs += 1;
        let new = NewRun {
            slug: format!("r{}", self.runs),
            base_branch: Some("main".into()),
            run_branch: Some(format!("fleet/r{}", self.runs)),
            ..Default::default()
        };
        let run = runs::create(&self.conn, &self.repo, &new).unwrap();
        self.sql(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            &[&run.run_id, status],
        );
        run
    }

    /// A task of `run`.
    fn task(&self, run: &Run, name: &str) -> String {
        let t = NewTask::work(name, &format!("{name}.md"), "opus");
        tasks::add(&self.conn, run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    /// A row `id` in `state`, of the task `task`, made at `created_at`,
    /// with `gb` GB on disk. Its folder is `<repo>/.worktrees/<id>`.
    fn row(&self, id: &str, task: Option<&str>, state: &str, created_at: &str, gb: u64) {
        let new = NewWorktree {
            worktree_id: id.to_string(),
            task_id: task.map(str::to_string),
            worker_id: None,
            repo_root: self.repo.display().to_string(),
            path: self.repo.join(".worktrees").join(id).display().to_string(),
            branch: format!("fleet/{id}"),
            base: "main".to_string(),
            base_commit: None,
            status: WorktreeStatus::Open,
            mode: None,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        trees::insert_orphan(&tx, None, &new, cause()).unwrap();
        tx.commit().unwrap();
        self.conn
            .execute(
                "UPDATE worktrees SET state = ?2, created_at = ?3, bytes_on_disk = ?4
                  WHERE worktree_id = ?1",
                params![id, state, created_at, i64::try_from(gb * GB).unwrap()],
            )
            .unwrap();
    }

    fn set_gb(&self, id: &str, gb: u64) {
        self.conn
            .execute(
                "UPDATE worktrees SET bytes_on_disk = ?2 WHERE worktree_id = ?1",
                params![id, i64::try_from(gb * GB).unwrap()],
            )
            .unwrap();
    }

    fn budget(&mut self) -> Report {
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let report = disk_budget(&ctx(&config), &tx, CLOCK).unwrap();
        tx.commit().unwrap();
        report
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn state(&self, id: &str) -> String {
        trees::get(&self.conn, id)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            .to_string()
    }

    /// The open needs-you items of the budget: (key, text).
    fn disk_items(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT key, text FROM needs_you
                  WHERE kind = 'worktree_disk' AND state = 'open' ORDER BY key",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn removal_jobs(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove'")
    }

    /// A passed `worktree_remove` run of the tree `id`, ended before the
    /// next pass: the reconciler of that pass observes it.
    fn removal_passed(&mut self, id: &str) {
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let row = trees::get(&tx, id).unwrap().unwrap();
        let Ok(Queued::New(job)) =
            queue_remove(&ctx(&config), &tx, &row, "a test", false, CLOCK).unwrap()
        else {
            panic!("no new worktree_remove run of {id}");
        };
        let key = job.to_string();
        transition(
            &tx,
            None,
            Entity::Job,
            &key,
            "running",
            cause(),
            &NeedsYouList,
        )
        .unwrap();
        jobs_store::set_started(&tx, job, 4_000_000, None, FleetSeconds(1_000_000)).unwrap();
        let result = serde_json::json!({
            "passed": true,
            "verdict": true,
            "text": "removed",
            "data": {
                "path": row.path,
                "branch": row.branch,
                "forced": false,
                "ignored_files": 0,
                "branch_deleted": false,
            },
        });
        jobs_store::set_result(&tx, job, &result.to_string()).unwrap();
        let gone = ProcessFact {
            key,
            liveness: Liveness::Dead,
        };
        let seen = jobs::observe(&ctx(&config), &tx, &[gone], CLOCK).unwrap();
        assert_eq!(seen.ended, vec![(job, State::Passed)]);
        tx.commit().unwrap();
    }

    fn pass<'a>(&self, outside: &'a dyn Outside, host: &'a dyn ProcessHost) -> Pass<'a> {
        let ports = Ports {
            outside,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host,
        };
        Pass::new(self.settings(), ports)
    }

    /// The `hold_started` and `hold_ended` events: (kind, data).
    fn hold_events(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kind, data_json FROM events
                  WHERE kind IN ('hold_started', 'hold_ended') ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn settings(&self) -> Settings {
        Settings {
            fleet_root: self.top.join("fleet"),
            generation: Generation(1),
            config: self.config.clone(),
            horch: PathBuf::from("/bin/false"),
            faults: Faults::default(),
        }
    }
}

// ─── the fakes of the pass ──────────────────────────────────────────────

/// No herdr, no health hold, no tree folder and no repository: the pass
/// runs only its ledger steps.
struct Nothing;

impl Outside for Nothing {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no launch of attempt {} in these tests", attempt.id)
    }
}

struct NoLaunch;

impl ProcessHost for NoLaunch {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        panic!("no launch in these tests: {:?}", spec.program)
    }
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first() {
    let mut w = world();
    let done = w.run("done");
    let abandoned = w.run("abandoned");
    let active = w.run("active");
    let t1 = w.task(&done, "t1");
    let t2 = w.task(&abandoned, "t2");
    let t3 = w.task(&done, "t3");
    let t4 = w.task(&active, "t4");
    // 45 GB against the budget of 20 GB.
    w.row("newest", Some(&t3), "landed", "2026-01-05T00:00:00Z", 5);
    w.row("oldest", Some(&t2), "rescued", "2026-01-01T00:00:00Z", 5);
    w.row("middle", Some(&t1), "landed", "2026-01-03T00:00:00Z", 5);
    w.row("no-task", None, "rescued", "2026-01-02T00:00:00Z", 5);
    w.row("live-run", Some(&t4), "landed", "2026-01-01T00:00:00Z", 15);
    w.row("orphan", None, "orphaned", "2026-01-01T00:00:00Z", 10);
    let report = w.budget();
    assert_eq!(report.total, 45 * GB);
    assert_eq!(report.budget, 20 * GB);
    assert_eq!(report.removals, ["oldest", "middle", "newest"]);
    assert!(report.hold.is_some(), "30 GB stays after the 3 spare trees");
    for id in ["oldest", "middle", "newest"] {
        assert_eq!(
            w.count(&format!(
                "SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove' AND subject = '{id}'"
            )),
            1,
            "{id}"
        );
    }
    assert_eq!(w.removal_jobs(), 3);
    // Under a budget of 40 GB the oldest spare tree is enough.
    let d = decision(&w.conn, 40).unwrap();
    assert_eq!(d.remove, ["oldest"]);
    assert!(!d.hold);
}

#[test]
fn over_budget_never_removes_an_active_tree_and_holds_dispatch() {
    let mut w = world();
    let done = w.run("done");
    let t1 = w.task(&done, "t1");
    let t2 = w.task(&done, "t2");
    // An active tree with a commit, a modified tracked file and an
    // untracked file. Its run is done: only its state keeps it.
    let path = w.repo.join(".worktrees").join("live");
    let path_s = path.display().to_string();
    git(
        &w.repo,
        &["worktree", "add", "-q", "-b", "fleet/live", &path_s, "main"],
    );
    std::fs::write(path.join("work.txt"), "committed\n").unwrap();
    git(&path, &["add", "work.txt"]);
    git(&path, &["commit", "-q", "-m", "work"]);
    let head = git(&path, &["rev-parse", "HEAD"]);
    std::fs::write(path.join("a.txt"), "dirty\n").unwrap();
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    w.row("live", Some(&t1), "active", "2026-01-01T00:00:00Z", 30);
    w.row("spare", Some(&t2), "landed", "2026-01-02T00:00:00Z", 5);

    let report = w.budget();
    assert_eq!(report.removals, ["spare"]);
    assert!(report.hold.is_some(), "{report:?}");
    let hold = worktree_disk(&w.conn, w.config.worktree_disk_budget_gb).unwrap();
    assert!(
        matches!(&hold, Some(Hold::WorktreeDisk { cause }) if cause.contains("35.0 GB")),
        "{hold:?}"
    );
    assert_eq!(w.disk_items().len(), 1);
    assert_eq!(
        w.count("SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove' AND subject = 'live'"),
        0
    );
    assert_eq!(w.state("live"), "active");
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(path.join("a.txt")).unwrap(),
        "dirty\n"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("new.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(
        git(&path, &["status", "--porcelain"]),
        "M a.txt\n?? new.txt"
    );
}

#[test]
fn over_budget_never_removes_a_tree_of_an_active_run() {
    let mut w = world();
    let active = w.run("active");
    let t1 = w.task(&active, "t1");
    let t2 = w.task(&active, "t2");
    w.row("landed", Some(&t1), "landed", "2026-01-01T00:00:00Z", 15);
    w.row("rescued", Some(&t2), "rescued", "2026-01-02T00:00:00Z", 15);
    let report = w.budget();
    assert!(report.removals.is_empty(), "{report:?}");
    assert!(report.hold.is_some());
    assert_eq!(w.state("landed"), "landed");
    assert_eq!(w.state("rescued"), "rescued");
}

#[test]
fn hold_opens_1_needs_you_item_and_resolves_under_the_budget() {
    let mut w = world();
    let active = w.run("active");
    let t1 = w.task(&active, "t1");
    // A rescued tree of an active run: no removal takes it (Q8).
    w.row("big", Some(&t1), "rescued", "2026-01-01T00:00:00Z", 25);
    let mut pass = w.pass(&Nothing, &NoLaunch);

    pass.run(&mut w.conn).unwrap();
    pass.run(&mut w.conn).unwrap();
    let items = w.disk_items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].0, "worktree_disk");
    assert!(
        items[0].1.contains("25.0 GB") && items[0].1.contains("20 GB"),
        "{}",
        items[0].1
    );
    let fact = r#"{"fact":"worktree_disk"}"#.to_string();
    assert_eq!(
        w.hold_events(),
        [("hold_started".to_string(), fact.clone())]
    );

    w.set_gb("big", 15);
    pass.run(&mut w.conn).unwrap();
    assert!(w.disk_items().is_empty());
    assert_eq!(
        w.count(
            "SELECT COUNT(*) FROM needs_you WHERE key = 'worktree_disk' AND state = 'resolved'"
        ),
        1
    );
    assert_eq!(
        w.hold_events(),
        [
            ("hold_started".to_string(), fact.clone()),
            ("hold_ended".to_string(), fact)
        ]
    );
}

/// The act (ticket 18 of fleetd-p3): the budget queues 1 removal for space,
/// and does not queue it again while it is queued.
#[test]
fn over_budget_queues_1_removal_for_space() {
    let mut w = world();
    let done = w.run("done");
    let t1 = w.task(&done, "t1");
    w.row("spare", Some(&t1), "landed", "2026-01-01T00:00:00Z", 25);
    let first = w.budget();
    let second = w.budget();
    assert_eq!(first.removals, ["spare"]);
    assert!(second.removals.is_empty(), "{second:?}");
    for report in [first, second] {
        assert!(report.hold.is_none(), "the spare tree is enough: no hold");
    }
    assert_eq!(w.removal_jobs(), 1);
    assert_eq!(
        w.count(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove'
              AND subject = 'spare' AND state = 'queued'"
        ),
        1
    );
    assert_eq!(w.state("spare"), "landed");
    assert!(w.disk_items().is_empty());
}

/// The hold comes from the rows that the reconciler of the same pass
/// wrote: a removal that the pass observes ends the excess before any
/// hold starts.
#[test]
fn a_removal_observed_in_the_pass_starts_no_hold() {
    let mut w = world();
    let active = w.run("active");
    let t1 = w.task(&active, "t1");
    w.row("big", Some(&t1), "landed", "2026-01-01T00:00:00Z", 25);
    w.removal_passed("big");
    let mut pass = w.pass(&Nothing, &NoLaunch);

    pass.run(&mut w.conn).unwrap();

    assert_eq!(w.state("big"), "removed");
    assert!(w.hold_events().is_empty(), "{:?}", w.hold_events());
    assert!(w.disk_items().is_empty());
}
