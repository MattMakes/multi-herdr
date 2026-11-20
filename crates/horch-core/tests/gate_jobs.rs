//! The `fast_gate` and `full_gate` jobs and the gate tree of a feature
//! (docs/specs/fleetd/design.md, "Between features" and "The kinds of
//! job"; docs/tickets/fleetd-p4/28-gate-jobs.md, MAP.md Q18, Q19, Q24 and
//! Q26).
//!
//! Each test makes its own git repository and ledger in [`world_base`],
//! not below the temp folder of this process: the gate sandbox lets a gate
//! write its temp folder, so a path there proves no refusal. A job process is a small command started through the launcher,
//! and the executor runs in the test process with the recorded pid, as
//! `horch job run` runs it. The test of the time limit starts this test
//! binary as the job process.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{self, ProcessFact, ScanClock};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, Outcome, State};
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_gate::{
    self, gate_key, no_gate_sandbox_key, GateRequest, GateSandbox,
};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::schema;
use horch_core::taskledger::store::Store;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::Worker;
use horch_platform::sandbox::{sandboxed, Denials};
use horch_platform::NotSupported;
use rusqlite::{Connection, Transaction, TransactionBehavior};

const FEATURE: &str = "F-1";

/// The variable that makes this test binary the job process of the time
/// limit test: `<ledger path>|<job id>`.
const JOB_CHILD: &str = "HORCH_GATE_JOBS_CHILD";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    run: Run,
    /// The repository, as its canonical path.
    repo: PathBuf,
    /// The tip of `main`.
    main: String,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// A ledger of the current schema with the run `r1` and the feature
/// [`FEATURE`] on `fleet/r1`, and a repository with 1 commit on `main`.
fn world() -> World {
    let dir = tempfile::tempdir_in(world_base()).unwrap();
    let mut conn = open(&dir.path().join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("marker.txt"), "main\n").unwrap();
    git(&repo, &["add", "marker.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let main = git(&repo, &["rev-parse", "HEAD"]);
    // The worktree_add job writes only the tree folder: its parent exists.
    std::fs::create_dir(repo.join(".worktrees")).unwrap();
    std::fs::create_dir_all(dir.path().join("fleet")).unwrap();
    let tx = conn.transaction().unwrap();
    let new = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = runs::create(&tx, &repo, &new).unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some("fleet/r1"),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &feature, cause("the run starts")).unwrap();
    // FR-12: a task of the run needs an idea in flight of the run feature.
    let run_feature = format!("F-{:04}", 1);
    if rows::feature(&tx, &run_feature).unwrap().is_none() {
        let new = NewFeature {
            id: &run_feature,
            name: Some("r1 run"),
            branch: None,
            primary_branch: "main",
        };
        rows::insert_feature(&tx, None, &new, cause("the run starts")).unwrap();
    }
    let idea = NewIdea {
        id: "I-1",
        feature_id: &run_feature,
        priority: 0,
    };
    ideas_store::insert_idea(&tx, None, &idea, cause("idea added")).unwrap();
    tx.commit().unwrap();
    World {
        dir,
        conn,
        config: FleetConfig::default(),
        run,
        repo,
        main,
    }
}

/// A folder for the worlds of these tests outside the temp folder of this
/// process, which the gate sandbox lets a gate write, and outside HOME,
/// which the git sandbox of fleetd hides.
fn world_base() -> PathBuf {
    let canon = |p: PathBuf| p.canonicalize().unwrap_or(p);
    let temp = canon(std::env::temp_dir());
    let home = canon(PathBuf::from(std::env::var_os("HOME").unwrap_or_default()));
    let base = ["/var/tmp", "/tmp"]
        .into_iter()
        .map(|p| canon(PathBuf::from(p)))
        .find(|p| p.is_dir() && !p.starts_with(&temp) && !p.starts_with(&home))
        .expect("a folder outside the temp folder and HOME");
    let base = base.join("horch-gate-jobs");
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
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

/// The sandbox profiles of a test: no task check runs in these tests.
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

/// A gate sandbox that this host does not have.
fn no_sandbox(
    _cmd: horch_platform::start::Command,
    _writable: &[PathBuf],
    _denied: &Denials,
) -> Result<horch_platform::start::Command, NotSupported> {
    Err(NotSupported::new("sandbox", "test").because("the test host has no sandbox"))
}

/// Whether this host has the gate sandbox. A Linux host with no `bwrap`
/// has none: the test then proves only that the gate does not run
/// (MAP.md of fleetd-p4, Q-A).
fn has_sandbox() -> bool {
    match exec_gate::sandbox_gap(sandboxed) {
        None => true,
        Some(gap) if cfg!(target_os = "linux") => {
            eprintln!("no gate sandbox on this Linux host: {gap}");
            false
        }
        Some(gap) => panic!("no gate sandbox: {gap}"),
    }
}

impl World {
    fn db(&self) -> PathBuf {
        self.dir.path().join("ledger.sqlite3")
    }

    fn fleet_root(&self) -> PathBuf {
        self.dir.path().join("fleet")
    }

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

    /// A commit on `branch` from `from` with each file of `files`. Gives
    /// its tip.
    fn commit(&self, branch: &str, from: &str, files: &[(&str, &str)]) -> String {
        git(&self.repo, &["checkout", "-q", "-B", branch, from]);
        for (name, text) in files {
            std::fs::write(self.repo.join(name), text).unwrap();
            git(&self.repo, &["add", name]);
        }
        git(&self.repo, &["commit", "-q", "-m", branch]);
        let tip = git(&self.repo, &["rev-parse", "HEAD"]);
        git(&self.repo, &["checkout", "-q", "main"]);
        tip
    }

    /// A commit on `branch` from `main` whose project `fleet.toml` has the
    /// gates `gates` and whose `marker.txt` is `marker`.
    fn tip_with(&self, branch: &str, gates: &str, marker: &str) -> String {
        let main = self.main.clone();
        self.commit(
            branch,
            &main,
            &[
                ("fleet.toml", gates),
                ("marker.txt", &format!("{marker}\n")),
            ],
        )
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn gate_path(&self) -> PathBuf {
        gate_tree::path(&self.repo, FEATURE)
    }

    /// The gate tree of [`FEATURE`], made by its `worktree_add` job and
    /// observed, as a pass observes it.
    fn gate_tree(&mut self) -> trees::WorktreeRow {
        let repo = self.repo.clone();
        let main = self.main.clone();
        let run_id = self.run.run_id.clone();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &run_id,
            repo: &repo,
            base: "fleet/r1",
            tip: &main,
        };
        let job = match self.tx(|ctx, tx| gate_tree::reserve(ctx, tx, &req, FleetSeconds(0))) {
            Ok(GateTree::Reserved { job, .. }) => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        };
        let ended = self.run(job);
        assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
        let seen = self.tx(|ctx, tx| create::observe(ctx, tx, &ended)).unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
        gate_tree::of_feature(&self.conn, &repo, FEATURE)
            .unwrap()
            .unwrap()
    }

    /// A bundle of 1 task branch of [`FEATURE`] on `base_tip`.
    fn bundle(&mut self, base_tip: &str) -> i64 {
        let run = self.run.clone();
        let tx = self.conn.transaction().unwrap();
        let new = NewTask::work("t1", "t1.md", "dev");
        let task = tasks::add(&tx, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let branch = NewBranch {
            name: "fleet/r1/t1",
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 100,
        };
        rows::insert_branch(&tx, None, &branch, cause("task bound")).unwrap();
        let bundle = NewBundle {
            feature_id: FEATURE,
            base_tip,
            parent_id: None,
            members: &["fleet/r1/t1"],
            due_at: 0,
        };
        let id = rows::insert_bundle(&tx, None, &bundle, cause("bundle built")).unwrap();
        tx.commit().unwrap();
        id
    }

    /// Queue the gate `kind` of `subject` on `tip` over `base_tip`.
    fn queue_gate(
        &mut self,
        kind: Kind,
        subject: &str,
        tip: &str,
        base_tip: &str,
    ) -> Option<Queued> {
        let repo = self.repo.clone();
        let req = GateRequest {
            kind,
            subject,
            feature_id: FEATURE,
            tip,
            base_tip,
            repo: &repo,
        };
        self.tx(|ctx, tx| exec_gate::queue(ctx, tx, &req, FleetSeconds(0)))
            .unwrap()
    }

    fn new_gate(&mut self, kind: Kind, subject: &str, tip: &str, base_tip: &str) -> i64 {
        match self.queue_gate(kind, subject, tip, base_tip) {
            Some(Queued::New(id)) => id,
            other => panic!("expected a new run, got {other:?}"),
        }
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
        let job = Store::open_read_only(&self.db()).unwrap();
        let written = jobs::run_job(&ctx, job.conn(), id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.job(id)
    }

    /// The executor of the gate `row` with the sandbox `sandbox` on a
    /// read-only connection, then its facts recorded in 1 transaction, as
    /// the ingest of its result file records them. Give its outcome.
    fn gate_with(&mut self, row: &JobRow, sandbox: GateSandbox) -> Outcome {
        let job = Store::open_read_only(&self.db()).unwrap();
        let ran = exec_gate::run_in(job.conn(), row, sandbox).unwrap();
        self.tx(|ctx, tx| exec_gate::record(ctx, tx, row, &ran))
            .unwrap();
        ran.outcome
    }

    /// Start the run `id` through the launcher, as part 3 of a pass starts
    /// it. Give the started row.
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
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        assert!(JobRunner::new(&Launcher)
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap());
        self.job(id)
    }

    /// End each run from its result file, as a pass of fleetd ends it.
    fn ingest(&mut self) -> Vec<(i64, State)> {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        jobs::end_from_files(&ctx, &mut self.conn).unwrap()
    }

    /// The `result` of the newest gate run of [`FEATURE`] on `tip` over
    /// `base`.
    fn gate_result(&self, gate: &str, tip: &str, base: &str) -> Option<String> {
        let row = rows::gate_run_of(&self.conn, FEATURE, gate, tip, base)
            .unwrap()
            .expect("a gate run");
        row.result
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

    /// Reap and scan until the process of the run `id` is gone.
    fn wait_gone(&self, runner: &mut JobRunner<'_>, id: i64) -> Vec<ProcessFact> {
        let until = Instant::now() + Duration::from_secs(20);
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
}

fn text(row: &JobRow) -> String {
    outcome(row).map(|o| o.text).unwrap_or_default()
}

/// Whether `pid` runs. A zombie (state `Z`) is dead: a killed process stays a
/// zombie, and `kill -0` still succeeds on it, until its parent or the
/// subreaper of its sandbox reaps it. On Linux CI that reaper can be slow or
/// absent, so the state decides, not `kill -0`.
fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&out.stdout);
    let state = state.trim();
    out.status.success() && !state.is_empty() && !state.starts_with('Z')
}

// ─── the gate tree ──────────────────────────────────────────────────────

#[test]
fn gate_tree_is_a_custody_row_with_no_task() {
    let mut w = world();
    let tree = w.gate_tree();
    assert_eq!(tree.path, w.gate_path().display().to_string());
    assert!(tree.path.ends_with("/.worktrees/gate-F-1"), "{}", tree.path);
    assert_eq!(tree.task_id, None);
    assert_eq!(tree.worker_id, None);
    assert_eq!(tree.owner_attempt, None, "the owner is fleetd: no attempt");
    assert_eq!(tree.state, WorktreeState::Created);
    assert_eq!(tree.repo_root, w.repo.display().to_string());
    assert!(w.gate_path().is_dir());
    assert_eq!(git(&w.gate_path(), &["rev-parse", "HEAD"]), w.main);

    // The next request of the feature finds the tree: no second row.
    let repo = w.repo.clone();
    let main = w.main.clone();
    let run_id = w.run.run_id.clone();
    let req = GateTreeRequest {
        feature_id: FEATURE,
        run_id: &run_id,
        repo: &repo,
        base: "fleet/r1",
        tip: &main,
    };
    let again = w
        .tx(|ctx, tx| gate_tree::reserve(ctx, tx, &req, FleetSeconds(5)))
        .unwrap();
    assert!(matches!(again, GateTree::Ready(ref r) if r.worktree_id == tree.worktree_id));

    // The end of the feature gives the tree to custody, which rescues and
    // removes it.
    assert!(w
        .tx(|ctx, tx| gate_tree::release(ctx, tx, &repo, FEATURE, "its feature is merged"))
        .unwrap());
    let row = trees::get(&w.conn, &tree.worktree_id).unwrap().unwrap();
    assert_eq!(row.state, WorktreeState::Orphaned);
}

// ─── the gates ──────────────────────────────────────────────────────────

#[test]
fn fast_gate_runs_the_declared_command_on_the_candidate_tip() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let gates = "[gates]\nfast = \"cat marker.txt > seen.txt\"\nfull = \"false\"\n";
    let candidate = w.tip_with("candidate", gates, "candidate");
    let main = w.main.clone();
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &candidate, &main);
    let job = w.job(id);
    assert_eq!(job.input_key, gate_key(&candidate, &main));
    assert_eq!(job.repo.as_deref(), Some(w.repo.to_str().unwrap()));
    assert_eq!(
        w.gate_result("fast", &candidate, &main),
        None,
        "null while it runs"
    );

    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let tree = w.gate_path();
    assert_eq!(git(&tree, &["rev-parse", "HEAD"]), candidate);
    assert_eq!(
        std::fs::read_to_string(tree.join("seen.txt")).unwrap(),
        "candidate\n"
    );
    assert_eq!(
        w.gate_result("fast", &candidate, &main).as_deref(),
        Some("pass")
    );
}

#[test]
fn full_gate_runs_on_the_feature_tip() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let gates = "[gates]\nfast = \"false\"\nfull = \"cat marker.txt > seen.txt; exit 3\"\n";
    let feature = w.tip_with("fleet/r1", gates, "feature");
    let main = w.main.clone();
    let id = w.new_gate(Kind::FullGate, FEATURE, &feature, &main);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed);
    let o = outcome(&ended).unwrap();
    assert!(o.verdict, "the gate ran to its end: {}", o.text);
    assert!(o.text.contains("exit 3"), "{}", o.text);
    let tree = w.gate_path();
    assert_eq!(git(&tree, &["rev-parse", "HEAD"]), feature);
    assert_eq!(
        std::fs::read_to_string(tree.join("seen.txt")).unwrap(),
        "feature\n"
    );
    assert_eq!(
        w.gate_result("full", &feature, &main).as_deref(),
        Some("fail")
    );

    // A feature tip that does not contain the fetched primary tip waits for
    // the rebase: the gate does not run, and the run has no verdict.
    let primary = w.commit("main", &main, &[("other.txt", "x\n")]);
    std::fs::remove_file(tree.join("seen.txt")).unwrap();
    let id = w.new_gate(Kind::FullGate, FEATURE, &feature, &primary);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed);
    let o = outcome(&ended).unwrap();
    assert!(!o.verdict);
    assert!(o.text.contains("rebase"), "{}", o.text);
    assert!(!tree.join("seen.txt").exists(), "the gate did not run");
    assert_eq!(w.gate_result("full", &feature, &primary), None);
}

#[test]
fn passed_gate_of_1_key_is_used_again() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let main = w.main.clone();
    let pass = w.tip_with("a", "[gates]\nfast = \"true\"\nfull = \"true\"\n", "a");
    let id = w.new_gate(Kind::FullGate, FEATURE, &pass, &main);
    assert_eq!(w.run(id).state, State::Passed);
    match w.queue_gate(Kind::FullGate, FEATURE, &pass, &main) {
        Some(Queued::Reused(r)) => assert_eq!(r.id, id),
        other => panic!("expected the passed run again, got {other:?}"),
    }

    // A fail of a gate that ran to its end is used again.
    let fail = w.tip_with("b", "[gates]\nfast = \"true\"\nfull = \"false\"\n", "b");
    let id = w.new_gate(Kind::FullGate, FEATURE, &fail, &main);
    assert_eq!(w.run(id).state, State::Failed);
    match w.queue_gate(Kind::FullGate, FEATURE, &fail, &main) {
        Some(Queued::Reused(r)) => assert_eq!(r.id, id),
        other => panic!("expected the failed run again, got {other:?}"),
    }

    // A fail with no verdict runs again.
    let none = w.tip_with("c", "[gates]\nfast = \"true\"\n", "c");
    let id = w.new_gate(Kind::FullGate, FEATURE, &none, &main);
    assert_eq!(w.run(id).state, State::Failed);
    match w.queue_gate(Kind::FullGate, FEATURE, &none, &main) {
        Some(Queued::New(next)) => assert_ne!(next, id),
        other => panic!("expected a new run, got {other:?}"),
    }
}

#[test]
fn gate_with_no_declared_command_fails_with_the_cause() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let main = w.main.clone();
    let tip = w.tip_with("nogates", "pr = \"manual\"\n", "x");
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &tip, &main);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed);
    let o = outcome(&ended).unwrap();
    assert!(!o.verdict, "no gate ran");
    assert!(
        o.text.contains("no fast gate") && o.text.contains("[gates] fast"),
        "{}",
        o.text
    );
    assert_eq!(w.gate_result("fast", &tip, &main), None);
}

#[test]
fn gate_command_that_writes_outside_its_paths_is_refused() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let other = w.repo.join(".worktrees").join("other");
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            other.to_str().unwrap(),
            "main",
        ],
    );
    let home = w.dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let in_root = w.fleet_root().join("intruder");
    let in_other = other.join("intruder");
    let in_cargo = home.join(".cargo").join("intruder");
    let command = format!(
        "s=0; echo x > '{}' || s=1; echo x > '{}' || s=1; \
         (mkdir -p '{}' && echo x > '{}') || s=1; exit $s",
        in_root.display(),
        in_other.display(),
        home.join(".cargo").display(),
        in_cargo.display()
    );
    let gates = format!("[gates]\nfast = \"{command}\"\nfull = \"true\"\n");
    let main = w.main.clone();
    let tip = w.tip_with("bad", &gates, "bad");
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &tip, &main);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    for path in [&in_root, &in_other, &in_cargo] {
        assert!(!path.exists(), "the gate wrote {}", path.display());
    }
}

#[test]
fn gate_writes_its_cache_only_in_the_gate_cache() {
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.gate_tree();
    let command = "echo \\\"$CARGO_HOME\\\" > cargo_home.txt && echo \\\"$XDG_CACHE_HOME\\\" > xdg.txt \
                   && mkdir -p \\\"$CARGO_HOME/registry\\\" && echo x > \\\"$CARGO_HOME/registry/f\\\"";
    let gates = format!("[gates]\nfast = \"{command}\"\nfull = \"true\"\n");
    let main = w.main.clone();
    let tip = w.tip_with("cache", &gates, "cache");
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &tip, &main);
    let ended = w.run(id);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let cache = gate_tree::cache_dir(&w.fleet_root(), FEATURE);
    assert!(cache.ends_with("gate-cache/F-1"), "{}", cache.display());
    let tree = w.gate_path();
    let read = |name: &str| std::fs::read_to_string(tree.join(name)).unwrap();
    assert_eq!(
        read("cargo_home.txt").trim(),
        cache.join("cargo").to_str().unwrap()
    );
    assert_eq!(read("xdg.txt").trim(), cache.join("xdg").to_str().unwrap());
    assert!(cache.join("cargo").join("registry").join("f").is_file());
}

#[test]
fn gate_with_no_sandbox_does_not_run_and_holds_the_feature_with_1_item() {
    let mut w = world();
    w.gate_tree();
    let gates = "[gates]\nfast = \"echo ran > ran.txt\"\nfull = \"true\"\n";
    let main = w.main.clone();
    let tip = w.tip_with("held", gates, "held");
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &tip, &main);
    let row = w.job(id);
    for _ in 0..2 {
        let o = w.gate_with(&row, no_sandbox);
        assert!(!o.passed && !o.verdict);
        assert!(o.text.starts_with("no gate sandbox: "), "{}", o.text);
    }
    assert!(
        !w.gate_path().join("ran.txt").exists(),
        "the gate did not run"
    );
    assert_ne!(
        git(&w.gate_path(), &["rev-parse", "HEAD"]),
        tip,
        "no checkout"
    );
    assert_eq!(w.gate_result("fast", &tip, &main), None);

    let key = no_gate_sandbox_key(FEATURE);
    let items: Vec<_> = open_items(&w.conn)
        .unwrap()
        .into_iter()
        .filter(|i| i.key == key)
        .collect();
    assert_eq!(items.len(), 1, "1 needs-you item");
    assert!(exec_gate::held(&w.conn, FEATURE).unwrap());
    let other = w.tip_with("held2", gates, "held2");
    assert_eq!(
        w.queue_gate(Kind::FullGate, FEATURE, &other, &main),
        None,
        "no gate of a held feature is queued"
    );

    // A later pass that finds a sandbox ends the hold.
    let ended = w
        .tx(|ctx, tx| exec_gate::end_hold(ctx, tx, FEATURE))
        .unwrap();
    assert!(ended);
    assert!(!exec_gate::held(&w.conn, FEATURE).unwrap());
    assert!(open_items(&w.conn).unwrap().iter().all(|i| i.key != key));
}

/// A gate result file counts only for the job that fleetd started: a file
/// of a job that does not exist, a file of a job that is not running, and
/// a file whose job id is not the job of its name change no gate run and
/// end no job.
#[test]
fn gate_result_file_with_another_job_id_changes_nothing() {
    let mut w = world();
    w.gate_tree();
    let gates = "[gates]\nfast = \"true\"\nfull = \"true\"\n";
    let main = w.main.clone();
    let tip = w.tip_with("forged", gates, "forged");
    let bundle = w.bundle(&main).to_string();
    let started = w.new_gate(Kind::FastGate, &bundle, &tip, &main);
    let started = w.start(started);
    let other_tip = w.tip_with("forged2", gates, "forged2");
    let queued = w.new_gate(Kind::FullGate, FEATURE, &other_tip, &main);
    let queued = w.job(queued);
    let passed = |job_id: i64, run: i64| JobResult {
        job_id,
        run,
        passed: true,
        verdict: true,
        text: "forged".into(),
        data: None,
    };
    let root = w.fleet_root();
    // No job has this id.
    job_result_file::write(&root, &passed(started.id + 1000, 1)).unwrap();
    // The job exists, but fleetd did not start it.
    job_result_file::write(&root, &passed(queued.id, queued.run)).unwrap();
    // The name is the started job, but the payload names another job.
    let path = job_result_file::path(&root, started.id, started.run);
    let body = serde_json::to_vec(&passed(queued.id, started.run)).unwrap();
    std::fs::write(&path, body).unwrap();

    assert_eq!(w.ingest(), Vec::new());

    assert_eq!(w.gate_result("fast", &tip, &main), None);
    assert_eq!(w.gate_result("full", &other_tip, &main), None);
    assert_eq!(w.job(started.id).state, State::Running);
    assert_eq!(w.job(queued.id).state, queued.state);
}

// ─── the time limit ─────────────────────────────────────────────────────

/// The job process of the time limit test: `horch job run <id>` of the
/// ledger and the run of [`JOB_CHILD`].
fn job_child(value: &str) {
    let (db, id) = value.rsplit_once('|').unwrap();
    let conn = open(Path::new(db));
    let config = FleetConfig::default();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &NoProfiles,
        sweep: None,
    };
    let _ = jobs::run_job(
        &ctx,
        &conn,
        id.parse().unwrap(),
        std::process::id(),
        &checks,
    );
}

#[test]
fn gate_at_its_limit_is_abandoned_and_its_group_killed() {
    if let Ok(value) = std::env::var(JOB_CHILD) {
        job_child(&value);
        return;
    }
    let mut w = world();
    if !has_sandbox() {
        return;
    }
    w.config.job_limit_s.fast_gate = 10;
    w.gate_tree();
    let tree = w.gate_path();
    let pid_file = tree.join("sleep.pid");
    let gates = "[gates]\nfast = \"sleep 600 & echo $! > sleep.pid; wait\"\nfull = \"true\"\n";
    let main = w.main.clone();
    let tip = w.tip_with("slow", gates, "slow");
    let bundle = w.bundle(&main).to_string();
    let id = w.new_gate(Kind::FastGate, &bundle, &tip, &main);

    // Part 3 of a pass starts the job process: this test binary.
    let spec = LaunchSpec {
        program: std::env::current_exe().unwrap(),
        args: vec![
            "--exact".into(),
            "gate_at_its_limit_is_abandoned_and_its_group_killed".into(),
            "--nocapture".into(),
        ],
        cwd: None,
        env_rule: EnvRule {
            vars: vec![(
                JOB_CHILD.into(),
                format!("{}|{id}", w.db().display()).into(),
            )],
        },
        detach: false,
        output: None,
    };
    let job = w.job(id);
    let config = w.config.clone();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut runner = JobRunner::new(&Launcher);
    assert!(runner
        .start(&ctx, &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());
    assert_eq!(w.job(id).deadline, Some(FleetSeconds(10)));

    let until = Instant::now() + Duration::from_secs(60);
    let sleep_pid = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file) {
            if !text.trim().is_empty() {
                break text.trim().to_string();
            }
        }
        assert!(Instant::now() < until, "the gate command started");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(alive(&sleep_pid));

    // At the deadline the pass stops the job process; its group goes too.
    let facts = w.facts();
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(10)))
        .unwrap();
    assert_eq!(seen.stops.len(), 1);
    assert!(jobs::stop(&seen.stops[0]));
    let facts = w.wait_gone(&mut runner, id);
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(11)))
        .unwrap();
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    assert_eq!(w.job(id).state, State::Abandoned);
    assert_eq!(
        w.gate_result("fast", &tip, &main),
        None,
        "an abandoned run has no verdict"
    );

    let until = Instant::now() + Duration::from_secs(10);
    while alive(&sleep_pid) {
        assert!(Instant::now() < until, "the gate group was killed");
        std::thread::sleep(Duration::from_millis(50));
    }
}
