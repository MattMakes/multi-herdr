//! The pass of Phase 6, part 2: the trial merges, the stacks and their
//! edges, a merge with ideas in flight and the feature ready rule in the
//! integration part of the reconcile pass
//! (docs/tickets/fleetd-p6/52-pass-integration.md).
//!
//! `docs/specs/fleetd/design.md`, "Between features", "Features" (A merge
//! with ideas in flight), "The reconcile pass", "Long work is a job. A pass
//! never waits for it", RR-4, RR-7 and PL-5.
//!
//! Each test makes its own temporary git repository, ledger and fleet root,
//! and runs the passes as library calls with no herdr snapshot. No test
//! starts a `horch` binary: a job process is a script that sleeps, and the
//! test runs the executor of the job in its own process with the pid of the
//! row, as `horch job run <id>` runs it. No test reads the load, the limits
//! or the clock of this machine: there is no health hold, and the numbers
//! come from a test `FleetConfig`.
#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, Ports, Settings, TaskWork, ABORT_AFTER_COMMIT,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::fleetd::task_execution::application::launch::{Launches, PushLaunch};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::application::create::RunJob;
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::finish::{CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

/// The folder of the world of a child process of a crash test.
const CHILD: &str = "HORCH_P6B_CHILD";

// ─── the world ──────────────────────────────────────────────────────────

/// A temp git repository `project` on `main` with a `fleet.toml` of no
/// remote, a ledger at generation 1 and a fleet root. Each feature is 1
/// run with its idea in flight, as the release backfill writes it.
struct World {
    _temp: Option<tempfile::TempDir>,
    dir: PathBuf,
    conn: Connection,
    config: FleetConfig,
    tools: Tools,
    sandboxes: WorkerSandboxes,
}

/// 1 feature of the world: its run, its id and its branch.
struct Feature {
    run: Run,
    id: String,
    branch: String,
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

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = horch_platform::path::canonicalize(temp.path()).unwrap();
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
    fs::write(project.join("fleet.toml"), "remote = \"none\"\n").unwrap();
    git(&project, &["add", "a.txt", "fleet.toml"]);
    git(&project, &["commit", "-q", "-m", "a"]);
    fs::create_dir_all(dir.join("fleet")).unwrap();
    drop(conn);
    let mut w = World::reopen(&dir);
    w._temp = Some(temp);
    w
}

impl World {
    /// The world of the folder `dir` that [`world`] made.
    fn reopen(dir: &Path) -> World {
        let conn = open(&dir.join("ledger.sqlite3"));
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
        let mut sandboxes = WorkerSandboxes::on_path(
            CodexSandbox::default(),
            dir.join("fleet"),
            std::env::var_os("PATH"),
        );
        sandboxes.gap = None;
        World {
            _temp: None,
            dir: dir.to_path_buf(),
            conn,
            config: FleetConfig::default(),
            tools,
            sandboxes,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn repo(&self) -> PathBuf {
        self.dir.join("project")
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
    /// file. The test runs the executor.
    fn job_program(&self) -> PathBuf {
        let path = self.dir.join("job.sh");
        if !path.exists() {
            fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// The feature of the new run `slug`: its branch `fleet/<slug>` from
    /// `main`, its idea in flight, and the row of its branch at its tip.
    fn feature(&self, slug: &str) -> Feature {
        let branch = format!("fleet/{slug}");
        git(&self.repo(), &["branch", &branch, "main"]);
        let new_run = NewRun {
            slug: slug.into(),
            base_branch: Some("main".into()),
            run_branch: Some(branch.clone()),
            finish_steps: vec![StepName::Check, StepName::Land],
            ..Default::default()
        };
        let run = runs::create(&self.conn, &self.repo(), &new_run).unwrap();
        // A task needs an idea in flight of the feature of its run (FR-12).
        schema::run_idea(&self.conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("the idea of the run");
        let id: String = self
            .conn
            .query_row(
                "SELECT id FROM features WHERE branch = ?1",
                [&branch],
                |r| r.get(0),
            )
            .unwrap();
        self.seen(&branch);
        Feature { run, id, branch }
    }

    /// The row of the branch `branch` holds its tip in git.
    fn seen(&self, branch: &str) {
        let tip = self.tip(branch).unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let fact = BranchFact::SeenTip(&tip);
        rows::set_branch_fact(&tx, None, branch, fact, cause("its tip")).unwrap();
        tx.commit().unwrap();
    }

    /// Commit `file` on the git branch `branch`, made from `from` when it
    /// is new. Gives the new tip.
    fn commit_on(&self, branch: &str, from: &str, file: &str, text: &str) -> String {
        let repo = self.repo();
        if self.tip(branch).is_some() {
            git(&repo, &["checkout", "-q", branch]);
        } else {
            git(&repo, &["checkout", "-q", "-b", branch, from]);
        }
        fs::write(repo.join(file), text).unwrap();
        git(&repo, &["add", file]);
        git(&repo, &["commit", "-q", "-m", branch]);
        git(&repo, &["checkout", "-q", "main"]);
        self.tip(branch).unwrap()
    }

    /// A task of the feature `f` in `status`, its git branch `branch` from
    /// the feature branch with 1 commit of `file`, and the row of its task
    /// branch in `landing`. Gives the task id.
    fn task_branch(
        &self,
        f: &Feature,
        branch: &str,
        file: &str,
        status: &str,
        landing: &str,
    ) -> String {
        self.commit_on(branch, &f.branch, file, &format!("{branch}\n"));
        let task = self.task(f, status);
        let row = NewBranch {
            name: branch,
            feature_id: &f.id,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::insert_branch(&tx, None, &row, cause("the bind")).unwrap();
        tx.commit().unwrap();
        self.conn
            .execute(
                "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
                params![branch, landing],
            )
            .unwrap();
        task
    }

    /// A task of the feature `f` in `status`, of the persona `dev`, with
    /// no branch. Gives the task id.
    fn task(&self, f: &Feature, status: &str) -> String {
        let n = self.count("SELECT count(*) FROM tasks") + 1;
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let task = tasks::add(&self.conn, &f.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
        task
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn text(&self, sql: &str, arg: &str) -> Option<String> {
        self.conn.query_row(sql, [arg], |r| r.get(0)).ok().flatten()
    }

    fn feature_state(&self, id: &str) -> String {
        self.text("SELECT state FROM features WHERE id = ?1", id)
            .unwrap()
    }

    fn feature_of_idea(&self, idea: &str) -> String {
        self.text("SELECT feature_id FROM ideas WHERE id = ?1", idea)
            .unwrap()
    }

    fn feature_of_branch(&self, branch: &str) -> String {
        self.text("SELECT feature_id FROM branches WHERE name = ?1", branch)
            .unwrap()
    }

    fn stacked_on(&self, id: &str) -> Option<String> {
        self.text("SELECT stacked_on FROM features WHERE id = ?1", id)
    }

    /// The runs of `kind`, by id.
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

    /// The executor of each running run of `kind` runs in this process, as
    /// `horch job run` runs it, and ends the run.
    fn run_executors(&self, kind: Kind) -> usize {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let mut conn = open(&self.db());
        let mut ran = 0;
        for job in self.jobs_of(kind) {
            if job.state != State::Running {
                continue;
            }
            let ended = test_jobs::TestJobs
                .run_job(&ctx, &mut conn, job.id)
                .unwrap();
            assert!(
                matches!(ended.state, State::Passed | State::Failed),
                "{ended:?}"
            );
            ran += 1;
        }
        ran
    }

    /// The id of the first event after `after` of the entity `id` whose
    /// `to_state` is `to`, or whose kind is `note` when `to` is `None`.
    fn first_event(&self, after: i64, id: &str, to: Option<&str>) -> Option<i64> {
        let sql = "SELECT min(event_id) FROM events WHERE event_id > ?1 AND entity_id = ?2
                   AND ((?3 IS NULL AND kind = 'note') OR to_state = ?3)";
        self.conn
            .query_row(sql, params![after, id, to], |r| r.get(0))
            .unwrap()
    }

    fn last_event(&self) -> i64 {
        self.count("SELECT coalesce(max(event_id), 0) FROM events")
    }

    /// Each rescue ref, as (ref, tip).
    fn rescue_refs(&self) -> Vec<(String, String)> {
        let out = git(
            &self.repo(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/horch/rescue/",
            ],
        );
        out.lines()
            .map(|l| {
                let (name, tip) = l.split_once(' ').unwrap();
                (name.to_string(), tip.to_string())
            })
            .collect()
    }

    /// You merge the feature branch `branch` into `main` with a squash
    /// merge.
    fn squash_merge(&self, branch: &str) {
        let repo = self.repo();
        git(&repo, &["merge", "-q", "--squash", branch]);
        git(&repo, &["commit", "-q", "-m", "squash"]);
    }
}

/// The outside of the tests: no herdr snapshot and no health hold.
struct Fake;

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
fn fleetd<'a>(w: &'a World, faults: Faults) -> Pass<'a> {
    let ports = Ports {
        outside: &Fake,
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

fn run(p: &mut Pass<'_>, conn: &mut Connection) -> PassReport {
    let report = p.run(conn).unwrap();
    assert!(report.heartbeat, "{report:?}");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    report
}

/// The feature `r1` with 1 landed task branch, whose commit is on the
/// feature branch, and 1 task branch in `building` of the same idea in
/// flight, whose task waits for you (`escalated`), so no pass dispatches
/// it. Gives the feature and the branch in `building`.
fn feature_with_idea_in_flight(w: &World) -> (Feature, String) {
    let f = w.feature("r1");
    let landed = "fleet/r1-T1";
    w.task_branch(&f, landed, "one.txt", "done", "landed");
    git(&w.repo(), &["branch", "-f", &f.branch, landed]);
    w.seen(&f.branch);
    let building = "fleet/r1-T2".to_string();
    w.task_branch(&f, &building, "two.txt", "escalated", "building");
    (f, building)
}

// ─── the tests ──────────────────────────────────────────────────────────

/// 1 pass runs the integration duties of Phase 6 in the order of ticket
/// 52: a merge with ideas in flight, then the trial merges to queue, then
/// the stacks, then the stack edges. The edge of a feature on a feature
/// that this pass records as merged ends in the same pass, and a trial
/// merge verdict that this pass records stacks its younger feature in the
/// same pass.
#[test]
fn pass_runs_the_integration_duties_of_phase_6_in_order() {
    let w = world();
    let mut conn = open(&w.db());
    let (one, _) = feature_with_idea_in_flight(&w);
    let two = w.feature("r2");
    w.conn
        .execute(
            "UPDATE features SET stacked_on = ?2 WHERE id = ?1",
            params![two.id, one.id],
        )
        .unwrap();
    let three = w.feature("r3");
    w.commit_on(&three.branch, "main", "c.txt", "three\n");
    w.seen(&three.branch);
    let four = w.feature("r4");
    w.commit_on(&four.branch, "main", "c.txt", "four\n");
    w.seen(&four.branch);
    // The resolver task of a stack, or of a stack edge, takes the persona
    // of a task of the upper feature.
    w.task(&two, "done");
    w.task(&four, "done");

    // Pass 1 queues the trial merges and starts them. It never waits for
    // them.
    let mut p = fleetd(&w, Faults::default());
    run(&mut p, &mut conn);
    let first = w.jobs_of(Kind::TrialMerge);
    assert!(
        first
            .iter()
            .any(|j| j.subject.contains(&three.id) && j.subject.contains(&four.id)),
        "the pair that shares c.txt gets its trial merge: {first:?}"
    );
    assert!(
        first
            .iter()
            .all(|j| j.state != State::Passed && j.state != State::Failed),
        "the pass does not wait for a trial merge: {first:?}"
    );
    assert_eq!(w.stacked_on(&four.id), None);
    assert!(w.run_executors(Kind::TrialMerge) > 0);

    // You merge r1. The upper feature r2 is on the merged primary tip.
    w.squash_merge(&one.branch);
    git(&w.repo(), &["branch", "-f", &two.branch, "main"]);
    w.seen(&two.branch);
    let mark = w.last_event();
    let jobs_before = w.jobs_of(Kind::TrialMerge).last().map_or(0, |j| j.id);
    run(&mut p, &mut conn);

    let merged = w
        .first_event(mark, &one.id, Some("merged"))
        .expect("the merge with ideas in flight");
    assert_ne!(w.feature_of_idea("I-0001"), one.id, "the idea moved");
    let queued = w
        .jobs_of(Kind::TrialMerge)
        .into_iter()
        .find(|j| j.id > jobs_before)
        .expect("the new primary tip queues new trial merges");
    let queued = w
        .first_event(mark, &queued.id.to_string(), Some("queued"))
        .expect("the event of the queued trial merge");
    let stacked = w
        .first_event(mark, &four.id, None)
        .expect("the stack of the younger feature");
    assert_eq!(w.stacked_on(&four.id), Some(three.id.clone()));
    let edge = w
        .first_event(mark, &two.id, None)
        .expect("the end of the stack edge");
    assert_eq!(w.stacked_on(&two.id), None);
    assert!(
        merged < queued && queued < stacked && stacked < edge,
        "merged {merged}, queued {queued}, stacked {stacked}, edge {edge}"
    );
}

/// A feature whose pull request you merged while 1 of its ideas is in
/// flight: the pass moves the idea to a new feature before the feature
/// ready rule records the feature as merged, so no idea is left without a
/// feature, and the task branch of the idea moves to the new feature with
/// its rescue ref and 1 rebase job.
#[test]
fn merge_with_ideas_in_flight_is_handled_before_ready() {
    let w = world();
    let mut conn = open(&w.db());
    let (one, building) = feature_with_idea_in_flight(&w);
    let tip = w.tip(&building).unwrap();
    w.squash_merge(&one.branch);

    let mut p = fleetd(&w, Faults::default());
    run(&mut p, &mut conn);
    run(&mut p, &mut conn);

    assert_eq!(w.feature_state(&one.id), "merged");
    let new = w.feature_of_idea("I-0001");
    assert_ne!(new, one.id, "the idea in flight moved to a new feature");
    assert_eq!(w.feature_state(&new), "open");
    let new_branch = w
        .text("SELECT branch FROM features WHERE id = ?1", &new)
        .unwrap();
    assert_eq!(w.tip(&new_branch), w.tip("main"), "the new branch ref");
    assert_eq!(w.feature_of_branch(&building), new);
    let rescues = w.rescue_refs();
    assert!(
        rescues.iter().any(|(_, t)| *t == tip),
        "the rescue ref of {building}: {rescues:?}"
    );
    let rebases = w.jobs_of(Kind::Rebase);
    assert_eq!(
        rebases.iter().filter(|j| j.subject == building).count(),
        1,
        "{rebases:?}"
    );
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
    let mut conn = open(&w.db());
    let mut p = fleetd(&w, faults);
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

/// A crash after the commit of the pass that records a merge with ideas
/// in flight, before its ref effects: the next fleetd writes the branch
/// ref of the new feature, the rescue ref of the moved task branch and its
/// rebase job, each 1 time, and no tip is lost.
#[test]
fn crash_after_a_commit_completes_each_ref_effect_once() {
    let w = world();
    let (one, building) = feature_with_idea_in_flight(&w);
    let tip = w.tip(&building).unwrap();
    w.squash_merge(&one.branch);

    crash(&w, ABORT_AFTER_COMMIT);
    let new = w.feature_of_idea("I-0001");
    assert_ne!(new, one.id, "the committed rows of the crashed pass");
    let new_branch = w
        .text("SELECT branch FROM features WHERE id = ?1", &new)
        .unwrap();
    assert_eq!(w.tip(&new_branch), None, "no ref before the effect");
    assert!(
        w.rescue_refs().is_empty(),
        "no rescue ref before the effect"
    );

    let mut conn = open(&w.db());
    let mut p = fleetd(&w, Faults::default());
    run(&mut p, &mut conn);
    run(&mut p, &mut conn);
    let mut p = fleetd(&w, Faults::default());
    run(&mut p, &mut conn);

    assert_eq!(w.tip(&new_branch), w.tip("main"), "the new branch ref");
    let rescues = w.rescue_refs();
    assert_eq!(rescues.len(), 1, "1 rescue ref: {rescues:?}");
    assert_eq!(rescues[0].1, tip, "the rescue ref holds the tip");
    assert_eq!(w.tip(&building), Some(tip), "no tip is lost");
    assert_eq!(w.feature_of_branch(&building), new);
    let rebases = w.jobs_of(Kind::Rebase);
    assert_eq!(
        rebases.iter().filter(|j| j.subject == building).count(),
        1,
        "{rebases:?}"
    );
}
