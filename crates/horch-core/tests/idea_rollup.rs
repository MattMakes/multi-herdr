#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{
    FolderFact, Liveness, Presence, ProcessFact,
};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobRunner, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::adapters::ideas_store;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::rollup::{close_idea, roll_up_idea, roll_up_ideas};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::{
    worktree_list, CliWorktreeGit, ListResult,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::disk::{decision, disk_budget};
use horch_core::fleetd::worktree_custody::application::reconcile::{
    reconcile_worktrees, Facts, Listed, Report, WorkDir,
};
use horch_core::fleetd::worktree_custody::domain::disk::GB;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, TransactionBehavior};

const CLOCK: FleetSeconds = FleetSeconds(1_000);

struct World {
    _temp: tempfile::TempDir,
    top: PathBuf,
    conn: Connection,
    config: FleetConfig,
    repo: PathBuf,
    runs: u32,
}

fn world() -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
    let temp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(temp.path()).unwrap();
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
        conn,
        config: FleetConfig::default(),
        repo,
        runs: 0,
    }
}

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

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

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

impl World {
    fn sql(&self, sql: &str, args: &[&str]) {
        self.conn
            .execute(sql, rusqlite::params_from_iter(args))
            .unwrap();
    }

    fn run(&mut self, status: &str) -> Run {
        self.runs += 1;
        let new = NewRun {
            slug: format!("r{}", self.runs),
            base_branch: Some("main".into()),
            run_branch: Some(format!("fleet/r{}", self.runs)),
            ..Default::default()
        };
        let run = runs::create(&self.conn, &self.repo, &new).unwrap();
        horch_core::taskledger::schema::run_idea(
            &self.conn,
            &run.run_id,
            &horch_core::taskledger::util::now(),
            true,
        )
        .unwrap();
        self.sql(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            &[&run.run_id, status],
        );
        run
    }

    fn task(&self, run: &Run, name: &str) -> String {
        let t = NewTask::work(name, &format!("{name}.md"), "opus");
        tasks::add(&self.conn, run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn idea_of(&self, task: &str) -> String {
        self.conn
            .query_row(
                "SELECT idea_id FROM tasks WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn idea_state(&self, idea: &str) -> String {
        ideas_store::idea(&self.conn, idea).unwrap().unwrap().state
    }

    fn set_idea_state(&self, idea: &str, state: &str) {
        self.sql("UPDATE ideas SET state = ?2 WHERE id = ?1", &[idea, state]);
    }

    fn set_task_status(&self, task: &str, status: &str) {
        self.sql(
            "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
            &[task, status],
        );
    }

    fn set_feature_state(&self, idea: &str, state: &str) {
        self.sql(
            "UPDATE features SET state = ?2
              WHERE id = (SELECT feature_id FROM ideas WHERE id = ?1)",
            &[idea, state],
        );
    }

    fn branch(&self, name: &str, task: &str, landing_state: &str) {
        let idea = self.idea_of(task);
        self.sql(
            "INSERT INTO branches (name, feature_id, task_id, landing_state, owner, due_at)
             SELECT ?1, feature_id, ?2, ?3, 'fleetd', 0 FROM ideas WHERE id = ?4",
            &[&format!("fleet/{name}"), task, landing_state, &idea],
        );
    }

    fn set_landing_state(&self, name: &str, landing_state: &str) {
        self.sql(
            "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
            &[&format!("fleet/{name}"), landing_state],
        );
    }

    fn row(&self, name: &str, task: Option<&str>, state: &str) {
        let path = self.repo.join(".worktrees").join(name);
        self.insert_row(name, task, &path, None);
        self.set_state(name, state);
    }

    fn insert_row(&self, name: &str, task: Option<&str>, path: &Path, base: Option<String>) {
        let new = NewWorktree {
            worktree_id: name.to_string(),
            task_id: task.map(str::to_string),
            worker_id: None,
            repo_root: self.repo.display().to_string(),
            path: path.display().to_string(),
            branch: format!("fleet/{name}"),
            base: "main".to_string(),
            base_commit: base,
            status: WorktreeStatus::Open,
            mode: None,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let cause = Cause {
            actor: Actor::Horch,
            text: "a test",
        };
        trees::insert_orphan(&tx, None, &new, cause).unwrap();
        tx.commit().unwrap();
    }

    fn orphan(&mut self, name: &str, task: &str) -> PathBuf {
        let path = self.repo.join(".worktrees").join(name);
        let branch = format!("fleet/{name}");
        let path_s = path.display().to_string();
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", &branch, &path_s, "main"],
        );
        std::fs::write(path.join("u.txt"), "untracked\n").unwrap();
        let base = Some(git(&self.repo, &["rev-parse", "main"]));
        self.insert_row(name, Some(task), &path, base);
        self.set_state(name, "orphaned");
        path
    }

    fn set_state(&self, name: &str, state: &str) {
        self.sql(
            "UPDATE worktrees SET state = ?2 WHERE worktree_id = ?1",
            &[name, state],
        );
    }

    fn set_gb(&self, name: &str, gb: u64) {
        self.conn
            .execute(
                "UPDATE worktrees SET bytes_on_disk = ?2 WHERE worktree_id = ?1",
                params![name, i64::try_from(gb * GB).unwrap()],
            )
            .unwrap();
    }

    fn tree(&self, name: &str) -> WorktreeRow {
        trees::get(&self.conn, name).unwrap().unwrap()
    }

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
        let gw = CliWorktreeGit::new("git", self.top.join("fleet"))
            .reading(std::slice::from_ref(&self.repo));
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

    fn pass(&mut self) -> Report {
        let facts = self.facts();
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let report = reconcile_worktrees(&ctx(&config), &tx, &facts, CLOCK).unwrap();
        disk_budget(&ctx(&config), &tx, CLOCK).unwrap();
        tx.commit().unwrap();
        report
    }

    fn roll_up(&mut self, idea: &str) -> bool {
        let config = self.config.clone();
        let tx = self.conn.unchecked_transaction().unwrap();
        let moved = roll_up_idea(&ctx(&config), &tx, idea).unwrap();
        tx.commit().unwrap();
        moved
    }

    fn close(&mut self, idea: &str) -> bool {
        let config = self.config.clone();
        let tx = self.conn.unchecked_transaction().unwrap();
        let moved = close_idea(&ctx(&config), &tx, idea).unwrap();
        tx.commit().unwrap();
        moved
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn jobs_of(&self, kind: Kind, name: &str) -> Vec<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 ORDER BY id")
            .unwrap();
        stmt.query_map(params![kind.as_str(), name], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn run_job(&mut self, id: i64) -> JobRow {
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
        let pid = self.job(id).pid.unwrap();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let written = jobs::run_job(&ctx(&config), &self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx(&config), &mut self.conn).unwrap();
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
}

fn text(job: &JobRow) -> String {
    outcome(job).map(|o| o.text).unwrap_or_default()
}

#[test]
fn cancelled_task_does_not_block_its_idea() {
    let mut w = world();
    let run = w.run("active");
    let done = w.task(&run, "done");
    let cancelled = w.task(&run, "cancelled");
    let idea = w.idea_of(&done);
    assert_eq!(w.idea_of(&cancelled), idea);
    assert_eq!(w.idea_state(&idea), "in_progress");
    w.set_task_status(&done, "done");
    w.branch("done", &done, "landed");
    w.set_task_status(&cancelled, "cancelled");
    w.branch("cancelled", &cancelled, "building");

    assert!(!w.roll_up(&idea));
    assert_eq!(w.idea_state(&idea), "in_progress");

    w.set_landing_state("cancelled", "abandoned");
    assert!(w.roll_up(&idea));
    assert_eq!(w.idea_state(&idea), "integrating");
    assert!(!w.roll_up(&idea));
    assert_eq!(w.idea_state(&idea), "integrating");
}

#[test]
fn idea_is_integrating_when_each_branch_is_landed_or_abandoned() {
    let mut w = world();
    let run = w.run("active");
    let t1 = w.task(&run, "t1");
    let t2 = w.task(&run, "t2");
    let idea = w.idea_of(&t1);
    w.branch("t1", &t1, "landed");
    w.branch("t2", &t2, "verifying");
    w.set_task_status(&t1, "done");

    assert!(!w.roll_up(&idea));
    assert_eq!(w.idea_state(&idea), "in_progress");

    w.set_task_status(&t2, "done");
    for state in [
        "verifying",
        "ready",
        "in_bundle",
        "conflicted",
        "gate_failed",
    ] {
        w.set_landing_state("t2", state);
        assert!(!w.roll_up(&idea), "{state}");
        assert_eq!(w.idea_state(&idea), "in_progress", "{state}");
    }

    w.branch("t1-old", &t1, "abandoned");
    w.set_landing_state("t2", "landed");
    assert!(w.roll_up(&idea));
    assert_eq!(w.idea_state(&idea), "integrating");

    let other = w.run("active");
    let t3 = w.task(&other, "t3");
    let empty = w.idea_of(&t3);
    w.sql("DELETE FROM tasks WHERE task_id = ?1", &[&t3]);
    assert!(!w.roll_up(&empty));
    assert_eq!(w.idea_state(&empty), "in_progress");
}

#[test]
fn idea_is_merged_only_when_each_tree_is_removed_or_rescued() {
    let mut w = world();
    let run = w.run("active");
    let t1 = w.task(&run, "t1");
    let t2 = w.task(&run, "t2");
    let t3 = w.task(&run, "t3");
    let idea = w.idea_of(&t1);
    for t in [&t1, &t2, &t3] {
        w.set_task_status(t, "done");
    }
    assert!(w.roll_up(&idea));
    w.row("gone", Some(&t1), "removed");
    w.row("saved", Some(&t2), "rescued");
    w.row("live", Some(&t3), "landed");
    let other = w.run("active");
    let t4 = w.task(&other, "t4");
    w.row("other", Some(&t4), "active");

    assert!(!w.close(&idea));
    assert_eq!(w.idea_state(&idea), "integrating");

    w.set_feature_state(&idea, "merged");
    for state in [
        "reserved", "created", "active", "landing", "landed", "orphaned",
    ] {
        w.set_state("live", state);
        assert!(!w.close(&idea), "{state}");
        assert_eq!(w.idea_state(&idea), "integrating", "{state}");
    }

    w.set_state("live", "removed");
    assert!(w.close(&idea));
    assert_eq!(w.idea_state(&idea), "merged");
    assert!(!w.close(&idea));

    let t5 = w.task(&other, "t5");
    let in_flight = w.idea_of(&t5);
    w.set_feature_state(&in_flight, "merged");
    w.set_state("other", "removed");
    assert!(!w.close(&in_flight));
    assert_eq!(w.idea_state(&in_flight), "in_progress");
}

#[test]
fn rescued_trees_of_a_merged_idea_are_removed_and_refs_stay() {
    let mut w = world();
    let run = w.run("active");
    let task = w.task(&run, "t1");
    let idea = w.idea_of(&task);
    let path = w.orphan("kept", &task);

    w.pass();
    let rescue = w.jobs_of(Kind::Rescue, "kept");
    assert_eq!(rescue.len(), 1);
    let ended = w.run_job(rescue[0]);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();
    assert_eq!(w.tree("kept").state, WorktreeState::Rescued);
    let refs = w.rescue_refs();
    assert!(refs.starts_with("refs/horch/rescue/"), "{refs}");

    let report = w.pass();
    assert!(report.removals.is_empty(), "{report:?}");
    assert!(w.jobs_of(Kind::WorktreeRemove, "kept").is_empty());

    w.set_task_status(&task, "done");
    w.set_feature_state(&idea, "merged");
    let config = w.config.clone();
    let tx = w.conn.unchecked_transaction().unwrap();
    let rolled = roll_up_ideas(&ctx(&config), &tx).unwrap();
    tx.commit().unwrap();
    assert_eq!(rolled.integrating, std::slice::from_ref(&idea));
    assert_eq!(rolled.merged, std::slice::from_ref(&idea));
    assert_eq!(w.idea_state(&idea), "merged");

    let report = w.pass();
    assert_eq!(report.removals, ["kept"], "{report:?}");
    let report = w.pass();
    assert!(report.removals.is_empty(), "{report:?}");
    let removal = w.jobs_of(Kind::WorktreeRemove, "kept");
    assert_eq!(removal.len(), 1);

    let ended = w.run_job(removal[0]);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    w.pass();
    assert_eq!(w.tree("kept").state, WorktreeState::Removed);
    assert!(!path.exists(), "the folder is gone");
    assert_eq!(w.rescue_refs(), refs, "the rescue ref stays");
    let report = w.pass();
    assert!(report.removals.is_empty(), "{report:?}");
}

#[test]
fn disk_budget_uses_the_idea_state_not_the_run() {
    let mut w = world();
    let done_run = w.run("done");
    let in_flight = w.task(&done_run, "in-flight");
    let active_run = w.run("active");
    let merged = w.task(&active_run, "merged");
    let cancelled_run = w.run("active");
    let cancelled = w.task(&cancelled_run, "cancelled");
    w.set_idea_state(&w.idea_of(&in_flight), "in_progress");
    w.set_idea_state(&w.idea_of(&merged), "merged");
    w.set_idea_state(&w.idea_of(&cancelled), "cancelled");

    w.row("a-in-flight", Some(&in_flight), "landed");
    w.row("b-merged", Some(&merged), "landed");
    w.row("c-cancelled", Some(&cancelled), "rescued");
    w.row("d-no-task", None, "rescued");
    for name in ["a-in-flight", "b-merged", "c-cancelled", "d-no-task"] {
        w.set_gb(name, 10);
    }

    let d = decision(&w.conn, 1).unwrap();
    assert_eq!(d.total, 40 * GB);
    assert_eq!(d.remove, ["b-merged", "c-cancelled"]);
    assert!(
        d.hold,
        "the tree of the idea in flight and the tree with no task stay"
    );
}
