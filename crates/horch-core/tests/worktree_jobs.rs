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
    world_in(tempfile::tempdir().unwrap())
}

/// [`world`] in a temp folder below HOME, where the repository of a user
/// is. The folder goes at the end of the test.
fn world_below_home() -> World {
    let home = std::env::var_os("HOME").expect("HOME is set");
    world_in(tempfile::tempdir_in(home).unwrap())
}

fn world_in(dir: tempfile::TempDir) -> World {
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
            sweep: None,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
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
        sweep: None,
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

/// The sandbox hides HOME from git, but git can read the metadata of each
/// folder above the repository, so it can make the leading folders of the
/// tree.
#[test]
fn worktree_add_job_makes_the_tree_of_a_repository_below_home() {
    let mut w = world_below_home();
    let row = w.reserve("t1", "fleet/t1");
    let id = w.queue_add(&row);

    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let path = w.tree_path("t1");
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), w.base);
    assert_eq!(git(&path, &["branch", "--show-current"]), "fleet/t1");
}

#[test]
fn worktree_list_job_reports_the_trees_of_a_repository_below_home() {
    let mut w = world_below_home();
    let row = w.reserve("t1", "fleet/t1");
    let tree = w.tree_path("t1").display().to_string();
    git(
        &w.repo,
        &["worktree", "add", "-q", "-b", "fleet/t1", &tree, "main"],
    );
    let id = w.queue_list(125);

    let ended = w.run(id);

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let result = list_result(&ended);
    assert_eq!(result.entries.len(), 2, "{result:?}");
    let facts = result.trees.iter().find(|t| t.path == tree).unwrap();
    assert_eq!(facts.worktree_id.as_deref(), Some(row.worktree_id.as_str()));
    assert_eq!(facts.head.as_deref(), Some(w.base.as_str()));
    assert_eq!(facts.dirty, Some(false));
    assert_eq!(facts.error, None);
}

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

#[cfg(unix)]
fn sleeping(seconds: &str) -> LaunchSpec {
    LaunchSpec {
        program: "sleep".into(),
        args: vec![seconds.into()],
        cwd: None,
        env_rule: Default::default(),
        detach: true,
        output: None,
    }
}

#[cfg(unix)]
fn start_sleeping(
    w: &mut World,
    runner: &mut JobRunner<'_>,
    id: i64,
    seconds: &str,
    clock: i64,
) -> bool {
    let job = w.job(id);
    let config = w.config.clone();
    runner
        .start(
            &ctx(&config),
            &mut w.conn,
            &job,
            &sleeping(seconds),
            FleetSeconds(clock),
        )
        .unwrap()
}

#[cfg(unix)]
fn observe_run(
    w: &mut World,
    id: i64,
    liveness: horch_core::fleetd::fleet_runtime::adapters::scans::Liveness,
    clock: i64,
) -> jobs::Observed {
    use horch_core::fleetd::fleet_runtime::adapters::scans::ProcessFact;

    let config = w.config.clone();
    let facts = [ProcessFact {
        key: id.to_string(),
        liveness,
    }];
    let tx = w
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let seen = jobs::observe(&ctx(&config), &tx, &facts, FleetSeconds(clock)).unwrap();
    tx.commit().unwrap();
    seen
}

#[cfg(unix)]
fn wait_gone(runner: &mut JobRunner<'_>, pid: u32) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        runner.reap();
        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .unwrap()
            .status
            .success();
        if !alive {
            return;
        }
        assert!(Instant::now() < until, "the process {pid} is gone");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn rescue_and_observe(w: &mut World, row: &WorktreeRow, rescue: i64) -> String {
    use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
    use horch_core::fleetd::worktree_custody::application::rescue::{
        self as rescue_app, Observed as RescueObserved,
    };

    let ran = rescue_git::rescue(&w.fleet_git(), &w.fleet_root(), row);
    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    let result = JobResult {
        job_id: rescue,
        run: w.job(rescue).run,
        passed: true,
        verdict: true,
        text: ran.outcome.text,
        data: ran.data,
    };
    job_result_file::write(&w.fleet_root(), &result).unwrap();
    let config = w.config.clone();
    let done = jobs::end_from_files(&ctx(&config), &mut w.conn).unwrap();
    assert_eq!(done, vec![(rescue, State::Passed)]);
    let job = w.job(rescue);
    let tx = w
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let observed = rescue_app::observe(&ctx(&config), &tx, &job).unwrap();
    tx.commit().unwrap();
    match observed {
        RescueObserved::Rescued(rescue_ref) => rescue_ref,
        other => panic!("the row is not rescued: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue() {
    use horch_core::fleetd::fleet_runtime::adapters::scans::Liveness;
    use horch_core::fleetd::worktree_custody::adapters::sqlite::WorktreeState;

    let mut w = world();
    w.config.stop_grace_s = 1;
    let row = w.made("t1");
    w.conn
        .execute(
            "UPDATE worktrees SET state = 'orphaned' WHERE worktree_id = ?1",
            [&row.worktree_id],
        )
        .unwrap();
    let repo = w.repo.display().to_string();
    let fetch = w.queue(Kind::Fetch, &repo, "fetch origin");
    w.conn
        .execute("UPDATE jobs SET time_limit_s = 5 WHERE id = ?1", [fetch])
        .unwrap();
    let rescue_key = rescue_git::rescue_key(&row.path, &w.base);
    let rescue = w.queue(Kind::Rescue, &row.worktree_id, &rescue_key);
    let mut runner = JobRunner::new(&Launcher);

    assert!(start_sleeping(&mut w, &mut runner, fetch, "60", 0));
    let hung = w.job(fetch);
    assert_eq!(hung.state, State::Running);
    assert_eq!(hung.deadline, Some(FleetSeconds(5)));
    assert!(jobs::git_writer_busy(&w.conn, &repo).unwrap());
    assert!(
        !start_sleeping(&mut w, &mut runner, rescue, "0", 1),
        "the rescue waits for the fetch"
    );
    assert_eq!(w.job(rescue).state, State::Queued);

    let before = observe_run(&mut w, fetch, Liveness::Alive, 4);
    assert!(before.ended.is_empty() && before.stops.is_empty());
    let seen = observe_run(&mut w, fetch, Liveness::Alive, 6);
    assert!(seen.ended.is_empty(), "the process still lives: {seen:?}");
    assert_eq!(seen.stops.len(), 1, "{seen:?}");
    assert!(seen.stops[0].kill, "{seen:?}");
    assert_eq!(w.job(fetch).state, State::Running);
    assert!(jobs::git_writer_busy(&w.conn, &repo).unwrap());
    assert!(jobs::stop(&seen.stops[0]), "the stop reached the process");
    wait_gone(&mut runner, hung.pid.unwrap());

    let ended = observe_run(&mut w, fetch, Liveness::Dead, 7);
    assert_eq!(ended.ended, vec![(fetch, State::Abandoned)]);
    assert_eq!(w.job(fetch).state, State::Abandoned);
    assert!(!jobs::git_writer_busy(&w.conn, &repo).unwrap());

    assert!(start_sleeping(&mut w, &mut runner, rescue, "0", 7));
    assert_eq!(w.job(rescue).state, State::Running);
    let rescue_ref = rescue_and_observe(&mut w, &row, rescue);
    assert!(
        rescue_ref.starts_with("refs/horch/rescue/t1/"),
        "{rescue_ref}"
    );
    let saved = trees::get(&w.conn, "t1").unwrap().unwrap();
    assert_eq!(saved.state, WorktreeState::Rescued);
    assert_eq!(saved.rescue_ref.as_deref(), Some(rescue_ref.as_str()));
    let commit = git(&w.repo, &["rev-parse", &rescue_ref]);
    assert_eq!(git(&w.repo, &["rev-parse", &format!("{commit}^")]), w.base);
    assert_eq!(
        git(&w.repo, &["show", &format!("{commit}:new.txt")]),
        "new",
        "the untracked file is saved"
    );
    assert!(w.tree_path("t1").join("new.txt").is_file());
    runner.reap();
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
    let gw = CliWorktreeGit::new("git", w.dir.path().join("fleet"));
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

// ─── the race attacks of T-691 ──────────────────────────────────────────
//
// A live process of the project writes a program setting into the config
// of the repository AFTER the scan of fleetd and BEFORE the git command
// (the race seam of `CliWorktreeGit`). The program writes a marker in a
// folder that git of a job cannot write. The marker must not exist.

use horch_core::fleetd::worktree_custody::adapters::executors;
use horch_core::fleetd::worktree_custody::adapters::git::{
    self as custody_git, CliWorktreeGit, NO_SANDBOX,
};
use horch_core::fleetd::worktree_custody::adapters::rescue_git;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

impl World {
    /// The fleet root of this world: the folder `fleet` beside the ledger.
    fn fleet_root(&self) -> PathBuf {
        self.dir.path().join("fleet")
    }

    /// The fleetd adapter of a job of this world. It can write no path.
    fn fleet_git(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.fleet_root())
    }

    /// A program `name` that writes its marker, then runs `rest`. Its
    /// marker is in a folder outside the repository and the fleet root.
    fn attack(&self, name: &str, rest: &str) -> (PathBuf, PathBuf) {
        let folder = self.dir.path().join("attack");
        std::fs::create_dir_all(&folder).unwrap();
        let marker = folder.join(format!("{name}.ran"));
        let program = folder.join(format!("{name}.sh"));
        let script = format!("#!/bin/sh\necho ran > '{}'\n{rest}\n", marker.display());
        std::fs::write(&program, script).unwrap();
        make_executable(&program);
        (program, marker)
    }

    /// A tree `name` made by the test on the branch `fleet/<name>`, with a
    /// modified file and an untracked file, and its row.
    fn made(&mut self, name: &str) -> WorktreeRow {
        let row = self.reserve(name, &format!("fleet/{name}"));
        let path = self.tree_path(name);
        let path_s = path.display().to_string();
        git(
            &self.repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &row.branch,
                &path_s,
                &self.base,
            ],
        );
        std::fs::write(path.join("a.txt"), "changed\n").unwrap();
        std::fs::write(path.join("new.txt"), "new\n").unwrap();
        row
    }
}

/// A race seam that runs `attack` once, just before the first git call
/// whose arguments hold `trigger`.
fn once(
    trigger: &'static str,
    attack: impl Fn() + Send + Sync + 'static,
) -> impl Fn(&Path, &[&str]) + Send + Sync + 'static {
    let done = Arc::new(AtomicBool::new(false));
    move |_, args| {
        if args.contains(&trigger) && !done.swap(true, Ordering::SeqCst) {
            attack();
        }
    }
}

/// The attacker: write `key = value` into the config of the repository.
fn set_config(repo: &Path, key: &str, value: &str) {
    git(repo, &["config", key, value]);
}

/// The attacker: select a driver for each path in `$GIT_DIR/info/attributes`,
/// which git reads whatever its options.
fn select_driver(repo: &Path, attribute: &str) {
    let info = repo.join(".git").join("info");
    std::fs::create_dir_all(&info).unwrap();
    std::fs::write(info.join("attributes"), format!("* {attribute}\n")).unwrap();
}

/// The program did not run. A job that failed names `key` as its reason.
fn stopped(marker: &Path, passed: bool, text: &str, key: &str) {
    assert!(!marker.exists(), "the program ran: {text}");
    if !passed {
        assert!(text.contains(key), "the result says why: {text}");
    }
}

#[test]
fn race_hook_does_not_run_in_a_worktree_add_job() {
    let mut w = world();
    let (program, marker) = w.attack("hook", "exit 0");
    let hooks = w.dir.path().join("evil-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::copy(&program, hooks.join("post-checkout")).unwrap();
    make_executable(&hooks.join("post-checkout"));
    let row = w.reserve("t1", "fleet/t1");
    let (repo, hooks_s) = (w.repo.clone(), hooks.display().to_string());
    let git = custody_git::tree_writer(w.fleet_git(), &row)
        .unwrap()
        .with_race(once("add", move || {
            set_config(&repo, "core.hooksPath", &hooks_s)
        }));

    let ran = custody_git::worktree_add(&git, &row);

    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    stopped(&marker, true, &ran.outcome.text, "");
}

#[test]
fn race_smudge_filter_does_not_run_in_a_worktree_add_job() {
    let mut w = world();
    let (program, marker) = w.attack("smudge", "cat");
    let row = w.reserve("t1", "fleet/t1");
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = custody_git::tree_writer(w.fleet_git(), &row)
        .unwrap()
        .with_race(once("add", move || {
            select_driver(&repo, "filter=evil");
            set_config(&repo, "filter.evil.smudge", &program_s);
        }));

    let ran = custody_git::worktree_add(&git, &row);

    stopped(
        &marker,
        ran.outcome.passed,
        &ran.outcome.text,
        "filter.evil.smudge",
    );
}

#[test]
fn race_clean_filter_does_not_run_in_a_rescue_job() {
    let mut w = world();
    let (program, marker) = w.attack("clean", "cat");
    let row = w.made("t1");
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w.fleet_git().with_race(once("add", move || {
        select_driver(&repo, "filter=evil");
        set_config(&repo, "filter.evil.clean", &program_s);
    }));

    let ran = rescue_git::rescue(&git, &w.fleet_root(), &row);

    // The scan of the next git call finds the key, so the rescue fails and
    // the tree stays.
    assert!(!ran.outcome.passed, "{}", ran.outcome.text);
    stopped(&marker, false, &ran.outcome.text, "filter.evil.clean");
    assert!(w.tree_path("t1").join("new.txt").is_file());
}

#[test]
fn race_filter_in_a_rescue_writes_only_its_own_index_folder_of_the_shared_tmp() {
    let mut w = world();
    let tmp = w.fleet_root().join("tmp");
    let script = format!(
        "echo ran > '{t}/t1/ran'\necho x > '{t}/stolen.txt'\nmkdir '{t}/t2'\ncat",
        t = tmp.display()
    );
    let (program, _) = w.attack("tmp", &script);
    let row = w.made("t1");
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w.fleet_git().with_race(once("add", move || {
        select_driver(&repo, "filter=evil");
        set_config(&repo, "filter.evil.clean", &program_s);
    }));

    let ran = rescue_git::rescue(&git, &w.fleet_root(), &row);

    assert!(
        tmp.join("t1").join("ran").exists(),
        "the filter ran: {}",
        ran.outcome.text
    );
    assert!(!tmp.join("stolen.txt").exists(), "{}", ran.outcome.text);
    assert!(!tmp.join("t2").exists(), "{}", ran.outcome.text);
}

#[test]
fn race_fsmonitor_does_not_run_in_a_worktree_list_job() {
    let mut w = world();
    let (program, marker) = w.attack("fsmonitor", "exit 1");
    let row = w.made("t1");
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w.fleet_git().with_race(once("status", move || {
        set_config(&repo, "core.fsmonitor", &program_s)
    }));

    let ran = custody_git::worktree_list(&git, &w.repo, &[row]);

    stopped(
        &marker,
        ran.outcome.passed,
        &ran.outcome.text,
        "core.fsmonitor",
    );
}

#[test]
fn race_merge_driver_does_not_run_in_a_fleetd_git_call() {
    let w = world();
    let (program, marker) = w.attack("merge", "exit 1");
    git(&w.repo, &["checkout", "-q", "-b", "other"]);
    std::fs::write(w.repo.join("a.txt"), "other\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "other"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "main\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "main"]);
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w
        .fleet_git()
        .writing(&[w.repo.join(".git")])
        .with_race(once("merge-tree", move || {
            select_driver(&repo, "merge=evil");
            set_config(&repo, "merge.evil.driver", &program_s);
        }));

    let out = git.checked(&w.repo, &["merge-tree", "--write-tree", "main", "other"]);

    stopped(&marker, true, &format!("{out:?}"), "");
}

#[test]
fn race_driver_cannot_use_the_network_in_a_fleetd_git_call() {
    let w = world();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        "perl -MIO::Socket::INET -e 'IO::Socket::INET->new(PeerAddr => \"127.0.0.1\", PeerPort => {port})'\nexit 1"
    );
    let (program, _) = w.attack("network", &probe);
    git(&w.repo, &["checkout", "-q", "-b", "other"]);
    std::fs::write(w.repo.join("a.txt"), "other\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "other"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "main\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "main"]);
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w
        .fleet_git()
        // The repository is writable, so git gets to the driver, which runs
        // in the sandbox.
        .writing(std::slice::from_ref(&w.repo))
        .with_race(once("merge-tree", move || {
            select_driver(&repo, "merge=evil");
            set_config(&repo, "merge.evil.driver", &program_s);
        }));

    let out = git.checked(&w.repo, &["merge-tree", "--write-tree", "main", "other"]);

    listener.set_nonblocking(true).unwrap();
    let connection = listener.accept();
    assert!(
        matches!(&connection, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "the driver connected: {connection:?}, {out:?}"
    );
}

#[test]
fn race_driver_cannot_read_a_file_below_home_outside_the_job_paths() {
    let w = world();
    // The world folder is HOME: the repository and the fleet root are in
    // it, and so is a file of the user.
    let home = w.dir.path().canonicalize().unwrap();
    std::fs::write(home.join("secret.txt"), "secret\n").unwrap();
    // The program is outside HOME, so git can start it.
    let outside = tempfile::tempdir().unwrap();
    let program = outside.path().join("steal.sh");
    let script = format!(
        "#!/bin/sh\necho ran > '{repo}/ran.txt'\ncat '{home}/secret.txt' >> '{repo}/stolen.txt'\nexit 1\n",
        repo = w.repo.display(),
        home = home.display()
    );
    std::fs::write(&program, script).unwrap();
    make_executable(&program);
    git(&w.repo, &["checkout", "-q", "-b", "other"]);
    std::fs::write(w.repo.join("a.txt"), "other\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "other"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "main\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "main"]);
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w
        .fleet_git()
        .with_home(&home)
        // The repository is writable, so git gets to the driver, which runs
        // in the sandbox.
        .writing(std::slice::from_ref(&w.repo))
        .with_race(once("merge-tree", move || {
            select_driver(&repo, "merge=evil");
            set_config(&repo, "merge.evil.driver", &program_s);
        }));

    let out = git.checked(&w.repo, &["merge-tree", "--write-tree", "main", "other"]);

    assert!(w.repo.join("ran.txt").exists(), "the driver ran: {out:?}");
    let stolen = std::fs::read_to_string(w.repo.join("stolen.txt")).unwrap_or_default();
    assert!(!stolen.contains("secret"), "the driver read HOME: {out:?}");
}

/// git can read the metadata of each folder above a repository below the
/// real HOME, and nothing more: a driver cannot read `~/.ssh` or a file
/// beside the repository, and a listing of a parent folder shows no real
/// entry of it.
#[test]
fn race_driver_in_a_repository_below_home_cannot_read_or_list_home() {
    let w = world_below_home();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap();
    let world = w.dir.path().canonicalize().unwrap();
    let beside = world.join("secret.txt");
    std::fs::write(&beside, "secret\n").unwrap();
    // The program is outside HOME, so git can start it.
    let outside = tempfile::tempdir().unwrap();
    let program = outside.path().join("steal.sh");
    let ssh = home.join(".ssh");
    let script = format!(
        "#!/bin/sh\necho ran > '{repo}/ran.txt'\n\
         for f in '{ssh}'/* '{ssh}/config' '{ssh}/id_ed25519' '{ssh}/id_rsa' '{ssh}/known_hosts' '{beside}'; do\n\
         cat \"$f\" > /dev/null 2>&1 && echo \"read $f\" >> '{repo}/seen.txt'\ndone\n\
         for d in '{home}' '{ssh}' '{world}'; do\n\
         ls -A \"$d\" 2> /dev/null | sed \"s|^|$d/|\" >> '{repo}/listed.txt'\ndone\n\
         exit 1\n",
        repo = w.repo.display(),
        ssh = ssh.display(),
        beside = beside.display(),
        home = home.display(),
        world = world.display(),
    );
    std::fs::write(&program, script).unwrap();
    make_executable(&program);
    git(&w.repo, &["checkout", "-q", "-b", "other"]);
    std::fs::write(w.repo.join("a.txt"), "other\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "other"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "main\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "main"]);
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w
        .fleet_git()
        .writing(std::slice::from_ref(&w.repo))
        .with_race(once("merge-tree", move || {
            select_driver(&repo, "merge=evil");
            set_config(&repo, "merge.evil.driver", &program_s);
        }));

    let out = git.checked(&w.repo, &["merge-tree", "--write-tree", "main", "other"]);

    assert!(w.repo.join("ran.txt").exists(), "the driver ran: {out:?}");
    let seen = std::fs::read_to_string(w.repo.join("seen.txt")).unwrap_or_default();
    assert_eq!(seen, "", "the driver read below HOME: {out:?}");
    // macOS refuses each listing. On Linux bwrap makes the parent folders
    // of each mount in a new tmpfs: a listing shows only the parents of the
    // repository and of the hidden run folder.
    let listed = std::fs::read_to_string(w.repo.join("listed.txt")).unwrap_or_default();
    let run = world.join("fleet").join("run");
    for path in listed.lines() {
        assert!(
            w.repo.starts_with(path) || run.starts_with(path),
            "the driver listed {path}, which is not a parent of a mount: {listed}"
        );
    }
}

#[test]
fn race_driver_cannot_read_the_operator_token_in_a_fleetd_git_call() {
    let w = world();
    let run = w.fleet_root().join("run");
    std::fs::create_dir_all(&run).unwrap();
    let token = run.join("operator.token");
    std::fs::write(&token, "secret\n").unwrap();
    // The driver writes its own marker in the repository: the marker of
    // `attack` is outside the paths of the job.
    let (ran, stolen) = (w.repo.join("ran.txt"), w.repo.join("stolen.txt"));
    let probe = format!(
        "echo ran > '{}'\ncat '{}' >> '{}'\nexit 1",
        ran.display(),
        token.display(),
        stolen.display()
    );
    let (program, _) = w.attack("token", &probe);
    git(&w.repo, &["checkout", "-q", "-b", "other"]);
    std::fs::write(w.repo.join("a.txt"), "other\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "other"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "main\n").unwrap();
    git(&w.repo, &["commit", "-q", "-am", "main"]);
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w
        .fleet_git()
        // The repository and the token are writable, so only the operator
        // denials of the job keep the token from the driver.
        .writing(&[w.repo.clone(), token.clone()])
        .with_race(once("merge-tree", move || {
            select_driver(&repo, "merge=evil");
            set_config(&repo, "merge.evil.driver", &program_s);
        }));

    let out = git.checked(&w.repo, &["merge-tree", "--write-tree", "main", "other"]);

    assert!(ran.exists(), "the driver ran: {out:?}");
    let read = std::fs::read_to_string(&stolen).unwrap_or_default();
    assert!(
        !read.contains("secret"),
        "the driver read the token: {out:?}"
    );
}

#[test]
fn race_ssh_command_does_not_run_in_a_fleetd_git_call() {
    let w = world();
    let (program, marker) = w.attack("ssh", "exit 1");
    let (repo, program_s) = (w.repo.clone(), program.display().to_string());
    let git = w.fleet_git().with_race(once("ls-remote", move || {
        set_config(&repo, "core.sshCommand", &program_s)
    }));

    let out = git.checked(&w.repo, &["ls-remote", "ssh://git@example.invalid/r.git"]);

    stopped(&marker, true, &format!("{out:?}"), "");
}

/// A local HTTP server that answers each request with 401, so git asks
/// its credential helpers. Its URL.
fn asks_for_credentials() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/r.git", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"r\"\r\n\
                  Content-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    url
}

#[test]
fn race_credential_helper_does_not_run_in_a_fleetd_git_call() {
    let w = world();
    let (program, marker) = w.attack("credential", "exit 1");
    let url = asks_for_credentials();
    let (repo, helper) = (w.repo.clone(), format!("!{}", program.display()));
    let git = w.fleet_git().with_race(once("ls-remote", move || {
        set_config(&repo, "credential.helper", &helper)
    }));

    let out = git.checked(&w.repo, &["ls-remote", &url]);

    stopped(&marker, true, &format!("{out:?}"), "");
}

#[test]
fn race_external_diff_and_textconv_do_not_run_in_a_fleetd_git_call() {
    let mut w = world();
    let (external, external_marker) = w.attack("external-diff", "exit 0");
    let (textconv, textconv_marker) = w.attack("textconv", "cat \"$1\"");
    w.made("t1");
    let tree = w.tree_path("t1");
    let repo = w.repo.clone();
    let (external_s, textconv_s) = (
        external.display().to_string(),
        textconv.display().to_string(),
    );
    let git = w.fleet_git().with_race(once("diff", move || {
        set_config(&repo, "diff.external", &external_s);
        select_driver(&repo, "diff=evil");
        set_config(&repo, "diff.evil.textconv", &textconv_s);
    }));

    let out = git.checked(&tree, &["diff", "HEAD"]);

    stopped(&external_marker, true, &format!("{out:?}"), "");
    stopped(&textconv_marker, true, &format!("{out:?}"), "");
}

// ─── no sandbox ─────────────────────────────────────────────────────────

/// The fake answer of a host with no sandbox.
fn no_sandbox_host(
    _cmd: horch_platform::start::Command,
    _reach: &horch_platform::sandbox::Reach<'_>,
) -> Result<horch_platform::start::Command, horch_platform::NotSupported> {
    Err(horch_platform::NotSupported::new("sandbox", "test").because("the fake host has none"))
}

#[test]
fn job_on_a_host_with_no_sandbox_runs_no_git_and_opens_1_needs_you_item() {
    let mut w = world();
    for name in ["t1", "t2"] {
        let row = w.reserve(name, &format!("fleet/{name}"));
        let id = w.queue_add(&row);
        let mut job = w.job(id);

        let ran = executors::run_in(&w.conn, &job, no_sandbox_host).unwrap();

        assert!(!ran.outcome.passed);
        assert!(
            ran.outcome.text.starts_with(NO_SANDBOX),
            "{}",
            ran.outcome.text
        );
        assert!(ran.outcome.text.contains("the fake host has none"));
        assert!(!w.tree_path(name).exists(), "no git ran");
        assert_eq!(git(&w.repo, &["branch", "--list", &row.branch]), "");
        // The run ends `failed` with this text, and the command observes it.
        job.state = State::Failed;
        job.result_json = Some(
            serde_json::json!({
                "passed": false,
                "verdict": false,
                "text": ran.outcome.text,
                "data": ran.data,
            })
            .to_string(),
        );
        let config = w.config.clone();
        let tx = w
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let seen = create::observe(&ctx(&config), &tx, &job).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Dropped(_)), "{seen:?}");
    }
    let items: Vec<(String, String)> = w
        .conn
        .prepare("SELECT key, text FROM needs_you WHERE kind = ?1 AND state = 'open'")
        .unwrap()
        .query_map([create::GIT_SANDBOX_KIND], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(items.len(), 1, "1 item for each kind: {items:?}");
    assert_eq!(items[0].0, "git_sandbox:worktree_add");
    assert!(items[0].1.starts_with(NO_SANDBOX), "{}", items[0].1);
}

#[test]
fn job_with_no_absolute_home_runs_no_git() {
    let w = world();
    for home in ["", "relative/home"] {
        let fleet = w.fleet_git().with_home(home);

        let ran = custody_git::no_sandbox(&fleet).expect("no sandbox without an absolute HOME");
        assert!(
            ran.outcome.text.starts_with(NO_SANDBOX),
            "{}",
            ran.outcome.text
        );
        assert!(ran.outcome.text.contains("HOME"), "{}", ran.outcome.text);
        assert!(ran.data.is_some(), "the typed no_sandbox data");
        let e = fleet.git(&w.repo, &["branch", "no-home"]).unwrap_err();
        assert!(format!("{e:#}").contains(NO_SANDBOX), "{e:#}");
        assert!(!git(&w.repo, &["branch", "--list", "no-home"]).contains("no-home"));
    }
}

// ─── the rescue saves the bytes on disk ─────────────────────────────────

#[test]
fn rescue_saves_each_file_byte_for_byte_with_the_filters_off() {
    let mut w = world();
    let row = w.made("t1");
    let tree = w.tree_path("t1");
    // The tree converts line endings: a `.gitattributes` of the tree, and
    // `core.autocrlf` in the config of the repository.
    std::fs::write(tree.join(".gitattributes"), "*.txt text eol=lf\n").unwrap();
    git(&tree, &["add", ".gitattributes"]);
    git(&tree, &["commit", "-q", "-m", "attributes"]);
    git(&w.repo, &["config", "core.autocrlf", "true"]);
    let files: [(&str, &[u8]); 3] = [
        ("a.txt", b"changed\r\nline\r\n"),
        ("new.txt", b"untracked\r\n"),
        ("raw.bin", b"\x00\r\n\xff"),
    ];
    for (name, bytes) in files {
        std::fs::write(tree.join(name), bytes).unwrap();
    }

    let ran = rescue_git::rescue(&w.fleet_git(), &w.fleet_root(), &row);

    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    let commit = ran.data.unwrap()["commit"].as_str().unwrap().to_string();
    for (name, bytes) in files {
        let out = git_out(&tree, &["cat-file", "blob", &format!("{commit}:{name}")]);
        assert_eq!(out.stdout, bytes, "{name} is saved byte for byte");
    }
}

/// The variable that marks the child run of
/// [`rescue_runs_no_filter_of_the_global_config`].
const GLOBAL_CHILD: &str = "HORCH_T691_GLOBAL_CHILD";

/// Review F4 of T-287: the global config of the user defines an lfs-style
/// filter, and a `.gitattributes` of the project selects it. The rescue
/// runs no filter and saves the bytes on disk. The test runs again in a
/// child process with `GIT_CONFIG_GLOBAL` set, as a user has it.
#[test]
fn rescue_runs_no_filter_of_the_global_config() {
    let Some(global) = std::env::var_os(GLOBAL_CHILD) else {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("lfs.ran");
        let program = dir.path().join("lfs.sh");
        let script = format!(
            "#!/bin/sh\necho ran > '{}'\necho pointer\n",
            marker.display()
        );
        std::fs::write(&program, script).unwrap();
        make_executable(&program);
        let config = dir.path().join("gitconfig");
        let text = format!(
            "[filter \"lfs\"]\n\tclean = {0}\n\tsmudge = {0}\n\tprocess = {0}\n\trequired = true\n",
            program.display()
        );
        std::fs::write(&config, text).unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "rescue_runs_no_filter_of_the_global_config",
                "--nocapture",
            ])
            .env(GLOBAL_CHILD, &config)
            .env("GIT_CONFIG_GLOBAL", &config)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"));
        assert!(!marker.exists(), "the global filter ran");
        return;
    };
    assert_eq!(
        std::env::var_os("GIT_CONFIG_GLOBAL").as_deref(),
        Some(global.as_os_str())
    );
    let mut w = world();
    let row = w.made("t1");
    let tree = w.tree_path("t1");
    std::fs::write(tree.join(".gitattributes"), "*.bin filter=lfs\n").unwrap();
    std::fs::write(tree.join("big.bin"), b"\x00big\r\n").unwrap();

    let ran = rescue_git::rescue(&w.fleet_git(), &w.fleet_root(), &row);

    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    let commit = ran.data.unwrap()["commit"].as_str().unwrap().to_string();
    let out = git_out(&tree, &["cat-file", "blob", &format!("{commit}:big.bin")]);
    assert_eq!(out.stdout, b"\x00big\r\n");
}

/// The `git status` of a tree takes no optional lock: a job with the git
/// folder writable (a remove job) does not write the index of a worker
/// tree, so a worker commit never finds an `index.lock` of fleetd.
#[test]
fn tree_state_does_not_write_the_index_of_a_worker_tree() {
    use horch_core::fleetd::worktree_custody::application::ports::WorktreeGitGateway;

    let w = world();
    let tree = w.tree_path("t1");
    let tree_s = tree.display().to_string();
    git(
        &w.repo,
        &["worktree", "add", "-q", "-b", "fleet/t1", &tree_s, "main"],
    );
    let index = PathBuf::from(git(
        &tree,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    ));
    // The same bytes with a new time: a status with an optional lock
    // refreshes the index and writes it.
    let file = std::fs::File::options()
        .write(true)
        .open(tree.join("a.txt"))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(60))
        .unwrap();
    let before = std::fs::read(&index).unwrap();

    // A remove job may write the git folder and the tree.
    let gw =
        CliWorktreeGit::new("git", w.fleet_root()).writing(&[w.repo.join(".git"), tree.clone()]);
    let state = gw.tree(&tree).unwrap().expect("a tree");
    assert!(!state.dirty, "{state:?}");
    assert_eq!(
        std::fs::read(&index).unwrap(),
        before,
        "status wrote the index"
    );
}
