//! `horch ledger worktree new` and `scratch` write the `reserved` row and
//! its `worktree_add` job first, then run `git worktree add` with no
//! transaction open (docs/specs/fleetd/design.md, "Worktrees: leased,
//! reconciled, never lost"; FR-11; I6; rows W1, W2 and W4 of
//! preservation-04-behavior.md; docs/tickets/fleetd-p3/08-create-worktree-row-first.md).
//!
//! Each test makes its own temporary git repository and ledger. No test
//! starts a `horch` binary: a job runs in the test process (`TestJobs`),
//! or in a thread that plays `horch job run` beside a fake `horch` that
//! sleeps.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

#[path = "common/test_jobs.rs"]
mod test_jobs;
#[path = "common/warm.rs"]
mod warm;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::{self as custody_git, CliWorktreeGit};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeMode, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::create::{
    self, CreateOutcome, JobProcess, Lease, Observed,
};
use horch_core::fleetd::worktree_custody::application::ports::{
    ListEntry, TreeState, WorktreeGitGateway,
};
use horch_core::taskledger::finish;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{NewWorker, Worker};
use horch_core::taskledger::worktree_ops::{self, CreateKind};
use horch_core::taskledger::Store;
use rusqlite::{Connection, TransactionBehavior};
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    run: Run,
    git: Git,
    base: String,
}

/// Run git in `dir` for the test itself, with no hook and no config of
/// the user.
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

/// Write `file` in the tree `dir`, commit it, and give the new HEAD.
fn commit(dir: &Path, file: &str) -> String {
    std::fs::write(dir.join(file), "work\n").unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

impl World {
    /// A repository with 1 commit on `main` and the run branch, a ledger,
    /// and a run of the repository.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "base\n").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["branch", RUN_BRANCH]);
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
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
            tmp,
            root,
            store,
            run,
            git: Git::new("git"),
            base,
        }
    }

    fn db(&self) -> PathBuf {
        self.store.path().to_path_buf()
    }

    /// A new task, claimed by a new worker; returns the worker id.
    fn claimed(&mut self) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| {
                let t = NewTask::work("feature", "t.md", "opus");
                tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                worker(tx, &run)
            })
            .unwrap()
    }

    /// The task of `worker_id` goes back to the queue, and a new worker
    /// claims it; returns the new worker id.
    fn next_worker(&mut self, worker_id: &str) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| {
                tx.execute(
                    "UPDATE tasks SET status = 'ready', current_worker_id = NULL
                     WHERE current_worker_id = ?1",
                    [worker_id],
                )?;
                tx.execute(
                    "UPDATE workers SET status = 'stopped' WHERE worker_id = ?1",
                    [worker_id],
                )?;
                tx.execute(
                    "UPDATE attempts SET state = 'ended' WHERE worker_id = ?1",
                    [worker_id],
                )?;
                worker(tx, &run)
            })
            .unwrap()
    }

    /// The `live` attempt of `worker_id`.
    fn attempt(&self, worker_id: &str) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT id FROM attempts WHERE worker_id = ?1 AND state = 'live'",
                [worker_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn row(&self, id: &str) -> WorktreeRow {
        trees::get(self.store.conn(), id).unwrap().unwrap()
    }

    /// The count of open needs-you items of `kind`.
    fn open_items(&self, kind: &str) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT count(*) FROM needs_you WHERE kind = ?1 AND state = 'open'",
                [kind],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn rows(&self) -> i64 {
        self.store
            .conn()
            .query_row("SELECT count(*) FROM worktrees", [], |r| r.get(0))
            .unwrap()
    }

    /// The runs of `kind` of the subject `subject`, oldest first.
    fn jobs(&self, kind: Kind, subject: &str) -> Vec<JobRow> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([kind.as_str(), subject], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ids.into_iter()
            .map(|id| jobs_store::get(self.store.conn(), id).unwrap().unwrap())
            .collect()
    }

    /// A fake `horch` that sleeps: `<horch> job run <id>` stays alive
    /// while a thread of the test runs the executor.
    #[cfg(unix)]
    fn sleeping_horch(&self) -> PathBuf {
        let path = self.tmp.path().join("fake-horch-sleeps");
        warm::write_warmed(
            &path,
            "#!/bin/sh\n[ \"$1\" = job ] || exit 0\nexec sleep 10\n",
            &[],
        );
        path
    }

    /// A fake `horch` whose job process ends at once with no result.
    #[cfg(unix)]
    fn lost_horch(&self) -> PathBuf {
        let path = self.tmp.path().join("fake-horch-lost");
        warm::write_warmed(&path, "#!/bin/sh\nexit 0\n", &[]);
        path
    }
}

/// A started worker, bound to the first startable task of `opus`.
fn worker(tx: &rusqlite::Transaction<'_>, run: &Run) -> anyhow::Result<String> {
    let n: i64 = tx.query_row("SELECT count(*) FROM workers", [], |r| r.get(0))?;
    // The first startable task of the persona, as the dispatch orders them.
    let task = tasks::startable(tx, &run.run_id)?
        .into_iter()
        .find(|t| t.claimable_by.as_deref() == Some("opus"))
        .ok_or_else(|| anyhow::anyhow!("no startable task"))?
        .task_id;
    let new = NewWorker {
        run_id: run.run_id.clone(),
        persona: "opus".into(),
        role: Some(format!("opus-{}", n + 1)),
        ..Default::default()
    };
    let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
        pid: 4243,
        pid_start: Some(2),
    };
    let id = {
        // These tests make the tree with `horch ledger worktree new`, as the
        // worker of a claim did: the bind reserves no tree for a task of the
        // shared checkout, and the task takes its own mode again after it.
        let mode: String = tx.query_row(
            "SELECT worktree_mode FROM tasks WHERE task_id = ?1",
            [&task],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
            [&task],
        )?;
        let id =
            horch_core::taskledger::testing::bind_live(tx, &task, &new, Some(agent))?.worker_id;
        tx.execute(
            "UPDATE tasks SET worktree_mode = ?2 WHERE task_id = ?1",
            rusqlite::params![task, mode],
        )?;
        id
    };
    tx.execute(
        "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1 WHERE worker_id = ?1",
        [&id],
    )?;
    Ok(id)
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

/// The `running` `worktree_add` run, once the command started it.
fn running_add(conn: &Connection) -> JobRow {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let running = jobs_store::running(conn).unwrap();
        if let Some(job) = running.into_iter().find(|j| j.kind == Kind::WorktreeAdd) {
            return job;
        }
        assert!(Instant::now() < until, "the command started its job");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Write the result of the run `job` as `horch job run` writes it.
fn write_result(conn: &mut Connection, job: &JobRow) {
    let config = FleetConfig::default();
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &NoProfiles,
        sweep: None,
    };
    let written = jobs::run_job(&ctx(&config), conn, job.id, job.pid.unwrap(), &checks).unwrap();
    assert!(written.is_some(), "the job process wrote its result");
}

// ─── a gateway that records what it saw ─────────────────────────────────

/// What the gateway saw when `git worktree add` started.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    /// The committed state of the row, read on another connection.
    state: String,
    /// Another connection took the write lock: no transaction was open.
    no_transaction: bool,
    /// The folder existed.
    folder: bool,
}

/// The git gateway of custody, with a record of each `add`.
struct Recording {
    inner: CliWorktreeGit,
    db: PathBuf,
    worktree_id: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl WorktreeGitGateway for Recording {
    fn list(&self, repo: &Path) -> anyhow::Result<Vec<ListEntry>> {
        self.inner.list(repo)
    }

    fn tree(&self, path: &Path) -> anyhow::Result<Option<TreeState>> {
        self.inner.tree(path)
    }

    fn branch_exists(&self, repo: &Path, branch: &str) -> anyhow::Result<bool> {
        self.inner.branch_exists(repo, branch)
    }

    fn branch_tip(&self, repo: &Path, branch: &str) -> anyhow::Result<Option<String>> {
        self.inner.branch_tip(repo, branch)
    }

    fn prune(&self, repo: &Path) -> anyhow::Result<()> {
        self.inner.prune(repo)
    }

    fn add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> anyhow::Result<()> {
        let mut conn = Connection::open(&self.db).unwrap();
        conn.busy_timeout(Duration::from_secs(3)).unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM worktrees WHERE worktree_id = ?1",
                [&self.worktree_id],
                |r| r.get(0),
            )
            .unwrap();
        let no_transaction = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .is_ok();
        self.seen.lock().unwrap().push(Seen {
            state,
            no_transaction,
            folder: path.exists(),
        });
        self.inner.add(repo, path, branch, base)
    }

    fn attach(&self, repo: &Path, path: &Path, branch: &str) -> anyhow::Result<()> {
        self.inner.attach(repo, path, branch)
    }
}

// ─── the tests ──────────────────────────────────────────────────────────

/// FR-11 and I6: the row commits in `reserved` before `git worktree add`
/// runs, and the command holds no transaction while it runs.
#[cfg(unix)]
#[test]
fn worktree_row_is_reserved_before_the_folder_exists() {
    let mut w = World::new();
    let id = w.claimed();
    let db = w.db();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    let job_process = std::thread::spawn(move || {
        let mut conn = open(&db);
        let job = running_add(&conn);
        let tree = trees::get(&conn, &job.subject).unwrap().unwrap();
        let fleet = db.parent().unwrap().join("fleet");
        let gateway = Recording {
            inner: custody_git::tree_writer(CliWorktreeGit::new("git", fleet), &tree).unwrap(),
            db,
            worktree_id: job.subject.clone(),
            seen: recorded,
        };
        let ran = custody_git::worktree_add(&gateway, &tree);
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        // The executor of the job finds the tree made, at its base tip.
        write_result(&mut conn, &job);
    });
    let jobs = JobProcess::new(w.sleeping_horch());
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &jobs).unwrap();
    job_process.join().unwrap();

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        [Seen {
            state: "reserved".into(),
            no_transaction: true,
            folder: false,
        }]
    );
    let row = w.row(&wt.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&id)));
    assert_eq!(row.worker_id.as_deref(), Some(id.as_str()));
    assert!(Path::new(&wt.path).is_dir());
    assert!(wt.path.ends_with(".worktrees/T1-feat"), "{}", wt.path);
    assert_eq!(wt.branch, "fleet/r1-T1-feat");
    assert_eq!(wt.base, RUN_BRANCH);
    assert_eq!(wt.base_commit.as_deref(), Some(w.base.as_str()));
    assert_eq!(wt.status, "open");
}

/// Q17: the command gives no path before the job passed. The job process
/// is slow; the command waits for it, and the lease of the attempt starts
/// when the command returns.
#[cfg(unix)]
#[test]
fn attempt_waits_for_worktree_add_job_and_lease_starts_at_launch() {
    let mut w = World::new();
    let id = w.claimed();
    let db = w.db();
    let job_process = std::thread::spawn(move || {
        let mut conn = open(&db);
        let job = running_add(&conn);
        std::thread::sleep(Duration::from_secs(1));
        write_result(&mut conn, &job);
        Instant::now()
    });
    let jobs = JobProcess::new(w.sleeping_horch());
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &jobs).unwrap();
    let returned = Instant::now();
    let passed = job_process.join().unwrap();

    assert!(
        returned >= passed,
        "the command returned before the job passed"
    );
    let add = w.jobs(Kind::WorktreeAdd, &wt.worktree_id);
    assert_eq!(add.len(), 1);
    assert_eq!(add[0].state, State::Passed);
    assert!(Path::new(&wt.path).is_dir());
    let row = w.row(&wt.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&id)));
}

/// A failed run drops the `reserved` row, and the command prints the
/// reason of the job. An abandoned run keeps the row `reserved`, and the
/// next run of the rule of abandoned runs makes the tree.
#[cfg(unix)]
#[test]
fn failed_worktree_add_drops_the_reserved_row_and_abandoned_runs_again() {
    let mut w = World::new();
    let id = w.claimed();
    // Another tree holds the branch of the tree: the job refuses it.
    let holder = w.tmp.path().join("holder");
    let holder_text = holder.display().to_string();
    git(
        &w.root,
        &[
            "worktree",
            "add",
            "-b",
            "fleet/r1-T1-feat",
            &holder_text,
            "main",
        ],
    );
    let e = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap_err();
    assert!(
        e.to_string()
            .contains("the branch fleet/r1-T1-feat is checked out in"),
        "{e:#}"
    );
    assert_eq!(w.open_items("worktree_branch_refused"), 1);
    assert_eq!(w.rows(), 0, "the reserved row is dropped");
    assert!(!w.root.join(".worktrees/T1-feat").exists());
    let dropped: String = w
        .store
        .conn()
        .query_row(
            "SELECT cause FROM events WHERE entity = 'worktree' AND from_state = 'reserved'
               AND to_state IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dropped, "worktree_add_failed");

    // The job process ends with no result: the run ends abandoned.
    let lost = JobProcess::new(w.lost_horch());
    let e = finish::worktree_new_with(&mut w.store, &w.git, &id, "other", &lost).unwrap_err();
    assert!(e.to_string().contains("ended abandoned"), "{e:#}");
    let tree: String = w
        .store
        .conn()
        .query_row("SELECT worktree_id FROM worktrees", [], |r| r.get(0))
        .unwrap();
    assert_eq!(w.row(&tree).state, WorktreeState::Reserved);
    let runs = w.jobs(Kind::WorktreeAdd, &tree);
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0].state, State::Abandoned);
    assert_eq!(runs[1].state, State::Queued);
    assert_eq!(runs[1].run, 2);
    assert!(
        runs[1].not_before.is_some(),
        "the next run waits its backoff"
    );

    // The next command takes the reserved row and its next run.
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "other", &TestJobs).unwrap();
    assert_eq!(wt.worktree_id, tree);
    assert_eq!(w.row(&tree).state, WorktreeState::Active);
    assert_eq!(w.jobs(Kind::WorktreeAdd, &tree).len(), 2);
    assert!(Path::new(&wt.path).is_dir());
}

/// `idempotent: task-leased`: the same request 2 times has the effect of
/// 1. The second call finds the row and its run.
#[test]
fn create_worktree_twice_has_the_effect_of_once() {
    let mut w = World::new();
    let id = w.claimed();
    let task: String = w
        .store
        .conn()
        .query_row("SELECT task_id FROM tasks", [], |r| r.get(0))
        .unwrap();
    let attempt = w.attempt(&id);
    let new = NewWorktree {
        worktree_id: "wt-1".into(),
        task_id: Some(task.clone()),
        worker_id: Some(id.clone()),
        repo_root: w.root.display().to_string(),
        path: w.root.join(".worktrees/T1-feat").display().to_string(),
        branch: "fleet/r1-T1-feat".into(),
        base: RUN_BRANCH.into(),
        base_commit: Some(w.base.clone()),
        status: WorktreeStatus::Open,
        mode: Some(WorktreeMode {
            run_id: w.run.run_id.clone(),
            name: "feat".into(),
            scratch: false,
        }),
    };
    let lease = Lease {
        task_id: Some(&task),
        worker_id: &id,
        attempt: Some(&attempt),
        from: None,
        new: Some(&new),
    };
    let config = FleetConfig::default();
    let mut conn = open(&w.db());
    let mut call = || {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let out = create::create_worktree(&ctx(&config), &tx, &lease, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        out
    };
    let first = call();
    let second = call();
    let (CreateOutcome::Reserved { row: a, job: j1 }, CreateOutcome::Reserved { row: b, job: j2 }) =
        (first, second)
    else {
        panic!("both calls give the reserved row");
    };
    assert_eq!(a.worktree_id, "wt-1");
    assert_eq!(b.worktree_id, "wt-1");
    assert_eq!(j1, j2);
    assert_eq!(w.rows(), 1);
    assert_eq!(w.jobs(Kind::WorktreeAdd, "wt-1").len(), 1);

    // The command after them runs that 1 job.
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    assert_eq!(wt.worktree_id, "wt-1");
    assert_eq!(w.rows(), 1);
    assert_eq!(w.jobs(Kind::WorktreeAdd, "wt-1").len(), 1);
    // A command again takes the same tree with no new job.
    let again = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    assert_eq!(again.worktree_id, "wt-1");
    assert_eq!(w.jobs(Kind::WorktreeAdd, "wt-1").len(), 1);
}

/// `contract: worktree-created`: the event carries exactly `worktree_id`,
/// `task_id`, `path` and `branch`.
#[test]
fn published_worktree_created_carries_worktree_id_task_id_path_branch() {
    let mut w = World::new();
    let id = w.claimed();
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    let job = w.jobs(Kind::WorktreeAdd, &wt.worktree_id).remove(0);

    // A reserved row of the same task observes the same run again: build
    // the event from a fresh row.
    let config = FleetConfig::default();
    let mut conn = open(&w.db());
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "UPDATE worktrees SET state = 'reserved' WHERE worktree_id = ?1",
        [&wt.worktree_id],
    )
    .unwrap();
    let seen = create::observe(&ctx(&config), &tx, &job).unwrap();
    tx.commit().unwrap();
    let Observed::Created(event) = seen else {
        panic!("the passed run makes the row created: {seen:?}");
    };
    let json = serde_json::to_value(&event).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["branch", "path", "task_id", "worktree_id"]);
    assert_eq!(json["worktree_id"], wt.worktree_id.as_str());
    assert_eq!(json["task_id"], wt.task_id.as_deref().unwrap());
    assert_eq!(json["path"], wt.path.as_str());
    assert_eq!(json["branch"], wt.branch.as_str());
    assert_eq!(w.row(&wt.worktree_id).state, WorktreeState::Active);
}

/// Review F1: a passed run whose folder is gone before the observation
/// does not keep the row `reserved`. The row is dropped with the reason. The
/// branch of the first run stays, and the retry with the same task and name
/// attaches it with each of its commits.
#[test]
fn passed_worktree_add_with_no_folder_drops_the_reserved_row() {
    let mut w = World::new();
    let id = w.claimed();
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    let job = w.jobs(Kind::WorktreeAdd, &wt.worktree_id).remove(0);
    let work = commit(Path::new(&wt.path), "work.txt");
    std::fs::remove_dir_all(&wt.path).unwrap();

    let config = FleetConfig::default();
    let mut conn = open(&w.db());
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "UPDATE worktrees SET state = 'reserved' WHERE worktree_id = ?1",
        [&wt.worktree_id],
    )
    .unwrap();
    let seen = create::observe(&ctx(&config), &tx, &job).unwrap();
    tx.commit().unwrap();
    let Observed::Dropped(reason) = seen else {
        panic!("a passed run with no folder drops the row: {seen:?}");
    };
    assert!(reason.contains("does not exist"), "{reason}");
    assert_eq!(w.rows(), 0);

    // The retry with the same task and name attaches the kept branch.
    let next = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    assert_eq!(next.path, wt.path);
    assert_eq!(next.branch, wt.branch);
    assert_eq!(w.row(&next.worktree_id).state, WorktreeState::Active);
    assert_eq!(git(Path::new(&next.path), &["rev-parse", "HEAD"]), work);
    assert!(Path::new(&next.path).join("work.txt").is_file());
}

/// Row W2: the next attempt of the task gets the same tree. It moves to
/// the new owner, and its base does not change without a drop.
#[test]
fn retry_reuses_the_same_worktree() {
    let mut w = World::new();
    let first_worker = w.claimed();
    let first =
        finish::worktree_new_with(&mut w.store, &w.git, &first_worker, "feat", &TestJobs).unwrap();
    let next = w.next_worker(&first_worker);
    let again = finish::worktree_new_with(&mut w.store, &w.git, &next, "other", &TestJobs).unwrap();
    assert_eq!(again.worktree_id, first.worktree_id);
    assert_eq!(again.path, first.path);
    assert_eq!(again.worker_id.as_deref(), Some(next.as_str()));
    let row = w.row(&first.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&next)));
    assert_eq!(w.rows(), 1);
    assert_eq!(w.jobs(Kind::WorktreeAdd, &first.worktree_id).len(), 1);

    let e = worktree_ops::create_with(
        &mut w.store,
        &w.git,
        &next,
        "other",
        CreateKind::Task { from: Some("main") },
        &TestJobs,
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "existing worktree base is fleet/r1; drop it before changing base"
    );
    assert_eq!(w.row(&first.worktree_id).base, RUN_BRANCH);
}

/// Q10: an `orphaned` tree is not taken as it is. Its verified rescue
/// comes first, then the next attempt takes the `rescued` tree.
#[test]
fn orphaned_tree_is_rescued_before_the_next_attempt_takes_it() {
    let mut w = World::new();
    let first_worker = w.claimed();
    let wt =
        finish::worktree_new_with(&mut w.store, &w.git, &first_worker, "feat", &TestJobs).unwrap();
    std::fs::write(Path::new(&wt.path).join("u.txt"), "unsaved\n").unwrap();
    w.store
        .immediate(|tx| {
            let cause = Cause {
                actor: Actor::Horch,
                text: "its attempt crashed",
            };
            transition(
                tx,
                None,
                Entity::Worktree,
                &wt.worktree_id,
                "orphaned",
                cause,
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    let next = w.next_worker(&first_worker);
    let again = finish::worktree_new_with(&mut w.store, &w.git, &next, "feat", &TestJobs).unwrap();

    assert_eq!(again.worktree_id, wt.worktree_id);
    let row = w.row(&wt.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&next)));
    let rescue_ref = row.rescue_ref.expect("the rescue ref of the tree");
    assert!(
        rescue_ref.starts_with(&format!("refs/horch/rescue/{}/", wt.worktree_id)),
        "{rescue_ref}"
    );
    git(&w.root, &["show-ref", "--verify", &rescue_ref]);
    let rescues = w.jobs(Kind::Rescue, &wt.worktree_id);
    assert_eq!(rescues.len(), 1);
    assert_eq!(rescues[0].state, State::Passed);
    let moves: Vec<String> = {
        let mut stmt = w
            .store
            .conn()
            .prepare(
                "SELECT to_state FROM events WHERE entity = 'worktree' AND entity_id = ?1
                   AND to_state IS NOT NULL ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([&wt.worktree_id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        moves,
        ["reserved", "created", "active", "orphaned", "rescued", "active"]
    );
    assert!(Path::new(&wt.path).join("u.txt").is_file());
}

/// Row W4: a scratch tree has no task, and it goes through the
/// `worktree_add` job too.
#[test]
fn scratch_tree_has_no_task_and_goes_through_the_job() {
    let mut w = World::new();
    let id = w.claimed();
    let wt = worktree_ops::create_with(
        &mut w.store,
        &w.git,
        &id,
        "probe",
        CreateKind::Scratch { from: None },
        &TestJobs,
    )
    .unwrap();
    assert_eq!(wt.task_id, None);
    assert!(
        wt.path.ends_with(".worktrees/_scratch/probe"),
        "{}",
        wt.path
    );
    assert_eq!(wt.branch, "fleet-scratch-r1-probe");
    assert!(Path::new(&wt.path).is_dir());
    let row = w.row(&wt.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&id)));
    assert_eq!(
        row.mode,
        Some(WorktreeMode {
            run_id: w.run.run_id.clone(),
            name: "probe".into(),
            scratch: true,
        })
    );
    let add = w.jobs(Kind::WorktreeAdd, &wt.worktree_id);
    assert_eq!(add.len(), 1);
    assert_eq!(add[0].state, State::Passed);
}

/// The fleetd git rule: a hook of the project never runs with the rights
/// of fleetd. `git worktree add` starts `post-checkout`; the job runs, and
/// the hook does not.
#[cfg(unix)]
#[test]
fn post_checkout_hook_of_the_project_does_not_run_in_the_job() {
    let mut w = World::new();
    let id = w.claimed();
    let marker = w.tmp.path().join("hook-ran");
    let hook = w.root.join(".git/hooks/post-checkout");
    warm::write_warmed(
        &hook,
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        &[],
    );
    std::fs::remove_file(&marker).unwrap();
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    assert!(Path::new(&wt.path).is_dir());
    assert!(
        !marker.exists(),
        "the post-checkout hook of the project ran"
    );
}

/// A process that is not `horch` never starts a job process, and never
/// uses a `horch` from PATH.
#[test]
fn a_process_that_is_not_horch_starts_no_job() {
    let mut w = World::new();
    let id = w.claimed();
    let e = finish::worktree_new(&mut w.store, &w.git, &id, "feat").unwrap_err();
    assert!(e.to_string().contains("is not horch"), "{e:#}");
    assert_eq!(w.rows(), 0);
}

/// Review F1, round 3: a failed add keeps the branch of the task and
/// name. When no tree holds it any more, the retry with the same name
/// attaches it, with each commit on it. No branch is deleted.
#[test]
fn same_name_retry_after_a_failed_add_attaches_the_kept_branch_with_its_commits() {
    let mut w = World::new();
    let id = w.claimed();
    let holder = w.tmp.path().join("holder");
    let holder_text = holder.display().to_string();
    git(
        &w.root,
        &[
            "worktree",
            "add",
            "-b",
            "fleet/r1-T1-feat",
            &holder_text,
            "main",
        ],
    );
    let work = commit(&holder, "work.txt");
    let e = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap_err();
    assert!(e.to_string().contains("is checked out in"), "{e:#}");
    assert_eq!(w.rows(), 0);

    // The holder lets the branch go; its commit stays on the branch.
    git(&w.root, &["worktree", "remove", &holder_text]);
    let wt = finish::worktree_new_with(&mut w.store, &w.git, &id, "feat", &TestJobs).unwrap();
    assert_eq!(wt.branch, "fleet/r1-T1-feat");
    assert_eq!(w.row(&wt.worktree_id).state, WorktreeState::Active);
    assert_eq!(git(Path::new(&wt.path), &["rev-parse", "HEAD"]), work);
    assert!(Path::new(&wt.path).join("work.txt").is_file());
    assert_eq!(
        git(Path::new(&wt.path), &["branch", "--show-current"]),
        "fleet/r1-T1-feat"
    );
}
