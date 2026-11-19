//! The worktree reconciler acts: each pass queues 1 `rescue` job for each
//! `orphaned` tree with no verified rescue ref, and 1 `worktree_remove`
//! job for each `landed` tree, and nothing else (docs/specs/fleetd/design.md,
//! "Migration plan" item 5, "Worktrees: leased, reconciled, never lost",
//! "Rescue before any cleanup", FM-25, FM-27; MAP.md of fleetd-p3, Q8 and
//! Q9; docs/tickets/fleetd-p3/18-reconciler-acts.md).
//!
//! Each test makes its own temporary git repository, ledger and fleet root
//! (`<temp>/fleet`, the fleet root of the temp ledger). [`World::pass`]
//! gives the reconciler the facts of part 1 as the pass reads them: the
//! folder of each row, the folders of `<repo>/.worktrees/` and the result
//! of the `worktree_list` executor. A job process is `true`, started
//! through the launcher; the executor runs in the test process with the
//! recorded pid, as `horch job run` runs it.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{
    FolderFact, Liveness, Presence, ProcessFact,
};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, CheckRunner, JobCtx, JobRunner, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::{
    worktree_list, CliWorktreeGit, ListResult,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::disk::disk_budget;
use horch_core::fleetd::worktree_custody::application::reconcile::{
    reconcile_worktrees, Facts, Listed, Report, WorkDir,
};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, TransactionBehavior};

const CLOCK: FleetSeconds = FleetSeconds(1_000);

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _temp: tempfile::TempDir,
    top: PathBuf,
    conn: Connection,
    config: FleetConfig,
    /// The repository, as its canonical path.
    repo: PathBuf,
}

/// A temp ledger of the current schema at generation 1, a fleet root, and
/// a temp repository with 1 commit on `main` that holds `a.txt` and
/// `.gitignore`.
fn world() -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
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
    std::fs::write(repo.join(".gitignore"), "build/\n.worktrees/\n").unwrap();
    git(&repo, &["add", "a.txt", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    World {
        _temp: temp,
        top,
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

/// Each file of `tree` but `.git`, with its text, in path order.
fn files(tree: &Path) -> Vec<(String, String)> {
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
    let mut out = Vec::new();
    collect(tree, tree, &mut out);
    out.sort();
    out
}

impl World {
    fn tree_path(&self, name: &str) -> PathBuf {
        self.repo.join(".worktrees").join(name)
    }

    /// A tree `name` on the branch `fleet/<name>`, made by git from `main`,
    /// with a row in `state` of the task `task`.
    fn tree(&mut self, name: &str, task: Option<&str>, state: &str) -> PathBuf {
        let path = self.tree_path(name);
        let branch = format!("fleet/{name}");
        let path_s = path.display().to_string();
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", &branch, &path_s, "main"],
        );
        let new = NewWorktree {
            worktree_id: name.to_string(),
            task_id: task.map(str::to_string),
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
        self.set_state(name, state);
        path
    }

    /// An `orphaned` tree `name` with a commit, a modified tracked file, a
    /// staged new file, an untracked file and an ignored file.
    fn orphan_with_work(&mut self, name: &str) -> PathBuf {
        let path = self.tree(name, None, "orphaned");
        write(&path.join("b.txt"), "b\n");
        git(&path, &["add", "b.txt"]);
        git(&path, &["commit", "-q", "-m", "work"]);
        write(&path.join("a.txt"), "a changed\n");
        write(&path.join("staged.txt"), "staged\n");
        git(&path, &["add", "staged.txt"]);
        write(&path.join("new/u.txt"), "untracked\n");
        write(&path.join("build/out.o"), "ignored\n");
        path
    }

    /// A tree `name` with 1 commit of `<name>.txt` on its branch, merged
    /// into `main`, in the state `landed` at that commit, as the land step
    /// leaves it.
    fn landed(&mut self, name: &str) -> PathBuf {
        let path = self.tree(name, None, "orphaned");
        let file = format!("{name}.txt");
        write(&path.join(&file), "landed\n");
        git(&path, &["add", &file]);
        git(&path, &["commit", "-q", "-m", "work"]);
        let head = git(&path, &["rev-parse", "HEAD"]);
        git(
            &self.repo,
            &["merge", "-q", "--ff-only", &format!("fleet/{name}")],
        );
        self.set_state(name, "landed");
        self.conn
            .execute(
                "UPDATE worktrees SET landed_commit = ?2 WHERE worktree_id = ?1",
                [name, head.as_str()],
            )
            .unwrap();
        path
    }

    /// A run of the repository in `status`, and 1 task of it.
    fn task_of_run(&mut self, status: &str) -> String {
        let new = NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            run_branch: Some("fleet/r1".into()),
            ..Default::default()
        };
        let run: Run = runs::create(&self.conn, &self.repo, &new).unwrap();
        // FR-12: a task needs an idea in flight of the feature of its run.
        horch_core::taskledger::schema::run_idea(
            &self.conn,
            &run.run_id,
            &horch_core::taskledger::util::now(),
            true,
        )
        .unwrap();
        self.conn
            .execute(
                "UPDATE runs SET status = ?2 WHERE run_id = ?1",
                [run.run_id.as_str(), status],
            )
            .unwrap();
        let t = NewTask::work("t1", "t1.md", "opus");
        tasks::add(&self.conn, &run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn set_state(&self, name: &str, state: &str) {
        self.conn
            .execute(
                "UPDATE worktrees SET state = ?2 WHERE worktree_id = ?1",
                [name, state],
            )
            .unwrap();
    }

    fn row(&self, name: &str) -> WorktreeRow {
        trees::get(&self.conn, name).unwrap().unwrap()
    }

    fn fleet(&self) -> PathBuf {
        self.top.join("fleet")
    }

    /// The facts of part 1 of a pass: the folder of each row, each folder
    /// of `<repo>/.worktrees/`, and the result of the `worktree_list`
    /// executor on the repository.
    fn facts(&self) -> Facts {
        let rows = trees::in_states(&self.conn, &WorktreeState::ALL).unwrap();
        let folders = rows
            .iter()
            .map(|r| FolderFact {
                key: r.worktree_id.clone(),
                presence: if Path::new(&r.path).is_dir() {
                    Presence::Present
                } else {
                    Presence::Missing
                },
            })
            .collect();
        let mut entries: Vec<String> = std::fs::read_dir(self.repo.join(".worktrees"))
            .unwrap()
            .map(|e| e.unwrap().path().display().to_string())
            .collect();
        entries.sort();
        let live: Vec<WorktreeRow> = rows
            .into_iter()
            .filter(|r| r.state != WorktreeState::Removed)
            .collect();
        let gw = CliWorktreeGit::new("git", self.fleet()).reading(std::slice::from_ref(&self.repo));
        let ran = worktree_list(&gw, &self.repo, &live);
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        let result: ListResult = serde_json::from_value(ran.data.unwrap()).unwrap();
        Facts {
            folders,
            work_dirs: vec![WorkDir {
                repo: self.repo.display().to_string(),
                folders: entries,
            }],
            lists: vec![Listed {
                repo: self.repo.display().to_string(),
                job: 0,
                seen_at: util::now(),
                result,
            }],
        }
    }

    /// Part 2 of 1 pass on `facts`: the reconciler, then the disk budget,
    /// in 1 transaction.
    fn pass_on(&mut self, facts: &Facts) -> Report {
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let report = reconcile_worktrees(&ctx(&config), &tx, facts, CLOCK).unwrap();
        disk_budget(&ctx(&config), &tx, CLOCK).unwrap();
        tx.commit().unwrap();
        report
    }

    /// 1 pass with the facts of the disk now.
    fn pass(&mut self) -> Report {
        let facts = self.facts();
        self.pass_on(&facts)
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Each run of `kind` of the tree `name`: (id, state).
    fn jobs_of(&self, kind: Kind, name: &str) -> Vec<(i64, State)> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([kind.as_str(), name], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ids.into_iter().map(|id| (id, self.job(id).state)).collect()
    }

    fn all_jobs(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE kind <> 'worktree_list'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// Start the run `id` with the process `true` through the launcher, as
    /// part 3 of a pass starts it.
    fn start(&mut self, id: i64) -> JobRow {
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
        let mut runner = JobRunner::new(&Launcher);
        let started = runner
            .start(&ctx(&config), &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap();
        assert!(started, "the run {id} starts");
        self.job(id)
    }

    /// The run `id` to its end with the executor of `horch job run`.
    fn run(&mut self, id: i64) -> JobRow {
        let pid = self.start(id).pid.unwrap();
        let config = self.config.clone();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let written = jobs::run_job(&ctx(&config), &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx(&config), &mut self.conn).unwrap();
        self.observe_end(id)
    }

    /// The run `id` started, then its process gone with `result` written:
    /// the pass observes its end.
    fn end_with(&mut self, id: i64, result: &serde_json::Value) -> JobRow {
        self.start(id);
        jobs_store::set_result(&self.conn, id, &result.to_string()).unwrap();
        self.observe_end(id)
    }

    fn observe_end(&mut self, id: i64) -> JobRow {
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let gone = ProcessFact {
            key: id.to_string(),
            liveness: Liveness::Dead,
        };
        jobs::observe(&ctx(&config), &tx, &[gone], CLOCK).unwrap();
        tx.commit().unwrap();
        self.job(id)
    }

    fn rescue_refs(&self) -> String {
        git(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"],
        )
    }

    fn open_kinds(&self) -> Vec<String> {
        needs_you::open_items(&self.conn)
            .unwrap()
            .into_iter()
            .map(|i| i.kind)
            .collect()
    }
}

fn text(job: &JobRow) -> String {
    outcome(job).map(|o| o.text).unwrap_or_default()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// FM-25: the pass queues 1 rescue job of an orphan; the job writes a
/// rescue ref, and the next pass reads it back as verified: `rescued`.
#[test]
fn orphan_gets_1_rescue_job_and_becomes_rescued_after_its_verification() {
    let mut w = world();
    let path = w.orphan_with_work("orphan");

    let report = w.pass();

    assert_eq!(report.rescues, ["orphan"], "{report:?}");
    assert!(report.removals.is_empty(), "{report:?}");
    let runs = w.jobs_of(Kind::Rescue, "orphan");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].1, State::Queued);
    assert_eq!(w.all_jobs(), 1, "only the rescue job");
    assert_eq!(w.row("orphan").state, WorktreeState::Orphaned);

    let ended = w.run(runs[0].0);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();

    let row = w.row("orphan");
    assert_eq!(row.state, WorktreeState::Rescued);
    let rescue_ref = row.rescue_ref.expect("a verified rescue ref");
    assert_eq!(w.rescue_refs(), rescue_ref);
    assert_eq!(
        git(&path, &["rev-parse", &rescue_ref]),
        result_data(&ended).unwrap()["commit"].as_str().unwrap(),
        "the row has the ref of the run"
    );
    // A rescued tree with no task is never removed by fleetd (Q8).
    let report = w.pass();
    assert!(
        report.rescues.is_empty() && report.removals.is_empty(),
        "{report:?}"
    );
    assert_eq!(w.all_jobs(), 1);
    assert!(path.is_dir());
}

/// FM-25: the rescue saves each commit, modified file, staged file and
/// untracked file of the orphan, and the tree keeps each one as it was.
#[test]
fn orphan_with_dirty_untracked_files_and_commits_keeps_each_one_after_the_pass() {
    let mut w = world();
    let path = w.orphan_with_work("orphan");
    let before = files(&path);
    let head = git(&path, &["rev-parse", "HEAD"]);
    let status = git(&path, &["status", "--porcelain=v1", "-uall", "--ignored"]);
    let index = git(&path, &["ls-files", "-s"]);

    w.pass();
    let (id, _) = w.jobs_of(Kind::Rescue, "orphan")[0];
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();

    assert_eq!(files(&path), before);
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&path, &["symbolic-ref", "HEAD"]),
        "refs/heads/fleet/orphan"
    );
    assert_eq!(
        git(&path, &["status", "--porcelain=v1", "-uall", "--ignored"]),
        status
    );
    assert_eq!(git(&path, &["ls-files", "-s"]), index);
    let rescue_ref = w.row("orphan").rescue_ref.expect("a rescue ref");
    for (file, want) in [
        ("a.txt", "a changed"),
        ("b.txt", "b"),
        ("staged.txt", "staged"),
        ("new/u.txt", "untracked"),
    ] {
        assert_eq!(
            git(&path, &["show", &format!("{rescue_ref}:{file}")]),
            want,
            "{file}"
        );
    }
    assert_eq!(
        git(&path, &["rev-parse", &format!("{rescue_ref}^")]),
        head,
        "the rescue commit has the HEAD as its parent"
    );
    assert!(w.jobs_of(Kind::WorktreeRemove, "orphan").is_empty());
}

/// FM-27: the pass queues 1 remove job of a landed tree; the job removes
/// it, and the next pass reads the row as `removed`.
#[test]
fn landed_tree_gets_1_remove_job_and_becomes_removed() {
    let mut w = world();
    let path = w.landed("done");

    let report = w.pass();

    assert_eq!(report.removals, ["done"], "{report:?}");
    assert!(report.rescues.is_empty(), "{report:?}");
    let runs = w.jobs_of(Kind::WorktreeRemove, "done");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(w.all_jobs(), 1, "only the remove job");
    // The second pass queues no second removal while the first is queued.
    let again = w.pass();
    assert!(again.removals.is_empty(), "{again:?}");
    assert_eq!(w.jobs_of(Kind::WorktreeRemove, "done").len(), 1);

    let ended = w.run(runs[0].0);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();

    assert_eq!(w.row("done").state, WorktreeState::Removed);
    assert!(!path.exists());
    let listed = git(&w.repo, &["worktree", "list", "--porcelain"]);
    assert!(!listed.contains(&path.display().to_string()), "{listed}");
    let report = w.pass();
    assert!(report.removals.is_empty(), "{report:?}");
    assert_eq!(w.all_jobs(), 1);
}

/// Q8: a rescued tree whose idea is in flight stays, also over the disk
/// budget; a rescued tree of a cancelled idea is removed for space, by the
/// budget and not by the reconciler (ticket 33 of fleetd-p6).
#[test]
fn rescued_tree_of_an_active_run_is_not_removed() {
    let mut w = world();
    w.config.worktree_disk_budget_gb = 0;
    let task = w.task_of_run("active");
    let path = w.tree("kept", Some(&task), "orphaned");
    write(&path.join("u.txt"), "untracked\n");
    w.pass();
    let (id, _) = w.jobs_of(Kind::Rescue, "kept")[0];
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();
    assert_eq!(w.row("kept").state, WorktreeState::Rescued);

    for _ in 0..2 {
        let report = w.pass();
        assert!(report.removals.is_empty(), "{report:?}");
    }

    assert!(w.jobs_of(Kind::WorktreeRemove, "kept").is_empty());
    assert_eq!(w.row("kept").state, WorktreeState::Rescued);
    assert_eq!(
        std::fs::read_to_string(path.join("u.txt")).unwrap(),
        "untracked\n"
    );

    // The same tree once its run is done: its idea is still in flight.
    w.conn
        .execute("UPDATE runs SET status = 'done'", [])
        .unwrap();
    w.pass();
    assert!(w.jobs_of(Kind::WorktreeRemove, "kept").is_empty());

    // The same tree once its idea is cancelled: the budget queues its
    // removal.
    w.conn
        .execute("UPDATE ideas SET state = 'cancelled'", [])
        .unwrap();
    let report = w.pass();
    assert!(report.removals.is_empty(), "{report:?}");
    assert_eq!(w.jobs_of(Kind::WorktreeRemove, "kept").len(), 1);
}

/// The index `jobs_one_active`: a rescue that is queued or running is not
/// queued again.
#[test]
fn rescue_job_is_queued_once_while_it_runs() {
    let mut w = world();
    w.orphan_with_work("orphan");

    let first = w.pass();
    let second = w.pass();

    assert_eq!(first.rescues, ["orphan"]);
    assert!(second.rescues.is_empty(), "{second:?}");
    let (id, _) = w.jobs_of(Kind::Rescue, "orphan")[0];
    w.start(id);
    assert_eq!(w.job(id).state, State::Running);
    let third = w.pass();

    assert!(third.rescues.is_empty(), "{third:?}");
    assert_eq!(w.jobs_of(Kind::Rescue, "orphan"), [(id, State::Running)]);
    assert_eq!(w.all_jobs(), 1);
}

/// FM-25, FM-27: a rescue that fails keeps the tree `orphaned` with 1
/// needs-you item; no removal is queued, and the same rescue is not
/// queued again.
#[test]
fn failed_rescue_leaves_the_tree_orphaned_and_queues_no_removal() {
    let mut w = world();
    let path = w.orphan_with_work("orphan");
    let before = files(&path);
    w.pass();
    let (id, _) = w.jobs_of(Kind::Rescue, "orphan")[0];
    let failed = serde_json::json!({
        "passed": false,
        "verdict": false,
        "text": "the read-back of the rescue ref failed",
    });
    let ended = w.end_with(id, &failed);
    assert_eq!(ended.state, State::Failed);

    for _ in 0..2 {
        let report = w.pass();
        assert!(
            report.rescues.is_empty() && report.removals.is_empty(),
            "{report:?}"
        );
    }

    let row = w.row("orphan");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.rescue_ref, None);
    assert_eq!(w.open_kinds(), ["rescue_failed"]);
    assert_eq!(w.jobs_of(Kind::Rescue, "orphan"), [(id, State::Failed)]);
    assert!(w.jobs_of(Kind::WorktreeRemove, "orphan").is_empty());
    assert_eq!(files(&path), before);
    assert_eq!(w.rescue_refs(), "");
}

/// A plain folder in `<repo>/.worktrees/` that `git worktree list` does
/// not name gets no row and no job; the pass reports it. A tree that git
/// names is adopted only once a `worktree_list` result names it.
#[test]
fn plain_folder_in_the_work_dir_gets_no_row_and_no_job() {
    let mut w = world();
    let plain = w.tree_path("plain");
    write(&plain.join("notes.txt"), "mine\n");
    let hand = w.tree_path("hand-made");
    let hand_s = hand.display().to_string();
    git(
        &w.repo,
        &["worktree", "add", "-q", "-b", "hand", &hand_s, "main"],
    );

    // No passed worktree_list result yet: nothing is adopted.
    let mut no_list = w.facts();
    no_list.lists.clear();
    let report = w.pass_on(&no_list);
    assert!(report.adopted.is_empty(), "{report:?}");
    assert!(report.not_trees.is_empty(), "{report:?}");

    let report = w.pass();

    assert_eq!(
        report.not_trees,
        [plain.display().to_string()],
        "{report:?}"
    );
    assert_eq!(report.adopted.len(), 1, "{report:?}");
    assert!(trees::by_path(&w.conn, &plain.display().to_string())
        .unwrap()
        .is_none());
    let adopted = trees::by_path(&w.conn, &hand_s).unwrap().unwrap();
    assert_eq!(adopted.state, WorktreeState::Orphaned);
    assert_eq!(
        w.all_jobs(),
        1,
        "only the rescue of the adopted tree: {:?}",
        w.jobs_of(Kind::Rescue, &adopted.worktree_id)
    );
    assert_eq!(
        std::fs::read_to_string(plain.join("notes.txt")).unwrap(),
        "mine\n"
    );
}

/// FM-25 for a reused tree: a rescued tree that the next attempt took and
/// left orphaned at the same HEAD gets a new rescue of its new work. The
/// passed rescue of the first orphan, and its ref, do not count for it.
#[test]
fn reused_tree_orphaned_again_at_the_same_head_gets_a_new_rescue() {
    let mut w = world();
    let path = w.orphan_with_work("reused");
    w.pass();
    let (first, _) = w.jobs_of(Kind::Rescue, "reused")[0];
    let ended = w.run(first);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();
    let old_ref = w.row("reused").rescue_ref.expect("the first rescue ref");
    // The next attempt takes the tree, edits a file, and dies.
    w.set_state("reused", "active");
    w.set_state("reused", "orphaned");
    write(&path.join("a.txt"), "edited again\n");

    let report = w.pass();

    assert_eq!(w.row("reused").state, WorktreeState::Orphaned);
    assert_eq!(report.rescues, ["reused"], "{report:?}");
    let runs = w.jobs_of(Kind::Rescue, "reused");
    assert_eq!(runs.len(), 2, "{runs:?}");
    let ended = w.run(runs[1].0);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();
    let row = w.row("reused");
    assert_eq!(row.state, WorktreeState::Rescued);
    let new_ref = row.rescue_ref.expect("the second rescue ref");
    assert_ne!(new_ref, old_ref);
    assert_eq!(
        git(&path, &["show", &format!("{new_ref}:a.txt")]),
        "edited again"
    );
    assert_eq!(
        git(&path, &["show", &format!("{old_ref}:a.txt")]),
        "a changed",
        "the first ref stays"
    );
}

/// FM-27: the guard refuses the removal of a landed tree with an untracked
/// file. The tree stays with 1 needs-you item, and the same removal is not
/// queued again on each pass.
#[test]
fn refused_removal_keeps_the_tree_and_is_not_queued_again() {
    let mut w = world();
    let path = w.landed("done");
    write(&path.join("late.txt"), "late work\n");
    w.pass();
    let (id, _) = w.jobs_of(Kind::WorktreeRemove, "done")[0];
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));

    for _ in 0..3 {
        let report = w.pass();
        assert!(report.removals.is_empty(), "{report:?}");
    }

    assert_eq!(
        w.jobs_of(Kind::WorktreeRemove, "done"),
        [(id, State::Failed)]
    );
    let items = needs_you::open_items(&w.conn).unwrap();
    assert!(
        items[0].text.contains(&format!("horch job requeue {id}")),
        "{}",
        items[0].text
    );
    assert_eq!(w.row("done").state, WorktreeState::Landed);
    assert_eq!(
        std::fs::read_to_string(path.join("late.txt")).unwrap(),
        "late work\n"
    );
    assert_eq!(w.open_kinds().len(), 1, "{:?}", w.open_kinds());
}
