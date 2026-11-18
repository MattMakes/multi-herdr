//! The `rescue` job: a rescue ref under `refs/horch/rescue/`, read back and
//! verified, never overwritten (docs/specs/fleetd/design.md, "Rescue before
//! any cleanup", RR-9, RR-16, FM-25; docs/tickets/fleetd-p3/06-verified-rescue-job.md).
//!
//! Each test makes its own temporary git repository, ledger and fleet root
//! (`<temp>/fleet`, the fleet root of the temp ledger). A job process is a
//! small command (`true`, `sleep`) started through the launcher; the
//! executor runs in the test process with the recorded pid, as `horch job
//! run` runs it. A fault test runs the rescue with a git wrapper and writes
//! its result on the run.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, CheckRunner, JobCtx, JobKey, JobRunner, Queued, Ran,
    SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{self as job_rules, Kind, State};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::ideas::domain::needs_you::Item;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::adapters::rescue_git::{self, rescue_key, Rescue};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::rescue::{self, Observed};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use rusqlite::{Connection, TransactionBehavior};

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    /// The repository, as its canonical path.
    repo: PathBuf,
}

/// A temp ledger of the current schema, and a temp repository with 1
/// commit on `main` that holds `a.txt` and `.gitignore`.
fn world() -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "build/\n*.log\n.worktrees/\n").unwrap();
    git(&repo, &["add", "a.txt", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    World {
        dir,
        conn,
        config: FleetConfig::default(),
        repo,
    }
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
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

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
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

/// What the test compares before and after a rescue: the HEAD, the branch,
/// the index, the status and each file of the tree.
#[derive(Debug, PartialEq, Eq)]
struct TreeView {
    head: String,
    branch: String,
    tip: String,
    staged: String,
    index: String,
    unstaged: String,
    status: String,
    files: Vec<(String, String)>,
}

fn view(tree: &Path, branch: &str) -> TreeView {
    let mut files = Vec::new();
    collect(tree, tree, &mut files);
    files.sort();
    TreeView {
        head: git(tree, &["rev-parse", "HEAD"]),
        branch: git(tree, &["symbolic-ref", "HEAD"]),
        tip: git(tree, &["rev-parse", &format!("refs/heads/{branch}")]),
        staged: git(tree, &["diff", "--cached"]),
        index: git(tree, &["ls-files", "-s"]),
        unstaged: git(tree, &["diff"]),
        status: git(tree, &["status", "--porcelain=v1", "-uall", "--ignored"]),
        files,
    }
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        let name = path.strip_prefix(root).unwrap().display().to_string();
        if name == ".git" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            out.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

impl World {
    fn fleet_root(&self) -> PathBuf {
        self.dir.path().join("fleet")
    }

    fn tree_path(&self, name: &str) -> PathBuf {
        self.repo.join(".worktrees").join(name)
    }

    /// A tree `name` on the branch `fleet/<name>`, made by git, with an
    /// `orphaned` row: a tree with no live owner.
    fn orphan(&mut self, name: &str) -> PathBuf {
        let path = self.tree_path(name);
        let branch = format!("fleet/{name}");
        let path_s = path.display().to_string();
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", &branch, &path_s, "main"],
        );
        let new = NewWorktree {
            worktree_id: name.to_string(),
            task_id: None,
            worker_id: None,
            repo_root: self.repo.display().to_string(),
            path: path_s,
            branch,
            base: "main".to_string(),
            base_commit: Some(git(&self.repo, &["rev-parse", "main"])),
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
        trees::insert_orphan(&tx, None, &new, cause).unwrap();
        tx.commit().unwrap();
        path
    }

    /// A `live` attempt `id` of a worker of a new run.
    fn attempt(&mut self, id: &str) {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = runs::create(&tx, self.dir.path(), &new).unwrap();
        let worker = NewWorker {
            run_id: run.run_id,
            persona: "dev".into(),
            ..Default::default()
        };
        let worker_id = workers::insert(&tx, &worker).unwrap();
        tx.execute(
            "INSERT INTO attempts (id, worker_id, state) VALUES (?1, ?2, 'live')",
            [id, worker_id.as_str()],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn row(&self, name: &str) -> WorktreeRow {
        trees::get(&self.conn, name).unwrap().unwrap()
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Queue a `rescue` run of the tree `name`, keyed by its path and HEAD.
    fn queue(&mut self, name: &str) -> i64 {
        let path = self.tree_path(name);
        let key = rescue_key(
            &path.display().to_string(),
            &git(&path, &["rev-parse", "HEAD"]),
        );
        let key = JobKey {
            kind: Kind::Rescue,
            subject: name,
            input_key: &key,
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

    /// Start the run `id` at `clock` with the process `program args`
    /// through the launcher, as part 3 of a pass starts it.
    fn start(&mut self, id: i64, program: &str, args: &[&str], clock: i64) -> JobRow {
        let spec = LaunchSpec {
            program: program.into(),
            args: args.iter().map(Into::into).collect(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let job = self.job(id);
        let config = self.config.clone();
        let mut runner = JobRunner::new(&Launcher);
        let started = runner
            .start(
                &ctx(&config),
                &mut self.conn,
                &job,
                &spec,
                FleetSeconds(clock),
            )
            .unwrap();
        assert!(started, "the run {id} starts");
        self.job(id)
    }

    /// The run `id` to its end with the executor of `horch job run`.
    fn run(&mut self, id: i64) -> JobRow {
        let pid = self.start(id, "true", &[], 0).pid.unwrap();
        let config = self.config.clone();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
        };
        let written = jobs::run_job(&ctx(&config), &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        self.job(id)
    }

    /// The run `id` to its end with the rescue of the tree `name` through
    /// the git binary `bin`: the result is written on the run, and the pass
    /// observes that its process is gone.
    fn run_with_git(&mut self, id: i64, name: &str, bin: &Path) -> JobRow {
        self.start(id, "true", &[], 0);
        let ran = rescue_git::rescue(
            &CliWorktreeGit::new(bin),
            &self.fleet_root(),
            &self.row(name),
        );
        jobs_store::set_result(&self.conn, id, &result_json(&ran)).unwrap();
        self.see(id, Liveness::Dead, 0);
        self.job(id)
    }

    /// 1 observation of the `running` runs by the pass at `clock`, with
    /// `liveness` for the process of the run `id`.
    fn see(&mut self, id: i64, liveness: Liveness, clock: i64) -> jobs::Observed {
        let facts = [ProcessFact {
            key: id.to_string(),
            liveness,
        }];
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let seen = jobs::observe(&ctx(&config), &tx, &facts, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
        seen
    }

    /// The observation of the ended rescue run `id`, in 1 transaction.
    fn observe(&mut self, id: i64) -> Observed {
        let job = self.job(id);
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let seen = rescue::observe(&ctx(&config), &tx, &job).unwrap();
        tx.commit().unwrap();
        seen
    }

    /// A git that runs the real git after `script`, a shell fragment that
    /// sees the arguments in `$*`.
    fn git_wrapper(&self, script: &str) -> PathBuf {
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
        let path = self.dir.path().join("git-wrapper");
        write(
            &path,
            &format!("#!/bin/sh\n{script}\nexec '{real}' \"$@\"\n"),
        );
        make_executable(&path);
        path
    }
}

/// The `result_json` that `horch job run` writes for `ran`.
fn result_json(ran: &Ran) -> String {
    serde_json::json!({
        "passed": ran.outcome.passed,
        "verdict": ran.outcome.verdict,
        "text": ran.outcome.text,
        "data": ran.data,
    })
    .to_string()
}

fn text(job: &JobRow) -> String {
    outcome(job).map(|o| o.text).unwrap_or_default()
}

fn saved(job: &JobRow) -> Rescue {
    serde_json::from_value(result_data(job).expect("a passed rescue names its ref")).unwrap()
}

/// The paths of the files of `rev`.
fn files_of(dir: &Path, rev: &str) -> Vec<String> {
    let out = git(dir, &["ls-tree", "-r", "--name-only", rev]);
    out.lines().map(str::to_string).collect()
}

fn open_items(w: &World) -> Vec<Item> {
    needs_you::open_items(&w.conn).unwrap()
}

/// 1 commit, 1 modified tracked file and 1 untracked file in the tree
/// `path`.
fn work_in(path: &Path) {
    write(&path.join("b.txt"), "b\n");
    git(path, &["add", "b.txt"]);
    git(path, &["commit", "-q", "-m", "work"]);
    write(&path.join("a.txt"), "a2\n");
    write(&path.join("u.txt"), "u\n");
}

// ─── FM-25, RR-9 ────────────────────────────────────────────────────────

#[test]
fn fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    let before = view(&tree, "fleet/t1");
    let id = w.queue("t1");

    let job = w.run(id);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r = saved(&job);
    assert_eq!(w.observe(id), Observed::Rescued(r.rescue_ref.clone()));
    let row = w.row("t1");
    assert_eq!(row.state, WorktreeState::Rescued);
    assert_eq!(row.rescue_ref.as_deref(), Some(r.rescue_ref.as_str()));
    assert_eq!(r.rescue_ref, "refs/horch/rescue/t1/1");
    assert_eq!(git(&tree, &["rev-parse", &r.rescue_ref]), r.commit);
    assert_eq!(
        files_of(&tree, &r.rescue_ref),
        [".gitignore", "a.txt", "b.txt", "u.txt"]
    );
    assert_eq!(
        git(&tree, &["show", &format!("{}:a.txt", r.rescue_ref)]),
        "a2"
    );
    assert_eq!(
        git(&tree, &["show", &format!("{}:u.txt", r.rescue_ref)]),
        "u"
    );
    assert_eq!(
        git(&tree, &["rev-parse", &format!("{}^", r.rescue_ref)]),
        before.head
    );
    assert_eq!(r.old_head, before.head);
    assert_eq!(view(&tree, "fleet/t1"), before, "the tree did not change");
}

#[test]
fn rescue_keeps_the_branch_and_the_index_of_the_tree() {
    let mut w = world();
    let tree = w.orphan("t1");
    // A staged new file, and a file with a staged and an unstaged change.
    write(&tree.join("s.txt"), "s\n");
    write(&tree.join("a.txt"), "staged\n");
    git(&tree, &["add", "s.txt", "a.txt"]);
    write(&tree.join("a.txt"), "on disk\n");
    let before = view(&tree, "fleet/t1");
    let index = std::fs::read(w.repo.join(".git/worktrees/t1/index")).unwrap();
    let id = w.queue("t1");

    let job = w.run(id);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r = saved(&job);
    assert_eq!(view(&tree, "fleet/t1"), before);
    assert_eq!(
        std::fs::read(w.repo.join(".git/worktrees/t1/index")).unwrap(),
        index,
        "the index file of the tree is the same"
    );
    assert_eq!(
        git(&tree, &["show", &format!("{}:a.txt", r.rescue_ref)]),
        "on disk"
    );
    assert_eq!(
        git(&tree, &["show", &format!("{}:s.txt", r.rescue_ref)]),
        "s"
    );
    assert_eq!(git(&w.repo, &["branch", "--list", "fleet/*"]), "+ fleet/t1");
}

// ─── RR-16 ──────────────────────────────────────────────────────────────

#[test]
fn rr16_second_rescue_of_1_tree_does_not_overwrite_the_first_ref() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    let first = w.queue("t1");
    let job = w.run(first);
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r1 = saved(&job);
    w.observe(first);
    write(&tree.join("u.txt"), "u2\n");
    write(&tree.join("v.txt"), "v\n");

    let second = w.queue("t1");
    let job = w.run(second);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r2 = saved(&job);
    assert_eq!(r1.rescue_ref, "refs/horch/rescue/t1/1");
    assert_eq!(r2.rescue_ref, "refs/horch/rescue/t1/2");
    assert_ne!(r2.commit, r1.commit);
    assert_eq!(git(&tree, &["rev-parse", &r1.rescue_ref]), r1.commit);
    assert_eq!(git(&tree, &["rev-parse", &r2.rescue_ref]), r2.commit);
    assert_eq!(w.observe(second), Observed::Rescued(r2.rescue_ref.clone()));
    assert_eq!(
        w.row("t1").rescue_ref.as_deref(),
        Some(r2.rescue_ref.as_str())
    );
}

#[test]
fn rescue_ref_is_read_back_before_the_tree_is_rescued() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    // A ref write that git reports and that does not read back.
    let bin = w.git_wrapper(
        r#"case "$*" in *rev-parse*refs/horch/rescue/*)
  echo 0123456789012345678901234567890123456789; exit 0;;
esac"#,
    );
    let id = w.queue("t1");

    let job = w.run_with_git(id, "t1", &bin);

    assert_eq!(job.state, State::Failed, "{}", text(&job));
    assert!(text(&job).contains("reads back"), "{}", text(&job));
    assert_eq!(w.observe(id), Observed::Failed);
    let row = w.row("t1");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.rescue_ref, None);

    // With a ref that reads back, the run passes, and only the observation
    // of the run moves the row.
    let id = w.queue("t1");
    let job = w.run(id);
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r = saved(&job);
    assert_eq!(git(&tree, &["rev-parse", &r.rescue_ref]), r.commit);
    assert_eq!(w.row("t1").state, WorktreeState::Orphaned);
    assert_eq!(w.observe(id), Observed::Rescued(r.rescue_ref));
    assert_eq!(w.row("t1").state, WorktreeState::Rescued);
}

#[test]
fn failed_rescue_verification_keeps_the_tree() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    // A file changes between step 1 and step 4: just before the `add` of
    // the second private index.
    let late = tree.join("late.txt");
    let bin = w.git_wrapper(&format!(
        r#"case "$GIT_INDEX_FILE:$*" in *verify.index:*" add "*)
  echo late > '{}';;
esac"#,
        late.display()
    ));
    let id = w.queue("t1");

    let job = w.run_with_git(id, "t1", &bin);

    assert_eq!(job.state, State::Failed, "{}", text(&job));
    assert!(text(&job).contains("files on disk"), "{}", text(&job));
    assert_eq!(w.observe(id), Observed::Failed);
    let row = w.row("t1");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.rescue_ref, None);
    let items = open_items(&w);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].key, "rescue_failed:t1");
    assert_eq!(items[0].kind, "rescue_failed");
    assert!(tree.join("u.txt").is_file() && late.is_file());
    // The private index files are gone.
    let run = w.fleet_root().join("run");
    let left: Vec<_> = std::fs::read_dir(&run).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn rescue_with_a_fleet_root_in_the_project_makes_nothing_there() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    let before = view(&tree, "fleet/t1");
    let git_bin = CliWorktreeGit::new("git");

    // A link to the project below a `..` of a folder that does not exist.
    std::os::unix::fs::symlink(&w.repo, w.dir.path().join("project-link")).unwrap();
    // A fleet root whose `run` is a link out of the fleet root.
    let linked = w.dir.path().join("linked-fleet");
    std::fs::create_dir_all(w.dir.path().join("elsewhere")).unwrap();
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(w.dir.path().join("elsewhere"), linked.join("run")).unwrap();
    let refused = [
        (tree.join("fleet"), "is in the project"),
        (w.repo.join("sub/../fleet"), "has a `..`"),
        (w.dir.path().join("missing/../project-link"), "has a `..`"),
        (linked, "is not in the fleet root"),
    ];
    for (root, reason) in refused {
        let ran = rescue_git::rescue(&git_bin, &root, &w.row("t1"));

        assert!(!ran.outcome.passed);
        assert!(ran.outcome.text.contains(reason), "{}", ran.outcome.text);
    }
    assert!(!tree.join("fleet").exists() && !w.repo.join("fleet").exists());
    assert!(!w.repo.join("sub").exists() && !w.repo.join("run").exists());
    assert_eq!(view(&tree, "fleet/t1"), before);
    let refs = git(&tree, &["for-each-ref", "refs/horch/rescue/"]);
    assert_eq!(refs, "");
}

// ─── the time limit and abandoned runs (Q25) ────────────────────────────

#[test]
fn rescue_job_at_its_limit_is_not_rescued_and_runs_again_after_its_backoff() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    let id = w.queue("t1");
    let limit = i64::try_from(w.config.job_limit_s.rescue).unwrap();
    let grace = i64::try_from(w.config.stop_grace_s).unwrap();
    w.start(id, "sleep", &["30"], 0);

    // Past its limit and its grace, the pass stops the run.
    let seen = w.see(id, Liveness::Alive, limit + grace + 1);
    assert_eq!(seen.stops.len(), 1, "{seen:?}");
    assert!(seen.stops[0].kill);
    jobs::stop(&seen.stops[0]);
    // Its process is gone with no result.
    let at = limit + grace + 2;
    let seen = w.see(id, Liveness::Dead, at);
    assert_eq!(seen.ended, [(id, State::Abandoned)]);

    assert_eq!(w.observe(id), Observed::Kept);
    let row = w.row("t1");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.rescue_ref, None);
    assert!(tree.join("u.txt").is_file());
    // The next run waits for its backoff.
    let next = jobs_store::active(&w.conn, Kind::Rescue, "t1")
        .unwrap()
        .expect("the next run");
    let backoff = i64::try_from(w.config.job_backoff_s[0]).unwrap();
    assert_eq!(next.not_before, Some(FleetSeconds(at + backoff)));
    let due = |clock: i64| -> Vec<i64> {
        jobs::due_to_start(&w.conn, FleetSeconds(clock))
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect()
    };
    assert!(due(at + backoff - 1).is_empty());
    assert_eq!(due(at + backoff), [next.id]);
}

#[test]
fn rescue_job_abandoned_3_times_opens_1_needs_you_item() {
    let mut w = world();
    w.orphan("t1");
    w.config.abandoned_run_limit = 3;
    let mut id = w.queue("t1");
    let mut clock = 0;
    for n in 1..=3 {
        w.start(id, "true", &[], clock);
        let seen = w.see(id, Liveness::Dead, clock);
        assert_eq!(seen.ended, [(id, State::Abandoned)], "run {n}");
        assert_eq!(w.observe(id), Observed::Kept);
        if n < 3 {
            let next = jobs_store::active(&w.conn, Kind::Rescue, "t1")
                .unwrap()
                .unwrap();
            clock = next.not_before.unwrap().0;
            id = next.id;
        }
    }

    let job = w.job(id);
    assert_eq!(
        jobs_store::active(&w.conn, Kind::Rescue, "t1").unwrap(),
        None
    );
    let items = open_items(&w);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(
        items[0].key,
        job_rules::needs_you_key(Kind::Rescue, "t1", &job.input_key)
    );
    let row = w.row("t1");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.rescue_ref, None);
}

// ─── the manifest and ignored files ─────────────────────────────────────

#[test]
fn rescue_writes_its_manifest_with_ref_head_count_size_and_attempt() {
    let mut w = world();
    let tree = w.orphan("t1");
    w.attempt("A2");
    trees::set_owner(&w.conn, "t1", Some("A2")).unwrap();
    write(&tree.join("a.txt"), "a2\n");
    write(&tree.join("u.txt"), "u\n");
    let head = git(&tree, &["rev-parse", "HEAD"]);
    let id = w.queue("t1");

    let job = w.run(id);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let path = w.fleet_root().join("rescue").join("t1-1.json");
    let manifest: Rescue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    // `.gitignore` (25 bytes), `a.txt` (3) and `u.txt` (2).
    let expected = Rescue {
        rescue_ref: "refs/horch/rescue/t1/1".into(),
        commit: git(&tree, &["rev-parse", "refs/horch/rescue/t1/1"]),
        old_head: head,
        files: 3,
        bytes: 30,
        attempt: Some("A2".into()),
    };
    assert_eq!(manifest, expected);
    assert_eq!(saved(&job), expected);
    assert_eq!(
        git(
            &tree,
            &["log", "-1", "--format=%s", "refs/horch/rescue/t1/1"]
        ),
        "horch rescue t1 attempt A2"
    );
}

#[test]
fn ignored_files_are_not_in_the_rescue_ref() {
    let mut w = world();
    let tree = w.orphan("t1");
    write(&tree.join("build/out.bin"), "x\n");
    write(&tree.join("x.log"), "x\n");
    write(&tree.join("u.txt"), "u\n");
    let id = w.queue("t1");

    let job = w.run(id);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    let r = saved(&job);
    assert_eq!(
        files_of(&tree, &r.rescue_ref),
        [".gitignore", "a.txt", "u.txt"]
    );
    assert!(tree.join("build/out.bin").is_file() && tree.join("x.log").is_file());
}

// ─── the fleetd git rule ────────────────────────────────────────────────

#[test]
fn rescue_job_runs_no_hook_or_program_of_the_project() {
    let mut w = world();
    let tree = w.orphan("t1");
    work_in(&tree);
    let marker = w.dir.path().join("hook-ran");
    let hooks = w.repo.join(".git").join("hooks");
    for hook in [
        "post-checkout",
        "post-commit",
        "post-index-change",
        "reference-transaction",
    ] {
        let path = hooks.join(hook);
        write(
            &path,
            &format!("#!/bin/sh\necho {hook} >> '{}'\n", marker.display()),
        );
        make_executable(&path);
    }

    let id = w.queue("t1");
    let job = w.run(id);

    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(!marker.exists(), "a hook ran");

    // A key of the project that starts a program stops the rescue.
    write(&tree.join("u.txt"), "u2\n");
    let before = view(&tree, "fleet/t1");
    let monitor = w.dir.path().join("monitor.sh");
    write(
        &monitor,
        &format!("#!/bin/sh\necho fsmonitor >> '{}'\n", marker.display()),
    );
    make_executable(&monitor);
    git(
        &w.repo,
        &["config", "core.fsmonitor", &monitor.display().to_string()],
    );
    let id = w.queue("t1");

    let job = w.run(id);

    assert_eq!(job.state, State::Failed, "{}", text(&job));
    assert!(text(&job).contains("core.fsmonitor"), "{}", text(&job));
    assert_eq!(w.observe(id), Observed::Failed);
    assert_eq!(open_items(&w).len(), 1);
    assert!(!marker.exists(), "a program of the project ran");
    git(&w.repo, &["config", "--unset", "core.fsmonitor"]);
    assert_eq!(view(&tree, "fleet/t1"), before);
    assert_eq!(
        git(
            &tree,
            &["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"]
        ),
        "refs/horch/rescue/t1/1"
    );
}
