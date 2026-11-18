//! The `worktree_add` and `worktree_list` jobs, the start and wait of a job
//! from a command, and 1 git writer for each repository
//! (docs/specs/fleetd/design.md, "Long work is a job. A pass never waits
//! for it", "Worktrees: leased, reconciled, never lost" and "1 git writer
//! for each repository"; docs/tickets/fleetd-p3/05-worktree-add-and-list-jobs.md).
//!
//! Each test makes its own temporary git repository and ledger. A job
//! process is a small command (`true`, `sleep`) started through the
//! launcher; the executor runs in the test process with the recorded pid,
//! as `horch job run` runs it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, CheckRunner, JobCtx, JobKey, JobRunner, Queued, SandboxProfiles,
    Stop,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use horch_core::fleetd::worktree_custody::adapters::git::{
    worktree_add_key, worktree_list_key, AddResult, ListResult,
};
use horch_core::fleetd::worktree_custody::adapters::run_now::{run_now, Starter};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow,
};
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use rusqlite::{Connection, TransactionBehavior};

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    /// The repository, as its canonical path.
    repo: PathBuf,
    /// The tip of `main`: the base tip of each row.
    base: String,
}

/// A temp ledger of the current schema, and a temp repository with 1
/// commit on `main`.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ledger.sqlite3");
    let mut conn = open(&db);
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    World {
        dir,
        conn,
        config: FleetConfig::default(),
        repo,
        base,
    }
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_out(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
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
        .output()
        .unwrap()
}

/// The context of a writer that is not fleetd.
fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

/// The sandbox profiles of a test: no check runs in these tests.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(
        &self,
        _worker: &Worker,
        _dir: &Path,
        _check: &str,
    ) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

/// The launcher, with a count of its launches.
struct Counting<'a>(&'a AtomicUsize);

impl ProcessHost for Counting<'_> {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Launcher.launch(spec)
    }
}

impl World {
    fn db(&self) -> PathBuf {
        self.dir.path().join("ledger.sqlite3")
    }

    /// The folder of the tree `name`: `<repo>/.worktrees/<name>`.
    fn tree_path(&self, name: &str) -> PathBuf {
        self.repo.join(".worktrees").join(name)
    }

    /// A `reserved` row `name` of the repository on `branch` at the base
    /// tip.
    fn reserve(&mut self, name: &str, branch: &str) -> WorktreeRow {
        let new = NewWorktree {
            worktree_id: name.to_string(),
            task_id: None,
            worker_id: None,
            repo_root: self.repo.display().to_string(),
            path: self.tree_path(name).display().to_string(),
            branch: branch.to_string(),
            base: "main".to_string(),
            base_commit: Some(self.base.clone()),
            status: WorktreeStatus::Open,
            mode: None,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let cause = Cause {
            actor: Actor::Horch,
            text: "test",
        };
        trees::insert_reserved(&tx, None, &new, cause).unwrap();
        tx.commit().unwrap();
        trees::get(&self.conn, name).unwrap().unwrap()
    }

    fn queue(&mut self, kind: Kind, subject: &str, input_key: &str) -> i64 {
        let key = JobKey {
            kind,
            subject,
            input_key,
        };
        let repo = self.repo.display().to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let q = jobs::queue(&ctx(&self.config), &tx, &key, Some(&repo), FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match q {
            Queued::New(id) => id,
            other => panic!("expected a new run, got {other:?}"),
        }
    }

    /// Queue the `worktree_add` job of `row`.
    fn queue_add(&mut self, row: &WorktreeRow) -> i64 {
        self.queue(Kind::WorktreeAdd, &row.worktree_id, &worktree_add_key(row))
    }

    /// Queue the `worktree_list` job of the repository at `clock`.
    fn queue_list(&mut self, clock: i64) -> i64 {
        let key = worktree_list_key(FleetSeconds(clock), self.config.worktree_list_period_s);
        let repo = self.repo.display().to_string();
        self.queue(Kind::WorktreeList, &repo, &key)
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// The run `id` to its end: its process starts as `true` through the
    /// launcher, as part 3 of a pass starts it, and its executor runs in
    /// this process with the recorded pid.
    fn run(&mut self, id: i64) -> JobRow {
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let job = self.job(id);
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let mut runner = JobRunner::new(&Launcher);
        assert!(runner
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap());
        let pid = self.job(id).pid.unwrap();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        self.job(id)
    }

    /// A fake `horch` that sleeps: `<horch> job run <id>` stays alive while
    /// the executor runs in [`job_process`].
    fn sleeping_horch(&self) -> PathBuf {
        let path = self.dir.path().join("fake-horch");
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        make_executable(&path);
        path
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The job process of the run `id`, as `horch job run <id>` runs it: it
/// waits until the run is `running`, then runs the executor with the
/// recorded pid, on its own connection.
fn job_process(db: &Path, config: &FleetConfig, id: i64) -> Option<State> {
    let mut conn = open(db);
    let until = Instant::now() + Duration::from_secs(20);
    let pid = loop {
        let row = jobs_store::get(&conn, id).unwrap().unwrap();
        if row.state == State::Running {
            break row.pid.unwrap();
        }
        assert!(Instant::now() < until, "job {id} started");
        std::thread::sleep(Duration::from_millis(20));
    };
    let ctx = JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    };
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &NoProfiles,
    };
    jobs::run_job(&ctx, &mut conn, id, pid, &checks)
        .unwrap()
        .map(|o| o.state())
}

/// Kill the process group of the run `row`.
fn kill(row: &JobRow) {
    if let Some(pid) = row.pid {
        jobs::stop(&Stop {
            job_id: row.id,
            pid,
            pid_start: row.pid_start,
            kill: true,
        });
    }
}

fn add_result(row: &JobRow) -> AddResult {
    serde_json::from_value(result_data(row).expect("a result with data")).unwrap()
}

fn list_result(row: &JobRow) -> ListResult {
    serde_json::from_value(result_data(row).expect("a result with data")).unwrap()
}

fn text(row: &JobRow) -> String {
    outcome(row).map(|o| o.text).unwrap_or_default()
}

// ─── worktree_add ───────────────────────────────────────────────────────

#[test]
fn worktree_add_job_makes_the_tree_on_its_branch_at_the_base_tip() {
    let mut w = world();
    let row = w.reserve("t1", "fleet/t1");
    let id = w.queue_add(&row);

    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let path = w.tree_path("t1");
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), w.base);
    assert_eq!(git(&path, &["branch", "--show-current"]), "fleet/t1");
    let result = add_result(&ended);
    assert_eq!(result.path, row.path);
    assert_eq!(result.branch, "fleet/t1");
    assert_eq!(result.head, w.base);
    // The row stays `reserved`: the pass or the command moves it.
    let after = trees::get(&w.conn, "t1").unwrap().unwrap();
    assert_eq!(after.state.as_str(), "reserved");
}

#[test]
fn worktree_add_job_prunes_an_empty_partial_folder_and_runs_again() {
    let mut w = world();
    // An empty folder that an interrupted add left.
    let row = w.reserve("t1", "fleet/t1");
    std::fs::create_dir_all(w.tree_path("t1")).unwrap();
    let id = w.queue_add(&row);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(git(&w.tree_path("t1"), &["rev-parse", "HEAD"]), w.base);

    // An empty folder with a stale entry of git: the folder of a detached
    // tree whose files are gone.
    let row = w.reserve("t2", "fleet/t2");
    let path = w.tree_path("t2");
    git(
        &w.repo,
        &["worktree", "add", "-q", "--detach", &row.path, &w.base],
    );
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(git(&w.repo, &["worktree", "list", "--porcelain"]).contains("prunable"));
    let id = w.queue_add(&row);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(git(&path, &["branch", "--show-current"]), "fleet/t2");
    assert!(!git(&w.repo, &["worktree", "list", "--porcelain"]).contains("prunable"));
}

#[test]
fn worktree_add_job_keeps_a_folder_with_work_and_fails() {
    let mut w = world();

    // A tree with a modified file.
    let row = w.reserve("modified", "fleet/modified");
    let first = w.queue_add(&row);
    assert_eq!(w.run(first).state, State::Passed);
    let modified = w.tree_path("modified").join("a.txt");
    std::fs::write(&modified, "changed\n").unwrap();

    // A tree with an untracked file.
    let row2 = w.reserve("untracked", "fleet/untracked");
    let first = w.queue_add(&row2);
    assert_eq!(w.run(first).state, State::Passed);
    let untracked = w.tree_path("untracked").join("new.txt");
    std::fs::write(&untracked, "new\n").unwrap();

    // A tree with a commit that is not on the base.
    let row3 = w.reserve("committed", "fleet/committed");
    let first = w.queue_add(&row3);
    assert_eq!(w.run(first).state, State::Passed);
    let tree3 = w.tree_path("committed");
    std::fs::write(tree3.join("b.txt"), "b\n").unwrap();
    git(&tree3, &["add", "b.txt"]);
    git(&tree3, &["commit", "-q", "-m", "work"]);
    let commit = git(&tree3, &["rev-parse", "HEAD"]);

    for row in [&row, &row2, &row3] {
        let id = w.queue_add(row);
        let ended = w.run(id);
        assert_eq!(ended.state, State::Failed, "{}", row.worktree_id);
        assert!(
            text(&ended).contains("the folder holds files or commits"),
            "{}",
            text(&ended)
        );
    }

    assert_eq!(std::fs::read_to_string(&modified).unwrap(), "changed\n");
    assert_eq!(std::fs::read_to_string(&untracked).unwrap(), "new\n");
    assert_eq!(git(&tree3, &["rev-parse", "HEAD"]), commit);
    assert_eq!(git(&w.repo, &["rev-parse", "fleet/committed"]), commit);
    assert!(tree3.join("b.txt").exists());
}

/// A branch `branch` with 1 commit on `main`, and `main` checked out
/// again. Its tip.
fn kept_branch(w: &World, branch: &str) -> String {
    std::fs::write(w.repo.join("c.txt"), "c\n").unwrap();
    git(&w.repo, &["checkout", "-q", "-b", branch]);
    git(&w.repo, &["add", "c.txt"]);
    git(&w.repo, &["commit", "-q", "-m", "kept work"]);
    let tip = git(&w.repo, &["rev-parse", "HEAD"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    tip
}

/// Observe the ended run `job` as the command does, and give the number
/// of open needs-you items of a refused branch.
fn observe_refused(w: &mut World, job: &JobRow) -> i64 {
    let config = w.config.clone();
    let tx = w
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let seen = create::observe(&ctx(&config), &tx, job).unwrap();
    tx.commit().unwrap();
    assert!(matches!(seen, Observed::Dropped(_)), "{seen:?}");
    w.conn
        .query_row(
            "SELECT count(*) FROM needs_you WHERE kind = ?1 AND state = 'open'",
            [create::BRANCH_REFUSED_KIND],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn worktree_add_job_attaches_a_free_branch_of_its_task_and_name_and_keeps_its_commit() {
    let mut w = world();
    let tip = kept_branch(&w, "fleet-T1-t1");

    let row = w.reserve("T1-t1", "fleet-T1-t1");
    let id = w.queue_add(&row);
    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let tree = w.tree_path("T1-t1");
    assert_eq!(
        add_result(&ended).head,
        tip,
        "the tree is at the kept commit"
    );
    assert_eq!(
        git(&tree, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "fleet-T1-t1"
    );
    assert!(tree.join("c.txt").exists());
    assert_eq!(git(&w.repo, &["rev-parse", "fleet-T1-t1"]), tip);
}

#[test]
fn worktree_add_job_refuses_a_branch_it_cannot_prove_safe_and_keeps_it() {
    let mut w = world();
    // Another worktree holds the branch of this task and name.
    let tip = kept_branch(&w, "fleet-T1-t1");
    let holder = w.tree_path("holder").display().to_string();
    git(&w.repo, &["worktree", "add", "-q", &holder, "fleet-T1-t1"]);
    let row = w.reserve("T1-t1", "fleet-T1-t1");
    let id = w.queue_add(&row);
    let ended = w.run(id);

    assert_eq!(ended.state, State::Failed);
    assert!(text(&ended).contains("fleet-T1-t1"), "{}", text(&ended));
    assert!(text(&ended).contains(&holder), "{}", text(&ended));
    assert_eq!(git(&w.repo, &["rev-parse", "fleet-T1-t1"]), tip);
    assert!(!w.tree_path("T1-t1").exists());
    assert_eq!(observe_refused(&mut w, &ended), 1);

    // A branch that is not the branch of this task and name.
    git(&w.repo, &["branch", "fleet/taken", "fleet-T1-t1"]);
    let row = w.reserve("T2-t2", "fleet/taken");
    let id = w.queue_add(&row);
    let ended = w.run(id);

    assert_eq!(ended.state, State::Failed);
    assert!(text(&ended).contains("fleet/taken"), "{}", text(&ended));
    assert!(text(&ended).contains("not the branch"), "{}", text(&ended));
    assert_eq!(git(&w.repo, &["rev-parse", "fleet/taken"]), tip);
    assert!(!w.tree_path("T2-t2").exists());
    assert_eq!(observe_refused(&mut w, &ended), 2);
}

// ─── worktree_list ──────────────────────────────────────────────────────

#[test]
fn worktree_list_job_reports_each_entry_with_prunable_and_locked() {
    let mut w = world();
    let locked = w.tree_path("locked");
    let gone = w.tree_path("gone");
    let locked_s = locked.display().to_string();
    let gone_s = gone.display().to_string();
    git(
        &w.repo,
        &["worktree", "add", "-q", "-b", "l", &locked_s, "main"],
    );
    git(
        &w.repo,
        &["worktree", "lock", "--reason", "kept", &locked_s],
    );
    git(
        &w.repo,
        &["worktree", "add", "-q", "--detach", &gone_s, "main"],
    );
    std::fs::remove_dir_all(&gone).unwrap();
    let before = git(&w.repo, &["worktree", "list", "--porcelain"]);

    let id = w.queue_list(125);
    assert_eq!(w.job(id).input_key, "2", "the number of the period");
    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let result = list_result(&ended);
    assert_eq!(result.entries.len(), 3);
    let main = &result.entries[0];
    assert_eq!(main.path, w.repo.display().to_string());
    assert_eq!(main.branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(main.head.as_deref(), Some(w.base.as_str()));
    let l = result.entries.iter().find(|e| e.path == locked_s).unwrap();
    assert_eq!(l.locked.as_deref(), Some("kept"));
    assert_eq!(l.prunable, None);
    let g = result.entries.iter().find(|e| e.path == gone_s).unwrap();
    assert!(g.detached);
    assert!(g.prunable.is_some());
    assert_eq!(g.locked, None);
    // It changes nothing.
    assert_eq!(git(&w.repo, &["worktree", "list", "--porcelain"]), before);
}

#[test]
fn worktree_list_job_reports_head_dirty_and_size_of_each_row_tree() {
    let mut w = world();
    let clean = w.reserve("clean", "fleet/clean");
    let id = w.queue_add(&clean);
    assert_eq!(w.run(id).state, State::Passed);
    let dirty = w.reserve("dirty", "fleet/dirty");
    let id = w.queue_add(&dirty);
    assert_eq!(w.run(id).state, State::Passed);
    std::fs::write(w.tree_path("dirty").join("big.bin"), vec![7u8; 20_000]).unwrap();
    // A row whose folder is gone: git gives no fact of it.
    let missing = w.reserve("missing", "fleet/missing");

    let id = w.queue_list(0);
    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let result = list_result(&ended);
    let of = |id: &str| {
        result
            .trees
            .iter()
            .find(|t| t.worktree_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("the facts of {id}"))
    };
    let c = of("clean");
    assert_eq!(c.path, clean.path);
    assert_eq!(c.head.as_deref(), Some(w.base.as_str()));
    assert_eq!(c.dirty, Some(false));
    assert!(c.bytes.unwrap() > 0);
    let d = of("dirty");
    assert_eq!(d.head.as_deref(), Some(w.base.as_str()));
    assert_eq!(d.dirty, Some(true));
    assert!(d.bytes.unwrap() >= 20_000);
    let m = of("missing");
    assert_eq!(m.path, missing.path);
    assert_eq!((m.head.as_deref(), m.dirty, m.bytes), (None, None, None));
    // The facts of the main tree too, with no size.
    let main = result
        .trees
        .iter()
        .find(|t| t.path == w.repo.display().to_string())
        .unwrap();
    assert_eq!(main.head.as_deref(), Some(w.base.as_str()));
    assert_eq!(main.worktree_id, None);
}

// ─── run_now and 1 git writer for each repository ───────────────────────

#[test]
fn run_now_starts_a_queued_job_once_when_the_pass_starts_it_too() {
    let mut w = world();
    let row = w.reserve("t1", "fleet/t1");
    let id = w.queue_add(&row);
    let horch = w.sleeping_horch();
    let launches = AtomicUsize::new(0);
    let db = w.db();
    let config = w.config.clone();

    // The pass starts the run first.
    let pass_host = Counting(&launches);
    let mut pass = JobRunner::new(&pass_host);
    let spec = jobs::job_spec(&horch, id, None);
    let job = w.job(id);
    assert!(pass
        .start(&ctx(&w.config), &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());

    let ended = std::thread::scope(|s| {
        let process = s.spawn(|| job_process(&db, &config, id));
        let mut conn = open(&db);
        let host = Counting(&launches);
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let starter = Starter {
            ctx: &ctx,
            host: &host,
            horch: &horch,
            output: None,
        };
        let ended = run_now(&mut conn, id, &starter).unwrap();
        assert_eq!(process.join().unwrap(), Some(State::Passed));
        ended
    });

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(launches.load(Ordering::SeqCst), 1, "1 process for the run");
    assert_eq!(git(&w.tree_path("t1"), &["rev-parse", "HEAD"]), w.base);
    // The pass cannot start the ended run again.
    let job = w.job(id);
    let late = pass
        .start(&ctx(&w.config), &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap();
    assert!(!late);
    kill(&ended);
}

#[test]
fn run_now_ends_a_job_whose_process_is_gone_as_abandoned() {
    let mut w = world();
    let row = w.reserve("t1", "fleet/t1");
    let id = w.queue_add(&row);
    let launches = AtomicUsize::new(0);
    let host = Counting(&launches);
    let ctx = ctx(&w.config);
    // The job process ends at once and writes no result.
    let starter = Starter {
        ctx: &ctx,
        host: &host,
        horch: Path::new("true"),
        output: None,
    };
    let mut conn = open(&w.db());

    let ended = run_now(&mut conn, id, &starter).unwrap();

    assert_eq!(ended.id, id);
    assert_eq!(ended.state, State::Abandoned);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert!(!w.tree_path("t1").exists());
}

#[test]
fn second_git_writing_job_of_1_repo_waits_for_the_first() {
    let mut w = world();
    let first_row = w.reserve("t1", "fleet/t1");
    let second_row = w.reserve("t2", "fleet/t2");
    let first = w.queue_add(&first_row);
    let second = w.queue_add(&second_row);
    let horch = w.sleeping_horch();
    let repo = w.repo.display().to_string();
    let db = w.db();
    let config = w.config.clone();
    assert!(!jobs::git_writer_busy(&w.conn, &repo).unwrap());

    // The pass starts the first run.
    let mut pass = JobRunner::new(&Launcher);
    let job = w.job(first);
    let spec = jobs::job_spec(&horch, first, None);
    assert!(pass
        .start(&ctx(&w.config), &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());
    assert!(jobs::git_writer_busy(&w.conn, &repo).unwrap());
    // A `worktree_list` run is no git writer.
    let other = w.dir.path().join("other").display().to_string();
    assert!(!jobs::git_writer_busy(&w.conn, &other).unwrap());

    let launches = AtomicUsize::new(0);
    let ended = std::thread::scope(|s| {
        let command = s.spawn(|| {
            let mut conn = open(&db);
            let host = Counting(&launches);
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let starter = Starter {
                ctx: &ctx,
                host: &host,
                horch: &horch,
                output: None,
            };
            run_now(&mut conn, second, &starter).unwrap()
        });
        // The command waits: the first run holds the repository.
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(launches.load(Ordering::SeqCst), 0);
        assert_eq!(w.job(second).state, State::Queued);

        // The first run ends; then the command starts the second.
        assert_eq!(job_process(&db, &config, first), Some(State::Passed));
        assert_eq!(job_process(&db, &config, second), Some(State::Passed));
        command.join().unwrap()
    });

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert!(!jobs::git_writer_busy(&w.conn, &repo).unwrap());
    assert_eq!(git(&w.tree_path("t2"), &["rev-parse", "HEAD"]), w.base);
    kill(&w.job(first));
    kill(&ended);
}

#[test]
fn rescue_job_has_the_time_limit_of_job_limit_s_rescue() {
    let mut w = world();
    w.config.job_limit_s.rescue = 777;
    assert_eq!(jobs::time_limit_s(Kind::Rescue, &w.config), Some(777));

    let id = w.queue(Kind::Rescue, "t1", "path head");
    assert_eq!(w.job(id).time_limit_s, 777);
}

// ─── the fleetd git rule ────────────────────────────────────────────────

#[test]
fn worktree_jobs_run_no_hook_or_program_of_the_project() {
    let mut w = world();
    let hook_marker = w.dir.path().join("hook-ran");
    let monitor_marker = w.dir.path().join("fsmonitor-ran");
    let hooks = w.repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in ["post-checkout", "reference-transaction"] {
        let path = hooks.join(hook);
        let script = format!("#!/bin/sh\necho {hook} >> '{}'\n", hook_marker.display());
        std::fs::write(&path, script).unwrap();
        make_executable(&path);
    }
    let monitor = w.dir.path().join("monitor.sh");
    let script = format!("#!/bin/sh\necho ran > '{}'\n", monitor_marker.display());
    std::fs::write(&monitor, script).unwrap();
    make_executable(&monitor);
    let monitor_s = monitor.display().to_string();
    git(&w.repo, &["config", "core.fsmonitor", &monitor_s]);

    // A key of the repository that starts a program stops each job.
    let row = w.reserve("t1", "fleet/t1");
    let id = w.queue_add(&row);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed);
    assert!(text(&ended).contains("core.fsmonitor"), "{}", text(&ended));
    assert!(!w.tree_path("t1").exists(), "it changes nothing");
    let id = w.queue_list(0);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed);
    assert!(text(&ended).contains("core.fsmonitor"), "{}", text(&ended));
    assert!(!monitor_marker.exists(), "no fsmonitor ran");

    // With the key removed, the jobs pass, and no hook runs.
    git(&w.repo, &["config", "--unset", "core.fsmonitor"]);
    let id = w.queue_add(&row);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let id = w.queue_list(60);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert!(!hook_marker.exists(), "no hook ran");
    assert!(!monitor_marker.exists(), "no fsmonitor ran");
}

#[test]
fn each_gateway_call_refuses_a_project_program_before_its_git_command() {
    use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
    use horch_core::fleetd::worktree_custody::application::ports::WorktreeGitGateway;

    let w = world();
    let gw = CliWorktreeGit::new("git");
    let tree = w.tree_path("t1");
    assert!(gw.branch_exists(&w.repo, "main").unwrap());
    git(&w.repo, &["config", "core.sshCommand", "/bin/false"]);

    let refused = |r: anyhow::Result<()>| {
        let e = format!("{:#}", r.expect_err("a refusal"));
        assert!(e.contains("core.sshcommand"), "{e}");
    };
    refused(gw.branch_exists(&w.repo, "main").map(drop));
    refused(gw.list(&w.repo).map(drop));
    refused(gw.tree(&w.repo).map(drop));
    refused(gw.prune(&w.repo));
    refused(gw.add(&w.repo, &tree, "fleet/t1", &w.base));
    assert!(!tree.exists(), "the refused add changes nothing");
    assert!(!git(&w.repo, &["branch", "--list", "fleet/t1"]).contains("fleet/t1"));
}
