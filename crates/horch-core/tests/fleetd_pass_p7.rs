//! The pass of Phase 7: the scan of the CLI binaries, the `docs_sync` and
//! `conformance` jobs, the health marks after each ended `conformance`
//! run, the drift idea after a failed run, the `context_limit` outcome and
//! the pacing hold of a pool
//! (docs/tickets/fleetd-p7/30-pass-wiring.md).
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass", "A pass has a work
//! budget", "Holds on dispatch are visible" and "Docs sync and adapter
//! conformance"; FR-8, FM-19, FM-20.
//!
//! Each test makes its own temporary git repository, ledger and fleet root,
//! and runs the passes as library calls with no herdr snapshot. A CLI
//! binary is a temporary file, a job process is a script that sleeps, and
//! the test writes the result file of each job. No test reads the load, the
//! limits, the quota file or the clock of this machine: the fleet clock is
//! a row of the ledger, and the pool readings have a fixed time.
#![cfg(unix)]

use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
use horch_core::fleetd::fleet_runtime::adapters::scans::{CliPath, FolderItem};
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, PoolFacts, Ports, Settings, TaskWork, ABORT_AFTER_COMMIT,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::application::launch::{Launches, PushLaunch};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::docs_dir;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, ConformanceReport, CHECKS,
};
use horch_core::roster::Roster;
use horch_core::routing::quota::{PoolReading, QuotaFile, QuotaView, Window};
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::finish::{CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::supervisor::{AgentFacts, AttemptFiles, OutcomeHint};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, testing, util};
use horch_marketplace::git::scrub_repo_env;
use rusqlite::{params, Connection};

/// The folder of the world of a child process of a crash test.
const CHILD: &str = "HORCH_P7_PASS_CHILD";

/// A teammate of the built-in roster with the agent `claude`.
const CLAUDE_PROFILE: &str = "opus";

/// A teammate of the built-in roster with the agent `antigravity`.
const AGY_PROFILE: &str = "antigravity";

/// The seconds between 2 daily `docs_sync` runs of 1 CLI (MAP.md Q11).
const DAY_S: i64 = 86_400;

// ─── the world ──────────────────────────────────────────────────────────

/// A temp git repository `project` on `main`, a ledger at generation 1, a
/// fleet root, 1 run, and a folder `bin` of fake CLI binaries.
struct World {
    _temp: Option<tempfile::TempDir>,
    dir: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    tools: Tools,
    sandboxes: WorkerSandboxes,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    let out = cmd
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
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
    git(&project, &["add", "a.txt"]);
    git(&project, &["commit", "-q", "-m", "a"]);
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        finish_steps: vec![StepName::Check, StepName::Land],
        ..Default::default()
    };
    let run = runs::create(&conn, &project, &new_run).unwrap();
    // A task needs an idea in flight of the feature of its run (FR-12).
    schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
    fs::create_dir_all(dir.join("fleet")).unwrap();
    fs::create_dir_all(dir.join("bin")).unwrap();
    drop(conn);
    let mut w = World::reopen(&dir);
    w._temp = Some(temp);
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
        // Each persona has a check sandbox: the host facts do not decide a
        // test.
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
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn project(&self) -> PathBuf {
        self.dir.join("project")
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
    /// file. The test writes the result file. Its binary id is the id of
    /// the horch binary of the pass.
    fn job_program(&self) -> PathBuf {
        let path = self.dir.join("job.sh");
        if !path.exists() {
            fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// The fake binary of the CLI `cli`, with the content `version`: a new
    /// content is a new binary id.
    fn install_cli(&self, cli: &str, version: &str) {
        fs::write(self.dir.join("bin").join(cli), version).unwrap();
    }

    /// Each fake CLI binary in `bin`, by name.
    fn cli_paths(&self) -> Vec<CliPath> {
        let mut out: Vec<CliPath> = fs::read_dir(self.dir.join("bin"))
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                CliPath {
                    cli: path.file_name().unwrap().to_string_lossy().into_owned(),
                    path,
                }
            })
            .collect();
        out.sort_by(|a, b| a.cli.cmp(&b.cli));
        out
    }

    fn set_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Each run of `kind`: id, subject, run number and state, oldest first.
    fn jobs(&self, kind: &str) -> Vec<(i64, String, i64, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, subject, run, state FROM jobs WHERE kind = ?1 ORDER BY id")
            .unwrap();
        stmt.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The first word of the subject of each run of `kind`.
    fn job_clis(&self, kind: &str) -> Vec<String> {
        self.jobs(kind)
            .into_iter()
            .map(|(_, s, _, _)| s.split_whitespace().next().unwrap().to_string())
            .collect()
    }

    /// The last run of `kind` of the CLI `cli`: id and run number.
    fn last_job(&self, kind: &str, cli: &str) -> (i64, i64) {
        self.jobs(kind)
            .into_iter()
            .rev()
            .find(|(_, s, _, _)| s.split_whitespace().next() == Some(cli))
            .map(|(id, _, run, _)| (id, run))
            .unwrap_or_else(|| panic!("no {kind} run of {cli}"))
    }

    /// The `docs_sync` run `id` ends with the result `passed`.
    fn end_docs_sync(&self, id: i64, run: i64) {
        let result = JobResult {
            job_id: id,
            run,
            passed: true,
            verdict: true,
            text: "docs sync: 3 pages".into(),
            data: None,
        };
        job_result_file::write(&self.root(), &result).unwrap();
    }

    /// The `conformance` run `id` of `report` ends: its `conformance.json`
    /// in the docs folder, `CHANGES.md` of its CLI, and its result file.
    fn end_conformance(&self, id: i64, run: i64, report: &ConformanceReport) {
        let docs = docs_dir::docs_root(&self.root());
        let path = docs_dir::write_conformance(&docs, report).unwrap();
        fs::write(
            docs.join(&report.cli).join("CHANGES.md"),
            "# changes\n\n- removed: --mcp-config takes a list\n",
        )
        .unwrap();
        let failed: Vec<&str> = report.failed().into_iter().map(Check::name).collect();
        let result = JobResult {
            job_id: id,
            run,
            passed: failed.is_empty(),
            verdict: true,
            text: format!("conformance of {} {}", report.cli, report.version),
            data: Some(serde_json::json!({
                "cli": report.cli,
                "version": report.version,
                "adapter_hash": report.adapter_hash,
                "path": path,
                "failed": failed,
            })),
        };
        job_result_file::write(&self.root(), &result).unwrap();
    }

    /// The health events of `profile`, oldest first.
    fn health_events(&self, profile: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kind FROM events WHERE kind IN ('profile_unhealthy', 'profile_healthy')
                 AND entity_id = ?1 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([profile], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The id of each idea, in order.
    fn ideas(&self) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM ideas ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The ideas of the drift of an adapter.
    fn drift_ideas(&self) -> Vec<String> {
        self.ideas()
            .into_iter()
            .filter(|id| {
                self.count(&format!(
                    "SELECT COUNT(*) FROM events WHERE entity_id = '{id}' AND kind = 'added'
                     AND data_json LIKE '%Update the claude adapter for 2.1.0%'"
                )) > 0
            })
            .collect()
    }

    /// 1 ready task of the shared checkout of `persona`.
    fn ready_task(&self, title: &str, persona: &str) -> String {
        let mut t = NewTask::work(title, &format!("{title}.md"), persona);
        t.worktree_mode = WorktreeMode::Shared;
        t.land_check = Some("true".into());
        tasks::add(&self.conn, &self.run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn status(&self, task: &str) -> String {
        self.conn
            .query_row("SELECT status FROM tasks WHERE task_id = ?1", [task], |r| {
                r.get(0)
            })
            .unwrap()
    }

    /// The hold events of the pool `pool`, oldest first.
    fn pool_hold_events(&self, pool: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kind FROM events WHERE kind IN ('hold_started', 'hold_ended')
                 AND json_extract(data_json, '$.pool') = ?1 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([pool], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
}

/// A report of `cli` 2.1.0: `Shared write` fails when `failed`, and each
/// other check passes.
fn report(cli: &str, failed: bool) -> ConformanceReport {
    let checks = CHECKS
        .into_iter()
        .map(|c| {
            if failed && c == Check::SharedWrite {
                CheckResult::fail(c, "result.json was not written")
            } else {
                CheckResult::pass(c, "seen")
            }
        })
        .collect();
    ConformanceReport {
        cli: cli.into(),
        version: "2.1.0".into(),
        adapter_hash: "h1".into(),
        checks,
    }
}

/// The outside of the tests: no herdr snapshot and no health hold; the
/// fake CLI binaries of the world; the built-in roster and 1 reading of
/// the claude pool at a fixed time, with `used` of its 7-day window.
struct Fake {
    clis: Vec<CliPath>,
    project: PathBuf,
    used: Cell<f64>,
}

impl Fake {
    fn of(w: &World) -> Fake {
        Fake {
            clis: w.cli_paths(),
            project: w.project(),
            used: Cell::new(0.1),
        }
    }
}

/// A view of the claude pool with 1 window of 7 days that is half elapsed
/// and `used`. 0.63 of it at half the window is a pace above 1.25.
fn half_elapsed(used: f64) -> QuotaView {
    let now = horch_core::clock::parse("2026-09-28T18:00:00Z").unwrap();
    let mut file = QuotaFile::default();
    file.pools.insert(
        "claude".into(),
        PoolReading {
            observed_at: Some("2026-09-28T17:55:00Z".into()),
            windows: vec![Window::new(
                10_080,
                used,
                Some("2026-10-02T06:00:00Z".into()),
                None,
            )],
            ..Default::default()
        },
    );
    QuotaView::new(file, now, Default::default(), false)
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
        Ok(vec![self.project.to_string_lossy().into_owned()])
    }
    fn attempt_spec(&self, a: &AttemptRow) -> Result<LaunchSpec> {
        bail!("attempt {} is not a push attempt of this test", a.id)
    }
    fn pools(&self) -> Option<PoolFacts> {
        Some(PoolFacts {
            roster: Roster::builtin().unwrap(),
            quota: half_elapsed(self.used.get()),
        })
    }
    fn cli_binaries(&self) -> Vec<CliPath> {
        self.clis.clone()
    }
}

/// The spawn of a push attempt: 1 line with the attempt id in the
/// launches file.
struct Spawns {
    file: PathBuf,
}

impl Launches for Spawns {
    fn spec(&self, l: &PushLaunch) -> Result<LaunchSpec> {
        Ok(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!("echo {} >> '{}'", l.attempt, self.file.display()).into(),
            ],
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        })
    }
}

fn spawns(w: &World) -> Spawns {
    Spawns {
        file: w.dir.join("launches.txt"),
    }
}

fn pass<'a>(w: &'a World, fake: &'a Fake, spawns: &'a Spawns, faults: Faults) -> Pass<'a> {
    let ports = Ports {
        outside: fake,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    Pass::new(w.settings(faults), ports).with_task_work(TaskWork {
        tools: &w.tools,
        sandboxes: &w.sandboxes,
        launches: spawns,
    })
}

/// 1 pass of `w` with `fake`, on a new connection.
fn run_pass(w: &World, fake: &Fake) -> PassReport {
    let sp = spawns(w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let report = pass(w, fake, &sp, Faults::default())
        .run(&mut conn)
        .unwrap();
    assert!(report.heartbeat, "{report:?}");
    report
}

/// Not a test of its own: 1 pass of a child process of a crash test, with
/// the fault of `HORCH_FAULT`. It runs only with [`CHILD`].
#[test]
fn child_pass() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let w = World::reopen(Path::new(&dir));
    let fake = Fake::of(&w);
    let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let _ = pass(&w, &fake, &sp, faults).run(&mut conn);
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

// ─── the docs jobs ──────────────────────────────────────────────────────

/// A new CLI binary queues `docs_sync` and `conformance` with no
/// repository, and part 3 starts each through the launcher. 1 day later on
/// the fleet clock, the pass queues a new `docs_sync` run with the next run
/// number.
#[test]
fn pass_queues_the_daily_docs_sync() {
    let w = world();
    w.install_cli("claude", "2.1.0");
    let fake = Fake::of(&w);

    run_pass(&w, &fake);
    let docs = w.jobs("docs_sync");
    assert_eq!(docs.len(), 1, "{docs:?}");
    assert_eq!(docs[0].3, "running", "part 3 started the job");
    assert_eq!(w.job_clis("conformance"), ["claude"]);
    assert_eq!(
        w.count("SELECT COUNT(*) FROM jobs WHERE kind IN ('docs_sync', 'conformance') AND repo IS NOT NULL"),
        0,
        "no repository"
    );

    let (docs_id, docs_run) = w.last_job("docs_sync", "claude");
    w.end_docs_sync(docs_id, docs_run);
    let (conf_id, conf_run) = w.last_job("conformance", "claude");
    w.end_conformance(conf_id, conf_run, &report("claude", false));
    run_pass(&w, &fake);
    assert_eq!(w.jobs("docs_sync").len(), 1, "no new run within the day");

    w.set_clock(DAY_S + 1);
    run_pass(&w, &fake);
    let docs = w.jobs("docs_sync");
    assert_eq!(docs.len(), 2, "a daily run is a new row: {docs:?}");
    assert_eq!(docs[1].1, docs[0].1, "the same subject");
    assert_eq!(docs[1].2, 2, "the next run number");
}

/// A docs source with no exec adapter (herdr) gets its `docs_sync` runs
/// and no `conformance` run, and an ended run queues no new one.
#[test]
fn docs_source_with_no_exec_adapter_gets_no_conformance_run() {
    let w = world();
    w.install_cli("herdr", "0.9.0");
    let fake = Fake::of(&w);

    run_pass(&w, &fake);
    assert_eq!(w.job_clis("docs_sync"), ["herdr"]);
    let (id, run) = w.last_job("docs_sync", "herdr");
    w.end_docs_sync(id, run);
    run_pass(&w, &fake);
    run_pass(&w, &fake);
    assert_eq!(w.jobs("docs_sync").len(), 1, "no loop of docs_sync runs");
    assert!(
        w.jobs("conformance").is_empty(),
        "{:?}",
        w.jobs("conformance")
    );
}

/// The scan of the CLI binaries reads at most `pass_budget` binaries in 1
/// pass, and the next pass continues at its cursor in `pass_cursors`.
#[test]
fn scan_of_cli_binaries_has_a_cursor() {
    let mut w = world();
    w.config.pass_budget = 1;
    for cli in ["agy", "claude", "codex"] {
        w.install_cli(cli, "1.0.0");
    }
    let fake = Fake::of(&w);

    run_pass(&w, &fake);
    assert_eq!(w.job_clis("docs_sync"), ["agy"]);
    assert_eq!(
        w.count("SELECT COUNT(*) FROM pass_cursors WHERE kind = 'cli_binaries'"),
        1
    );
    run_pass(&w, &fake);
    assert_eq!(w.job_clis("docs_sync"), ["agy", "claude"]);
    run_pass(&w, &fake);
    assert_eq!(w.job_clis("docs_sync"), ["agy", "claude", "codex"]);
}

// ─── the health of a profile and the drift idea ─────────────────────────

/// A failed `conformance` run marks each profile of its CLI unhealthy and
/// adds 1 drift idea; a passed run marks the profile healthy again.
#[test]
fn pass_marks_unhealthy_after_a_failed_conformance_run() {
    let w = world();
    w.install_cli("claude", "2.1.0");
    let fake = Fake::of(&w);
    run_pass(&w, &fake);

    let (id, run) = w.last_job("conformance", "claude");
    w.end_conformance(id, run, &report("claude", true));
    run_pass(&w, &fake);
    assert_eq!(w.health_events(CLAUDE_PROFILE), ["profile_unhealthy"]);
    assert_eq!(w.drift_ideas().len(), 1);
    assert!(w.health_events(AGY_PROFILE).is_empty(), "another CLI");

    // The docs sync of the next day ends, and its conformance run passes.
    let (docs_id, docs_run) = w.last_job("docs_sync", "claude");
    w.end_docs_sync(docs_id, docs_run);
    run_pass(&w, &fake);
    w.set_clock(DAY_S + 1);
    run_pass(&w, &fake);
    let (docs_id, docs_run) = w.last_job("docs_sync", "claude");
    assert_eq!(docs_run, 2, "the daily docs sync");
    w.end_docs_sync(docs_id, docs_run);
    run_pass(&w, &fake);
    let (id, run) = w.last_job("conformance", "claude");
    assert_eq!(run, 2, "a conformance run after the docs sync");
    w.end_conformance(id, run, &report("claude", false));
    run_pass(&w, &fake);
    assert_eq!(
        w.health_events(CLAUDE_PROFILE),
        ["profile_unhealthy", "profile_healthy"]
    );
    run_pass(&w, &fake);
    assert_eq!(w.health_events(CLAUDE_PROFILE).len(), 2, "1 mark per run");
    assert_eq!(w.drift_ideas().len(), 1);
}

/// The docs source `agy` is the CLI of the harness `antigravity`: its
/// failed run marks each profile with the agent `antigravity`.
#[test]
fn failed_agy_conformance_marks_each_antigravity_profile() {
    let w = world();
    w.install_cli("agy", "2.1.0");
    let fake = Fake::of(&w);
    run_pass(&w, &fake);

    let (id, run) = w.last_job("conformance", "agy");
    w.end_conformance(id, run, &report("agy", true));
    run_pass(&w, &fake);
    assert_eq!(w.health_events(AGY_PROFILE), ["profile_unhealthy"]);
    assert!(w.health_events(CLAUDE_PROFILE).is_empty(), "another CLI");
}

/// A crash after the commit of the marks and the idea row, before part 3
/// writes idea.md: the next pass writes idea.md 1 time, and adds no second
/// mark and no second idea.
#[test]
fn pass_adds_1_drift_idea_after_a_crash_and_a_restart() {
    let w = world();
    w.install_cli("claude", "2.1.0");
    let fake = Fake::of(&w);
    run_pass(&w, &fake);
    let (id, run) = w.last_job("conformance", "claude");
    w.end_conformance(id, run, &report("claude", true));

    crash(&w, ABORT_AFTER_COMMIT);
    let ideas = w.drift_ideas();
    assert_eq!(ideas.len(), 1, "the idea row is committed");
    let idea_md = w.root().join("ideas").join(&ideas[0]).join("idea.md");
    assert!(!idea_md.exists(), "part 3 did not run");

    run_pass(&w, &fake);
    run_pass(&w, &fake);
    assert_eq!(w.drift_ideas(), ideas, "no second idea");
    assert!(idea_md.exists(), "the next pass wrote idea.md");
    assert_eq!(w.health_events(CLAUDE_PROFILE), ["profile_unhealthy"]);
    assert_eq!(
        w.count(
            "SELECT COUNT(*) FROM needs_you WHERE kind = 'adapter-conformance-failed' AND state = 'open'"
        ),
        1
    );
}

// ─── context_limit ──────────────────────────────────────────────────────

/// The wrapper ended the attempt at the context limit: the pass ends it
/// with `context_limit`, burns no attempt and moves the task to `ready`
/// at once, with a handoff file for the next attempt.
#[test]
fn pass_moves_a_context_limit_task_to_ready() {
    let w = world();
    let task = w.ready_task("t1", "agy");
    let worker = NewWorker {
        run_id: w.run.run_id.clone(),
        kind: Some(WorkerKind::Worker),
        persona: "agy".into(),
        ..Default::default()
    };
    let bound = testing::bind_live(&w.conn, &task, &worker, None).unwrap();
    let a = AttemptRef {
        task_id: task.clone(),
        attempt_id: bound.attempt_id.clone(),
    };
    let files = AttemptFiles::new(&w.root(), a.clone());
    fs::create_dir_all(files.dir()).unwrap();
    // The wrapper makes its attempt folder with mode 0700.
    fs::set_permissions(files.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    files
        .write_exit_hinted(Some(0), false, Some(OutcomeHint::ContextLimit))
        .unwrap();
    let fake = Fake::of(&w);

    run_pass(&w, &fake);
    let (state, outcome): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&a.attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("ended", Some("context_limit"))
    );
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_ne!(t.status, "running", "the attempt ended");
    let floor: i64 = w
        .conn
        .query_row(
            "SELECT attempt_floor FROM tasks WHERE task_id = ?1",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(floor, 1, "no attempt is burned");
    let handoff: Option<String> = w
        .conn
        .query_row(
            "SELECT handoff_path FROM workers WHERE worker_id = ?1",
            [&bound.worker_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        handoff.is_some_and(|p| Path::new(&p).exists()),
        "a handoff file"
    );
}

#[test]
fn context_limit_in_the_pass_of_the_first_heartbeat_is_not_no_result() {
    let w = world();
    let task = w.ready_task("t1", "agy");
    let fake = Fake::of(&w);
    run_pass(&w, &fake);
    let attempt: String = w
        .conn
        .query_row(
            "SELECT id FROM attempts WHERE task_id = ?1 AND state = 'launching' AND via = 'push'",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    let files = AttemptFiles::new(
        &w.root(),
        AttemptRef {
            task_id: task.clone(),
            attempt_id: attempt.clone(),
        },
    );
    fs::create_dir_all(files.dir()).unwrap();
    fs::set_permissions(files.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    files.write_heartbeat(&AgentFacts::new(None)).unwrap();
    files
        .write_exit_hinted(None, false, Some(OutcomeHint::ContextLimit))
        .unwrap();

    let report = run_pass(&w, &fake);
    let (state, outcome): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("ended", Some("context_limit")),
        "{report:?}"
    );
    let t = tasks::get(&w.conn, &task).unwrap();
    let open = w.count(&format!(
        "SELECT COUNT(*) FROM attempts WHERE task_id = '{task}' AND state <> 'ended'"
    ));
    assert_eq!(t.attempts - t.attempt_floor, open, "no attempt is burned");
    assert_eq!(t.not_before, None, "a context limit sets no backoff");
}

#[test]
fn attempt_with_no_recorded_process_waits_for_its_exit_json() {
    let w = world();
    let task = w.ready_task("t1", "agy");
    let fake = Fake::of(&w);
    run_pass(&w, &fake);
    let attempt: String = w
        .conn
        .query_row(
            "SELECT id FROM attempts WHERE task_id = ?1 AND state = 'launching' AND via = 'push'",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    let files = AttemptFiles::new(
        &w.root(),
        AttemptRef {
            task_id: task.clone(),
            attempt_id: attempt.clone(),
        },
    );
    fs::create_dir_all(files.dir()).unwrap();
    fs::set_permissions(files.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    files.write_heartbeat(&AgentFacts::new(None)).unwrap();
    let state = |w: &World| -> (String, Option<String>) {
        w.conn
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [&attempt],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    };

    let report = run_pass(&w, &fake);
    assert_eq!(state(&w), ("live".into(), None), "{report:?}");
    files
        .write_exit_hinted(None, false, Some(OutcomeHint::ContextLimit))
        .unwrap();
    let report = run_pass(&w, &fake);
    assert_eq!(
        state(&w),
        ("ended".into(), Some("context_limit".into())),
        "{report:?}"
    );
}

#[test]
fn result_in_the_pass_of_the_first_heartbeat_is_not_no_result() {
    let w = world();
    let task = w.ready_task("t1", CLAUDE_PROFILE);
    let fake = Fake::of(&w);
    run_pass(&w, &fake);
    let attempt: String = w
        .conn
        .query_row(
            "SELECT id FROM attempts WHERE task_id = ?1 AND state = 'launching' AND via = 'push'",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    let files = AttemptFiles::new(
        &w.root(),
        AttemptRef {
            task_id: task.clone(),
            attempt_id: attempt.clone(),
        },
    );
    let outbox = files.dir().join("outbox");
    fs::create_dir_all(&outbox).unwrap();
    for d in [files.dir(), outbox.as_path()] {
        fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let result = ResultFile {
        task: task.clone(),
        attempt: "A1".into(),
        status: ResultStatus::Completed,
        summary: "done".into(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
    };
    let path = outbox.join("result.json");
    fs::write(&path, result.to_json()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    files.write_heartbeat(&AgentFacts::new(None)).unwrap();
    files.write_exit_hinted(Some(0), false, None).unwrap();

    let report = run_pass(&w, &fake);
    let events: Vec<String> = w
        .conn
        .prepare("SELECT kind || ' ' || text FROM events ORDER BY event_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        w.status(&task),
        "verifying",
        "{report:?}\n{}",
        events.join("\n")
    );
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_eq!(t.not_before, None, "no backoff of rung 6");
}

#[test]
fn quota_outcome_restarts_as_a_push_attempt_of_the_fallback() {
    let w = world();
    let task = w.ready_task("t1", CLAUDE_PROFILE);
    let worker = NewWorker {
        run_id: w.run.run_id.clone(),
        kind: Some(WorkerKind::Worker),
        persona: CLAUDE_PROFILE.into(),
        ..Default::default()
    };
    let bound = testing::bind_live(&w.conn, &task, &worker, None).unwrap();
    let before = tasks::get(&w.conn, &task).unwrap();
    let files = AttemptFiles::new(
        &w.root(),
        AttemptRef {
            task_id: task.clone(),
            attempt_id: bound.attempt_id.clone(),
        },
    );
    fs::create_dir_all(files.dir()).unwrap();
    fs::set_permissions(files.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    files
        .write_exit_hinted(Some(1), false, Some(OutcomeHint::Quota))
        .unwrap();
    let fake = Fake::of(&w);
    fake.used.set(1.0);

    let report = run_pass(&w, &fake);
    let mut stmt = w
        .conn
        .prepare(
            "SELECT a.id, a.state, a.outcome, a.via, w.persona FROM attempts a
             JOIN workers w ON w.worker_id = a.worker_id WHERE a.task_id = ?1 ORDER BY a.rowid",
        )
        .unwrap();
    let rows: Vec<(String, String, Option<String>, String, Option<String>)> = stmt
        .query_map([&task], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 2, "A1 and 1 restart attempt: {rows:?}");
    assert_eq!(rows[0].0, bound.attempt_id);
    assert_eq!(rows[0].2.as_deref(), Some("quota"), "{rows:?}");
    let (id, state, _, via, persona) = &rows[1];
    assert_eq!(id, &format!("{task}/A2"), "{rows:?}");
    assert_eq!(
        (state.as_str(), via.as_str(), persona.as_deref()),
        ("launching", "push", Some("codex-sol")),
        "the restart is a push attempt of the fallback: {report:?}"
    );
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_eq!(t.status, "leased");
    assert_eq!(t.not_before, None, "a quota outcome sets no backoff");
    assert_eq!(
        t.attempts - t.attempt_floor,
        before.attempts - before.attempt_floor,
        "no attempt is burned"
    );
    let claim_rows = w.count(&format!(
        "SELECT COUNT(*) FROM attempts WHERE task_id = '{task}' AND via = 'claim'
         AND id <> '{}'",
        bound.attempt_id
    ));
    assert_eq!(claim_rows, 0, "the old claim path writes no row");
}

// ─── pacing ─────────────────────────────────────────────────────────────

/// A paced pool holds the ready tasks of its personas in a pass, with 1
/// `hold_started` event, and the hold ends when the pacing ends: the task
/// is bound, with 1 `hold_ended` event.
#[test]
fn paced_pool_holds_its_ready_tasks_until_the_pacing_ends() {
    let w = world();
    let task = w.ready_task("t1", CLAUDE_PROFILE);
    let fake = Fake::of(&w);
    fake.used.set(0.63);

    run_pass(&w, &fake);
    run_pass(&w, &fake);
    assert_eq!(w.status(&task), "ready", "the pacing holds it");
    assert_eq!(w.pool_hold_events("claude"), ["hold_started"]);

    fake.used.set(0.1);
    run_pass(&w, &fake);
    assert_ne!(w.status(&task), "ready", "dispatch resumes");
    assert_eq!(w.pool_hold_events("claude"), ["hold_started", "hold_ended"]);
}
