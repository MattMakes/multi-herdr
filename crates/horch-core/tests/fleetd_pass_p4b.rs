//! The pass of Phase 4, part 2: landing, bundles, the ref move, candidate
//! refs, the feature, fetch, push, rebase, the rescue classes and the checks
//! of I8 to I12 in the reconcile pass
//! (docs/tickets/fleetd-p4/47-pass-wiring-integration.md).
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass", "Long work is a
//! job. A pass never waits for it", "A pass has a work budget", "No work is
//! lost", I8 to I12, "1 git writer for each repository", FM-39.
//!
//! Each test makes its own temporary git repository, ledger and fleet root,
//! and runs the passes as library calls with no herdr snapshot. No test
//! starts a `horch` binary: a job process is a script that sleeps, and the
//! test runs the executor of the job in its own process with the pid of the
//! row, as `horch job run <id>` runs it, or writes the result file of the
//! job. No test reads the load, the limits or the clock of this machine:
//! there is no health hold, and the numbers come from a test `FleetConfig`.
#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, JobRunner};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, Ports, Settings, TaskWork, ABORT_AFTER_COMMIT,
    ABORT_BEFORE_COMMIT,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::fleetd::integration::application::rebase::{rebase_key, Rebased};
use horch_core::fleetd::task_execution::application::launch::{Launches, PushLaunch};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed, RunJob};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::finish::{CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

/// The folder of the world of a child process of a crash test.
const CHILD: &str = "HORCH_P4B_CHILD";

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/r1";

// ─── the world ──────────────────────────────────────────────────────────

/// A temp git repository `project` on `main` with a `fleet.toml` of the
/// remote `remote`, the feature branch `fleet/r1`, a ledger at generation 1
/// and 1 run whose run branch is the feature branch.
struct World {
    _temp: Option<tempfile::TempDir>,
    dir: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    tools: Tools,
    sandboxes: WorkerSandboxes,
    /// The repositories that the outside names: each gets its
    /// `worktree_list` job.
    listed: bool,
}

fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn world(remote: &str) -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let mut conn = open(&dir.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    fs::write(project.join("a.txt"), "a\n").unwrap();
    fs::write(
        project.join("fleet.toml"),
        format!("remote = \"{remote}\"\n"),
    )
    .unwrap();
    git(&project, &["add", "a.txt", "fleet.toml"]);
    git(&project, &["commit", "-q", "-m", "a"]);
    git(&project, &["branch", FEATURE_BRANCH]);
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        finish_steps: vec![StepName::Check, StepName::Land],
        ..Default::default()
    };
    let run = runs::create(&conn, &project, &new_run).unwrap();
    // A task needs an idea in flight of the feature of its run (FR-12).
    schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
    let tip = git(&project, &["rev-parse", FEATURE_BRANCH]);
    // The idea of the run wrote the row of the feature branch.
    let tx = conn.transaction().unwrap();
    let seen = BranchFact::SeenTip(&tip);
    rows::set_branch_fact(&tx, None, FEATURE_BRANCH, seen, cause("its tip")).unwrap();
    tx.commit().unwrap();
    fs::create_dir_all(dir.join("fleet")).unwrap();
    drop(conn);
    let mut w = World::reopen(&dir);
    w._temp = Some(temp);
    w.gate_tree();
    w
}

impl World {
    /// The world of the folder `dir` that [`world`] made.
    fn reopen(dir: &Path) -> World {
        let conn = open(&dir.join("ledger.sqlite3"));
        let run_id: String = conn
            .query_row("SELECT run_id FROM runs WHERE slug = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let run = runs::get(&conn, &run_id).unwrap();
        let tools = Tools {
            git: Git::new("git"),
            gh: dir.join("no-gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: dir.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: None,
        };
        let mut sandboxes = WorkerSandboxes::new(CodexSandbox::default(), dir.join("fleet"));
        sandboxes.gap = None;
        World {
            _temp: None,
            dir: dir.to_path_buf(),
            conn,
            run,
            config: FleetConfig::default(),
            tools,
            sandboxes,
            listed: false,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    /// The gate tree of [`FEATURE`], so a fast gate can run (Q18): its
    /// reserved row, the folder that its `worktree_add` job makes, and the
    /// observation of the passed job, as a pass observes it.
    fn gate_tree(&self) {
        let repo = self.repo();
        let tip = self.tip(FEATURE_BRANCH).unwrap();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &self.run.run_id,
            repo: &repo,
            base: FEATURE_BRANCH,
            tip: &tip,
        };
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let job = match gate_tree::reserve(&ctx, &tx, &req, FleetSeconds(0)).unwrap() {
            GateTree::Reserved { job, .. } => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        };
        tx.commit().unwrap();
        let path = gate_tree::path(&repo, FEATURE);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &path.to_string_lossy(),
                &tip,
            ],
        );
        self.conn
            .execute(
                "UPDATE jobs SET state = 'passed', result_json = ?2 WHERE id = ?1",
                params![job, r#"{"passed":true,"verdict":true,"text":"made"}"#],
            )
            .unwrap();
        let ended = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let seen = create::observe(&ctx, &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    fn repo(&self) -> PathBuf {
        self.dir.join("project")
    }

    fn repo_text(&self) -> String {
        self.repo().to_string_lossy().into_owned()
    }

    fn db(&self) -> PathBuf {
        self.dir.join("ledger.sqlite3")
    }

    fn tip(&self, rev: &str) -> Option<String> {
        let out = git_out(&self.repo(), &["rev-parse", "--verify", "-q", rev]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn settings(&self, faults: Faults) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: self.config.clone(),
            horch: self.job_program(),
            faults,
        }
    }

    /// The program of each job process: it sleeps and writes no result
    /// file. The test runs the executor or writes the result file.
    fn job_program(&self) -> PathBuf {
        let path = self.dir.join("job.sh");
        if !path.exists() {
            fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// Commit `file` on the git branch `branch`, made from the feature
    /// branch when it is new. Gives the new tip.
    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
        let repo = self.repo();
        if self.tip(branch).is_some() {
            git(&repo, &["checkout", "-q", branch]);
        } else {
            git(&repo, &["checkout", "-q", "-b", branch, FEATURE_BRANCH]);
        }
        fs::write(repo.join(file), text).unwrap();
        git(&repo, &["add", file]);
        git(&repo, &["commit", "-q", "-m", branch]);
        git(&repo, &["checkout", "-q", "main"]);
        self.tip(branch).unwrap()
    }

    /// A task of `persona` in `status`, its git branch `branch` with 1
    /// commit of its own file, and the row of its task branch in
    /// `building`, as the bind writes it. Gives the task id.
    fn task_branch(&self, branch: &str, persona: &str, status: &str) -> String {
        let n = self.count("SELECT count(*) FROM tasks") + 1;
        let file = format!("f{n}.txt");
        self.commit_on(branch, &file, &format!("{branch}\n"));
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), persona);
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
        let row = NewBranch {
            name: branch,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::insert_branch(&tx, None, &row, cause("the bind")).unwrap();
        tx.commit().unwrap();
        task
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn landing(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn jobs_of(&self, kind: Kind) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([kind.as_str()], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter()
            .map(|id| jobs_store::get(&self.conn, id).unwrap().unwrap())
            .collect()
    }

    /// The 1 run of `kind`.
    fn only_job(&self, kind: Kind) -> JobRow {
        let jobs = self.jobs_of(kind);
        assert_eq!(jobs.len(), 1, "{kind} runs: {jobs:?}");
        jobs.into_iter().next().unwrap()
    }

    /// The executor of the running job `job` runs in this process, as
    /// `horch job run` runs it, and writes its result file.
    fn run_executor(&self, job: &JobRow) {
        assert_eq!(job.state, State::Running, "the pass started {job:?}");
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let mut conn = open(&self.db());
        let ended = test_jobs::TestJobs
            .run_job(&ctx, &mut conn, job.id)
            .unwrap();
        assert_eq!(ended.state, State::Passed, "{ended:?}");
    }

    /// The job process of the running `job` ends with a passed verdict.
    fn pass_job(&self, job: &JobRow) {
        assert_eq!(job.state, State::Running, "the pass started {job:?}");
        let result = JobResult {
            job_id: job.id,
            run: job.run,
            passed: true,
            verdict: true,
            text: "exit 0".into(),
            data: None,
        };
        job_result_file::write(&self.root(), &result).unwrap();
    }

    /// The events of `entity` `id` that moved it to `to`.
    fn moves_to(&self, id: &str, to: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE entity_id = ?1 AND to_state = ?2",
                params![id, to],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The events of the needs-you item `key`.
    fn item_events(&self, key: &str) -> i64 {
        self.moves_to(key, "open")
    }

    fn bundle_state(&self, id: i64) -> String {
        rows::bundle(&self.conn, id).unwrap().unwrap().state
    }
}

/// The outside of the tests: no herdr snapshot and no health hold. With
/// `repo`, it names the repository of the run, so each pass queues its
/// `worktree_list` job.
struct Fake {
    repo: Option<String>,
}

impl Outside for Fake {
    fn panes(&self) -> Option<std::collections::BTreeMap<String, InPane>> {
        None
    }
    fn close_pane(&self, _pane: &str) -> Result<()> {
        Ok(())
    }
    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }
    fn worktree_folders(&self, _conn: &Connection) -> Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }
    fn repositories(&self, _conn: &Connection) -> Result<Vec<String>> {
        Ok(self.repo.iter().cloned().collect())
    }
    fn attempt_spec(&self, a: &AttemptRow) -> Result<LaunchSpec> {
        bail!("attempt {} is not an attempt of this test", a.id)
    }
}

/// The spawn of a push attempt: `true`.
struct Spawns;

impl Launches for Spawns {
    fn spec(&self, _l: &PushLaunch) -> Result<LaunchSpec> {
        Ok(LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        })
    }
}

/// 1 fleetd of the world `w`: a new pass object, as after a restart.
fn fleetd<'a>(w: &'a World, fake: &'a Fake, faults: Faults) -> Pass<'a> {
    let ports = Ports {
        outside: fake,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    Pass::new(w.settings(faults), ports).with_task_work(TaskWork {
        tools: &w.tools,
        sandboxes: &w.sandboxes,
        launches: &Spawns,
    })
}

fn fake(w: &World) -> Fake {
    Fake {
        repo: w.listed.then(|| w.repo_text()),
    }
}

fn run(p: &mut Pass<'_>, conn: &mut Connection) -> PassReport {
    let report = p.run(conn).unwrap();
    assert!(report.heartbeat, "{report:?}");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    report
}

const ABC: [&str; 3] = ["fleet/r1-T1-a", "fleet/r1-T2-b", "fleet/r1-T3-c"];

/// A world whose 3 tasks are done, each with its task branch in
/// `building`.
fn three_done() -> World {
    let w = world("none");
    for b in ABC {
        w.task_branch(b, "dev", "done");
    }
    w
}

/// Run the passes of `w` until the bundle of `ABC` is `gating` and its
/// fast gate job runs, then end the job with a pass. Gives the id of the
/// bundle.
fn to_passed_gate(w: &World, p: &mut Pass<'_>, conn: &mut Connection) -> i64 {
    run(p, conn);
    for b in ABC {
        assert_eq!(w.landing(b), "in_bundle", "{b} followed its task");
    }
    let build = w.only_job(Kind::BundleBuild);
    w.run_executor(&build);
    run(p, conn);
    let id: i64 = build.subject.parse().unwrap();
    assert_eq!(w.bundle_state(id), "gating");
    w.pass_job(&w.only_job(Kind::FastGate));
    id
}

// ─── landing by passes alone ────────────────────────────────────────────

/// 3 task branches of done tasks follow their tasks to `ready`, go in 1
/// bundle, are built, gated 1 time and land on the feature branch by the
/// ref move, by passes alone. The candidate ref of the landed bundle is
/// deleted and the deletion verified.
#[test]
fn ready_branches_of_1_feature_land_by_passes_alone() {
    let w = three_done();
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, Faults::default());

    let id = to_passed_gate(&w, &mut p, &mut conn);
    let candidate_ref = format!("refs/horch/bundle/{id}");
    let candidate = w
        .tip(&candidate_ref)
        .expect("the build wrote the candidate");
    // The pass moves the bundle to `passed` and the ref after its commit.
    run(&mut p, &mut conn);
    assert_eq!(w.tip(FEATURE_BRANCH).as_deref(), Some(candidate.as_str()));
    // The next pass observes the move.
    run(&mut p, &mut conn);
    assert_eq!(w.bundle_state(id), "landed");
    for b in ABC {
        assert_eq!(w.landing(b), "landed", "{b}");
        let tip = w.tip(b).unwrap();
        git(
            &w.repo(),
            &["merge-base", "--is-ancestor", &tip, FEATURE_BRANCH],
        );
    }
    assert_eq!(w.jobs_of(Kind::BundleBuild).len(), 1, "1 build");
    assert_eq!(w.jobs_of(Kind::FastGate).len(), 1, "1 fast gate run");
    // The candidate ref is deleted, then the deletion is verified.
    run(&mut p, &mut conn);
    assert_eq!(w.tip(&candidate_ref), None, "the candidate ref is deleted");
    let bundle = rows::bundle(&w.conn, id).unwrap().unwrap();
    assert_eq!(bundle.candidate_ref, None, "the deletion is verified");
    let seen = rows::branch(&w.conn, FEATURE_BRANCH).unwrap().unwrap();
    assert_eq!(seen.seen_tip.as_deref(), Some(candidate.as_str()));
}

// ─── 1 git writer for each repository ───────────────────────────────────

/// A running fetch, push or rebase of the repository holds a second git
/// writer: the `bundle_build` run of the pass stays `queued` while it
/// runs.
#[test]
fn only_1_git_writer_job_runs_for_1_repository() {
    for kind in [Kind::Fetch, Kind::Push, Kind::Rebase] {
        let w = world("none");
        w.task_branch(ABC[0], "dev", "done");
        let ctx = JobCtx {
            guard: None,
            config: &w.config,
            needs_you: &NeedsYouList,
        };
        let mut conn = open(&w.db());
        let key = JobKey {
            kind,
            subject: "the-writer",
            input_key: "k",
        };
        let repo = w.repo_text();
        let tx = conn.transaction().unwrap();
        let queued = jobs::queue(&ctx, &tx, &key, Some(&repo), FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        let jobs::Queued::New(writer) = queued else {
            panic!("a new {kind} run");
        };
        let row = jobs_store::get(&conn, writer).unwrap().unwrap();
        let spec = jobs::job_spec(&w.job_program(), writer, None);
        let mut runner = JobRunner::new(&Launcher);
        assert!(runner
            .start(&ctx, &mut conn, &row, &spec, FleetSeconds(0))
            .unwrap());

        let f = fake(&w);
        let mut p = fleetd(&w, &f, Faults::default());
        run(&mut p, &mut conn);
        run(&mut p, &mut conn);

        assert_eq!(
            w.only_job(Kind::BundleBuild).state,
            State::Queued,
            "the running {kind} holds the build"
        );
        let running = w.count(&format!(
            "SELECT count(*) FROM jobs WHERE state = 'running' AND repo = '{repo}'"
        ));
        assert_eq!(running, 1, "1 git writer runs while {kind} runs");
        let pid = jobs_store::get(&conn, writer)
            .unwrap()
            .unwrap()
            .pid
            .unwrap();
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

// ─── the checks of I8 to I12 ────────────────────────────────────────────

/// Each breach of I9, I11 and I12 that a pass sees gets 1 event, and the
/// next passes add none:
/// - I9: the feature tip moves under a `passed` bundle: the bundle is
///   `dropped` 1 time, and the new tip is recorded 1 time.
/// - I11: a fleet branch with no row is adopted, reported with 1 needs-you
///   item and not deleted.
/// - I12: a landed branch is deleted, and a landed branch that a stash
///   entry names is kept. The schema trigger refuses a task branch in 2
///   bundles that are not ended, so that breach cannot be written.
#[test]
fn each_invariant_breach_records_1_event() {
    // I9.
    let w = three_done();
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, Faults::default());
    let id = to_passed_gate(&w, &mut p, &mut conn);
    // The bundle is `passed`, and its ref move did not run yet.
    crash(&w, ABORT_AFTER_COMMIT);
    assert_eq!(w.bundle_state(id), "passed");
    let moved = w.commit_on(FEATURE_BRANCH, "pushed-by-you.txt", "x\n");
    for _ in 0..3 {
        run(&mut p, &mut conn);
    }
    assert_eq!(w.bundle_state(id), "dropped");
    assert_eq!(w.moves_to(&id.to_string(), "dropped"), 1, "I9: 1 drop");
    assert_eq!(w.tip(FEATURE_BRANCH).as_deref(), Some(moved.as_str()));
    let seen = rows::branch(&w.conn, FEATURE_BRANCH).unwrap().unwrap();
    assert_eq!(
        seen.seen_tip.as_deref(),
        Some(moved.as_str()),
        "I9: new tip"
    );
    drop(p);

    // I11 and I12.
    let mut w = world("none");
    w.listed = true;
    let orphan = "fleet/r1-T9-orphan";
    w.commit_on(orphan, "o.txt", "o\n");
    let done = "fleet/r1-T1-done";
    let stashed = "fleet/r1-T2-stashed";
    for b in [done, stashed] {
        w.task_branch(b, "dev", "done");
        w.conn
            .execute(
                "UPDATE branches SET landing_state = 'landed' WHERE name = ?1",
                [b],
            )
            .unwrap();
    }
    let repo = w.repo();
    git(&repo, &["checkout", "-q", stashed]);
    fs::write(repo.join("wip.txt"), "wip\n").unwrap();
    git(&repo, &["add", "wip.txt"]);
    git(&repo, &["stash", "push", "-q", "-m", "wip"]);
    git(&repo, &["checkout", "-q", "main"]);
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, Faults::default());
    run(&mut p, &mut conn);
    w.run_executor(&w.only_job(Kind::WorktreeList));
    for _ in 0..3 {
        run(&mut p, &mut conn);
    }

    let orphan_key = format!("orphan_branch:{orphan}");
    assert_eq!(w.item_events(&orphan_key), 1, "I11: 1 item");
    assert!(rows::branch(&w.conn, orphan).unwrap().unwrap().orphan);
    assert!(w.tip(orphan).is_some(), "I11: the orphan is not deleted");
    assert_eq!(w.tip(done), None, "I12: the landed branch is deleted");
    assert!(w.tip(stashed).is_some(), "I12: the stash keeps its branch");
}

// ─── the work budget ────────────────────────────────────────────────────

/// A pass follows at most `pass_budget` task branches. A new fleetd goes
/// on at the cursor `branches` of the last: each branch gets its turn, and
/// none is followed twice before each had 1 turn.
#[test]
fn branch_rows_continue_at_their_cursor() {
    let mut w = world("none");
    w.config.pass_budget = 2;
    let names: Vec<String> = (1..=5).map(|i| format!("fleet/r1-T{i}-x")).collect();
    for b in &names {
        w.task_branch(b, "dev", "done");
    }
    let moved = |w: &World| {
        names
            .iter()
            .filter(|b| w.landing(b) != "building")
            .cloned()
            .collect::<Vec<_>>()
    };
    let f = fake(&w);
    let mut conn = open(&w.db());

    run(&mut fleetd(&w, &f, Faults::default()), &mut conn);
    assert_eq!(moved(&w), names[..2].to_vec());
    // A restart: a new pass object on the same ledger.
    run(&mut fleetd(&w, &f, Faults::default()), &mut conn);
    assert_eq!(moved(&w), names[..4].to_vec());
    run(&mut fleetd(&w, &f, Faults::default()), &mut conn);
    assert_eq!(moved(&w), names.to_vec());
    for kind in ["branches", "bundles", "features"] {
        let rows = w.count(&format!(
            "SELECT count(*) FROM pass_cursors WHERE kind = '{kind}'"
        ));
        assert_eq!(rows, 1, "the cursor {kind}");
    }
}

// ─── FM-39 ──────────────────────────────────────────────────────────────

/// A task is cancelled, and its branch is not abandoned: a crash between
/// the commit of the cancel and the abandon, or a cancel source other than
/// `horch task cancel`. The pass writes the rescue ref of the branch after
/// its commit, then abandons the branch 1 time with the ref on its row.
#[test]
fn pass_abandons_the_branch_of_a_cancelled_task_after_its_rescue() {
    let w = world("none");
    let branch = "fleet/r1-T1-gone";
    w.task_branch(branch, "dev", "cancelled");
    let tip = w.tip(branch).unwrap();
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, Faults::default());

    for _ in 0..3 {
        run(&mut p, &mut conn);
    }

    assert_eq!(w.landing(branch), "abandoned");
    let row = rows::branch(&w.conn, branch).unwrap().unwrap();
    let rescue = row.rescue_ref.expect("the rescue ref is on the row");
    assert_eq!(w.tip(&rescue), Some(tip), "the ref keeps the tip");
    assert_eq!(w.moves_to(branch, "abandoned"), 1, "1 abandon");
}

// ─── the resolver persona ───────────────────────────────────────────────

/// A rebase of the feature branch that conflicts opens a resolver task.
/// The feature branch has no task, so the resolver task takes the primary
/// persona of the newest task of the feature.
#[test]
fn rebase_resolver_task_takes_the_persona_of_the_newest_task_of_the_feature() {
    let w = world("none");
    w.task_branch("fleet/r1-T1-a", "dev", "done");
    w.task_branch("fleet/r1-T2-b", "rev", "done");
    let feature_tip = w.commit_on(FEATURE_BRANCH, "a.txt", "feature\n");
    let repo = w.repo();
    fs::write(repo.join("a.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main moves"]);
    let onto = w.tip("main").unwrap();
    let tx = w.conn.unchecked_transaction().unwrap();
    let seen = BranchFact::SeenTip(&feature_tip);
    rows::set_branch_fact(&tx, None, FEATURE_BRANCH, seen, cause("its tip")).unwrap();
    let ctx = JobCtx {
        guard: None,
        config: &w.config,
        needs_you: &NeedsYouList,
    };
    let key = rebase_key(&feature_tip, &onto);
    let job = JobKey {
        kind: Kind::Rebase,
        subject: FEATURE_BRANCH,
        input_key: &key,
    };
    let jobs::Queued::New(id) =
        jobs::queue(&ctx, &tx, &job, Some(&w.repo_text()), FleetSeconds(0)).unwrap()
    else {
        panic!("a new rebase run");
    };
    let result = serde_json::json!({
        "passed": false,
        "verdict": true,
        "text": "the rebase conflicts",
        "data": Rebased::Conflict { files: vec!["a.txt".into()] },
    });
    tx.execute(
        "UPDATE jobs SET state = 'failed', result_json = ?2, ended_at = 'now' WHERE id = ?1",
        params![id, result.to_string()],
    )
    .unwrap();
    tx.commit().unwrap();
    let f = fake(&w);
    let mut conn = open(&w.db());

    run(&mut fleetd(&w, &f, Faults::default()), &mut conn);

    let personas: Vec<Option<String>> = tasks::of_run(&w.conn, &w.run.run_id)
        .unwrap()
        .into_iter()
        .skip(2)
        .map(|t| t.primary_persona)
        .collect();
    assert_eq!(personas, vec![Some("rev".to_string())], "1 resolver task");
}

// ─── crash points ───────────────────────────────────────────────────────

/// Not a test of its own: 1 pass of a child process of a crash test, with
/// the fault of `HORCH_FAULT`. It runs only with [`CHILD`].
#[test]
fn child_pass() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let w = World::reopen(Path::new(&dir));
    let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, faults);
    let _ = p.run(&mut conn);
    panic!("the fault did not stop the child");
}

fn crash(w: &World, fault: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_pass", "--nocapture", "--test-threads=1"])
        .env(CHILD, &w.dir)
        .env("HORCH_FAULT", fault)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(ABORT_EXIT_CODE), "the fault {fault}");
}

/// The ref move of a `passed` bundle: a crash before the commit of the
/// pass that intends it, then a crash after that commit and before the
/// move. The next pass moves the ref 1 time, the bundle and each member
/// land 1 time, and no tip is lost.
#[test]
fn crash_before_and_after_the_commit_of_the_ref_move_lands_once() {
    let w = three_done();
    let f = fake(&w);
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, &f, Faults::default());
    let id = to_passed_gate(&w, &mut p, &mut conn);
    drop(p);
    let base = w.tip(FEATURE_BRANCH).unwrap();
    let candidate = w.tip(&format!("refs/horch/bundle/{id}")).unwrap();

    crash(&w, ABORT_BEFORE_COMMIT);
    assert_eq!(w.bundle_state(id), "gating", "no row of the crashed pass");
    assert_eq!(w.tip(FEATURE_BRANCH), Some(base.clone()));

    crash(&w, ABORT_AFTER_COMMIT);
    assert_eq!(w.bundle_state(id), "passed", "the intended row");
    assert_eq!(
        w.tip(FEATURE_BRANCH),
        Some(base),
        "no move before the effect"
    );

    let mut p = fleetd(&w, &f, Faults::default());
    run(&mut p, &mut conn);
    run(&mut p, &mut conn);
    assert_eq!(w.tip(FEATURE_BRANCH), Some(candidate));
    assert_eq!(w.bundle_state(id), "landed");
    assert_eq!(w.moves_to(&id.to_string(), "landed"), 1, "1 landing");
    for b in ABC {
        assert_eq!(w.moves_to(b, "landed"), 1, "{b} lands 1 time");
        let tip = w.tip(b).expect("no tip is lost");
        git(
            &w.repo(),
            &["merge-base", "--is-ancestor", &tip, FEATURE_BRANCH],
        );
    }
}
