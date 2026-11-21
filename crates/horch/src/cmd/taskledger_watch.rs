//! `horch ledger watch`: fleetd does the watch of each run
//! (`docs/specs/fleetd/design.md`, "Migration plan" item 3;
//! `docs/tickets/fleetd-p1/MAP.md`, Q11).
//!
//! - `horch ledger watch --once [--force] [--json]` runs 1 pass of fleetd
//!   in the foreground when no fleetd holds the fleetd lock, and prints the
//!   checks of the run as before Phase 1. When a fleetd holds the lock, it
//!   runs no pass, stops nothing, and says so in 1 line.
//! - `horch ledger watch` with no `--once` says that fleetd does the watch,
//!   and runs the check of `horch fleetd check`.

use std::process::ExitCode;

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::DutyEffects;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::land;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::watchdog;
use horch_core::taskledger::{LedgerWriter, Store};

use super::fleetd::{self, Once};
use super::taskledger_wake;
use crate::output;

/// The line of `horch ledger watch` with no `--once`.
const FLEETD_WATCHES: &str = "fleetd does the watch of each run: it checks the workers, reaps the dead and wakes the orchestrator.";

/// The effects of the watchdog duties of the pass of fleetd on this machine
/// (ticket 22): the delivery of the wakes and the cleanup retry. `horch
/// fleetd` (ticket 23) gives them to `WatchdogDuties`, with
/// `HerdrObserver`.
pub struct LiveEffects<'a> {
    pub ctx: &'a RuntimeContext,
    /// The writer of fleetd: these effects run in the fleetd process.
    pub writer: &'a LedgerWriter,
}

impl LiveEffects<'_> {
    fn store(&self) -> Result<Store> {
        let ledger = &self.ctx.paths.ledger_db;
        anyhow::ensure!(ledger.is_file(), "the task ledger is gone");
        self.writer.open(ledger)
    }
}

impl DutyEffects for LiveEffects<'_> {
    fn deliver(&self, run: &Run) {
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return eprintln!("horch fleetd: the wake delivery failed: {e:#}"),
        };
        // 1 push budget for the wakes of this pass (#67, #177).
        let _budget = horch_core::messaging::delivery::start_push_budget(
            horch_core::messaging::delivery::done_push_budget(&self.ctx.settings.faults),
        );
        taskledger_wake::drain_run(self.ctx, &store, run, None);
    }

    fn retry_cleanups(&self) -> Result<Vec<String>> {
        let git = Git::new(&self.ctx.bins.harness.git);
        Ok(land::retry_cleanup(&mut self.store()?, &git)?
            .into_iter()
            .map(|(path, o)| format!("cleanup of {path}: {}", o.note))
            .collect())
    }
}

/// `horch ledger watch`.
pub fn watch(
    ctx: &RuntimeContext,
    run: &Run,
    once: bool,
    force: bool,
    as_json: bool,
) -> Result<ExitCode> {
    if once {
        output::println(&once_report(ctx, run, force, as_json)?);
        return Ok(ExitCode::SUCCESS);
    }
    output::println(FLEETD_WATCHES);
    // `HORCH_NO_WATCHDOG=1`: no check and no start (MAP.md, Q17).
    if !ctx.settings.no_watchdog {
        output::println(&fleetd::describe(&fleetd::run_check(ctx)?));
    }
    Ok(ExitCode::SUCCESS)
}

/// The report of `watch --once` for `run`: the checks of 1 pass of fleetd,
/// or the line that a fleetd holds the lock.
fn once_report(ctx: &RuntimeContext, run: &Run, force: bool, as_json: bool) -> Result<String> {
    let checked = match fleetd::watch_once(ctx, force)? {
        Once::Ran(checked) => checked,
        Once::Held(row) => {
            let who = row.map_or("A fleetd".to_string(), |f| format!("fleetd PID {}", f.pid));
            return Ok(format!(
                "{who} holds the fleetd lock and does the checks: horch ledger watch --once runs no pass."
            ));
        }
    };
    let checked: Vec<watchdog::Checked> = checked
        .into_iter()
        .filter(|(r, _)| *r == run.run_id)
        .map(|(_, c)| c)
        .collect();
    report(&checked, as_json)
}

/// The checks of a pass: JSON, or 1 line per checked worker.
fn report(checked: &[watchdog::Checked], as_json: bool) -> Result<String> {
    if as_json {
        return Ok(serde_json::to_string_pretty(checked)?);
    }
    if checked.is_empty() {
        return Ok("No worker was due for a check.".to_string());
    }
    let lines: Vec<String> = checked
        .iter()
        .map(|c| {
            format!(
                "{} {}: {} (round {}): {}",
                c.role.as_deref().unwrap_or(&c.worker_id),
                c.task.as_deref().unwrap_or("-"),
                c.verdict,
                c.round,
                c.note
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::HerdrObserver as Live;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::schema::run_idea;
    use horch_core::taskledger::util;
    #[cfg(unix)]
    use horch_core::taskledger::watchdog::{Observation, Observer};
    use horch_core::taskledger::workers::{self, NewWorker, Worker};
    use horch_core::taskledger::Store;
    #[cfg(unix)]
    use horch_core::workspace::herdr::Herdr;
    use std::path::Path;
    #[cfg(unix)]
    use std::time::{Duration, UNIX_EPOCH};

    /// A worker row of a new run in `dir`. The tests set the fields `observe`
    /// reads and leave the others as the ledger made them.
    fn worker(dir: &Path) -> Worker {
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir, &new_run)?;
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
        let new_worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let id = store
            .immediate(|tx| workers::insert(tx, &new_worker))
            .unwrap();
        workers::find(store.conn(), &id).unwrap().unwrap()
    }

    /// A herdr that answers by pane: `p1` has a status, a change count and a
    /// screen; `p2` has a screen but no status; `p3` has a status but no
    /// screen; any other pane fails both.
    #[cfg(unix)]
    fn herdr(dir: &Path) -> Herdr {
        let bin = dir.join("herdr");
        // macOS scans a new executable at its first exec: warm it.
        let warm = crate::test_support::write_warmed(
            &bin,
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'pane get p1') echo '{\"result\":{\"pane\":{\"pane_id\":\"p1\",\"agent_status\":\"working\",\"state_change_seq\":7}}}' ;;\n\
             'pane read p1 --source visible') echo 'the visible text' ;;\n\
             'pane read p2 --source visible') echo 'only the screen' ;;\n\
             'pane get p3') echo '{\"result\":{\"pane\":{\"pane_id\":\"p3\",\"agent_status\":\"idle\",\"state_change_seq\":9}}}' ;;\n\
             warm*) ;;\n\
             *) exit 1 ;;\n\
             esac\n",
            &["warm"],
        );
        assert!(warm.status.success());
        Herdr::with_bin(bin)
    }

    #[cfg(unix)]
    #[test]
    fn the_agent_is_alive_when_its_pid_runs_with_the_recorded_start_time() {
        let dir = tempfile::tempdir().unwrap();
        let live = Live {
            herdr: herdr(dir.path()),
        };
        let mut w = worker(dir.path());
        let me = std::process::id();
        let start = horch_core::procid::start_time(me).unwrap() as i64;
        let alive = |w: &Worker| live.observe(w).agent_alive;

        (w.agent_pid, w.agent_started) = (Some(me as i64), Some(start));
        assert!(alive(&w), "the pid and its start time match");
        w.agent_started = Some(start + 1);
        assert!(!alive(&w), "the pid runs, but it started at another time");
        // An old record has no start time: the pid alone counts.
        w.agent_started = None;
        assert!(alive(&w), "the pid runs");
        w.agent_pid = Some(2_000_000_000);
        assert!(!alive(&w), "no process has this pid");
        (w.agent_pid, w.agent_started) = (None, Some(start));
        assert!(!alive(&w), "a start time without a pid");
        w.agent_started = None;
        assert!(!alive(&w), "no pid");
    }

    #[cfg(unix)]
    #[test]
    fn the_pane_and_the_screen_are_read_apart_and_only_for_a_worker_with_a_pane() {
        let dir = tempfile::tempdir().unwrap();
        let live = Live {
            herdr: herdr(dir.path()),
        };
        let mut w = worker(dir.path());
        let on = |w: &mut Worker, pane: Option<&str>| {
            w.pane_id = pane.map(str::to_string);
            live.observe(w)
        };

        assert_eq!(
            on(&mut w, Some("p1")),
            Observation {
                herdr_status: Some("working".into()),
                herdr_seq: Some(7),
                screen: Some("the visible text\n".into()),
                ..Default::default()
            }
        );
        assert_eq!(
            on(&mut w, Some("p2")),
            Observation {
                screen: Some("only the screen\n".into()),
                ..Default::default()
            },
            "a pane that cannot be read as a pane still has its screen"
        );
        assert_eq!(
            on(&mut w, Some("p3")),
            Observation {
                herdr_status: Some("idle".into()),
                herdr_seq: Some(9),
                ..Default::default()
            },
            "a pane whose screen cannot be read still has its status"
        );
        assert_eq!(on(&mut w, Some("p4")), Observation::default());
        assert_eq!(on(&mut w, None), Observation::default());
    }

    #[cfg(unix)]
    #[test]
    fn the_transcript_gives_its_size_and_the_time_it_was_written() {
        let dir = tempfile::tempdir().unwrap();
        let live = Live {
            herdr: herdr(dir.path()),
        };
        let mut w = worker(dir.path());
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, "12345").unwrap();
        let written = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(&transcript)
            .unwrap()
            .set_modified(written)
            .unwrap();

        w.transcript_path = Some(transcript.to_string_lossy().into_owned());
        assert_eq!(
            live.observe(&w),
            Observation {
                transcript_bytes: Some(5),
                transcript_mtime: Some("2023-11-14T22:13:20Z".into()),
                ..Default::default()
            }
        );
        w.transcript_path = Some(
            dir.path()
                .join("missing.jsonl")
                .to_string_lossy()
                .into_owned(),
        );
        assert_eq!(live.observe(&w), Observation::default());
        w.transcript_path = None;
        assert_eq!(live.observe(&w), Observation::default());
    }

    /// A fleet in `dir` for `horch ledger watch --once`: a ledger with 1 run
    /// and 1 working worker, and a herdr that is not there. Returns the context and the run.
    fn once_fleet(dir: &Path) -> (horch_core::runtime::RuntimeContext, runs::Run, String) {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        use horch_core::taskledger::model::Actor;
        use horch_core::taskledger::tasks::{self, NewTask};

        let db = dir.join("ledger.sqlite3");
        let env = MapEnv::new(dir)
            .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
            .with("HORCH_DATA_DIR", dir.join("data").to_str().unwrap())
            .with("HORCH_LEDGER_DB", db.to_str().unwrap())
            .with("HORCH_HERDR_BIN", dir.join("no-herdr").to_str().unwrap());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let mut store = Store::open(&db).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let (run, worker) = store
            .immediate(|tx| {
                let run = runs::create(tx, dir, &new_run)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                // No git repository: the task works in the shared checkout.
                let mut a = NewTask::work("a", "a.md", "dev");
                a.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &a, Actor::Orchestrator)?.task_id;
                let new_worker = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "dev".into(),
                    ..Default::default()
                };
                // This test is the live agent of the worker. fleetd binds
                // the task to a live attempt that owns it (FR-5), and the
                // fleet clock of 1 pass stays within the stall_after_s of
                // the bind, so no pass stops it.
                let me = std::process::id();
                let started = horch_core::procid::start_time(me);
                let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
                    pid: me,
                    pid_start: started,
                };
                let w = horch_core::taskledger::testing::bind_live(
                    tx,
                    &task,
                    &new_worker,
                    Some(agent),
                )?
                .worker_id;
                horch_core::taskledger::testing::record_supervisor(tx, &w, me, started)?;
                Ok((run, w))
            })
            .unwrap();
        (ctx, run, worker)
    }

    #[test]
    fn watch_once_runs_1_fleetd_pass_with_the_json_of_today() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, run, worker) = once_fleet(dir.path());
        let json = super::once_report(&ctx, &run, true, true).unwrap();
        let checked: serde_json::Value = serde_json::from_str(&json).unwrap();
        let checked = checked.as_array().expect("a JSON list of checks");
        assert_eq!(checked.len(), 1, "{json}");
        assert_eq!(checked[0]["worker_id"], worker.as_str());
        for field in ["verdict", "note"] {
            assert!(checked[0][field].is_string(), "{field}: {json}");
        }
        assert!(checked[0]["round"].is_i64(), "{json}");
        // It was a pass of fleetd: it took a generation and wrote the
        // heartbeat of the pass.
        let generation: i64 = Store::open(&ctx.paths.ledger_db)
            .unwrap()
            .conn()
            .query_row("SELECT generation FROM fleetd", [], |r| r.get(0))
            .unwrap();
        assert_eq!(generation, 1);
        assert!(ctx.paths.fleet_root.join("run/heartbeat.json").is_file());
    }

    #[test]
    fn watch_once_defers_to_a_fleetd_that_holds_the_lock() {
        use horch_core::fleetd::fleet_runtime::adapters::fleetd_row;

        let dir = tempfile::tempdir().unwrap();
        let (ctx, run, worker) = once_fleet(dir.path());
        let mut store = Store::open(&ctx.paths.ledger_db).unwrap();
        // This test is the fleetd that holds the lock.
        let held = fleetd_row::start(&ctx.paths.fleet_root, &mut store)
            .unwrap()
            .expect("the fleetd lock");
        let before = workers::find(store.conn(), &worker).unwrap().unwrap();
        let line = super::once_report(&ctx, &run, true, true).unwrap();
        assert_eq!(
            line,
            format!(
                "fleetd PID {} holds the fleetd lock and does the checks: horch ledger watch --once runs no pass.",
                std::process::id()
            )
        );
        // No second pass: the generation of the holder stays, no heartbeat
        // is written, and the worker is not checked or reaped.
        let generation: i64 = store
            .conn()
            .query_row("SELECT generation FROM fleetd", [], |r| r.get(0))
            .unwrap();
        assert_eq!(generation, held.generation.0);
        assert!(!ctx.paths.fleet_root.join("run/heartbeat.json").exists());
        let after = workers::find(store.conn(), &worker).unwrap().unwrap();
        assert_eq!(after.status, before.status);
    }

    /// `horch ledger watch --once` runs the Phase 4 pass of fleetd: its 1
    /// pass binds the ready task of a run to a push attempt
    /// (`docs/tickets/fleetd-p4/46-pass-wiring-dispatch-and-verifier.md`).
    #[test]
    fn watch_once_runs_the_phase4_pass() {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        use horch_core::taskledger::model::{Actor, WorktreeMode};
        use horch_core::taskledger::tasks::{self, NewTask};

        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "a",
        ]);
        let db = dir.join("ledger.sqlite3");
        // The job host of the pass is the sibling horch of the exe. A stub
        // that fails stands in for it, so no horch of the PATH runs.
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let failing_job_host = bin.join("horch");
        std::fs::write(&failing_job_host, "#!/bin/sh\nexit 1\n").unwrap();
        horch_platform::fs::make_executable(&failing_job_host).unwrap();
        // fleetd finds the sandbox tool on the PATH of its context (SR-4):
        // give it the PATH that the branch below reads.
        let path = std::env::var("PATH").unwrap_or_default();
        let env = MapEnv::new(dir)
            .with_exe(bin.join("horch-test"))
            .with("PATH", &path)
            .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
            .with("HORCH_DATA_DIR", dir.join("data").to_str().unwrap())
            .with("HORCH_LEDGER_DB", db.to_str().unwrap())
            .with("HORCH_HERDR_BIN", dir.join("no-herdr").to_str().unwrap());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        // The load of the build host does not hold dispatch in this test.
        let root = &ctx.paths.fleet_root;
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("fleet.toml"),
            "health_max_load_per_cpu = 1000000.0\n",
        )
        .unwrap();
        let mut store = Store::open(&db).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            ..Default::default()
        };
        let (run, task) = store
            .immediate(|tx| {
                let run = runs::create(tx, &project, &new_run)?;
                // A task needs an idea in flight of the feature of its run
                // (FR-12).
                run_idea(tx, &run.run_id, &util::now(), true)?;
                let mut t = NewTask::work("a", "a.md", "dev");
                t.worktree_mode = WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                Ok((run, task.task_id))
            })
            .unwrap();

        super::once_report(&ctx, &run, true, true).unwrap();

        let path = std::env::var_os("PATH");
        if horch_platform::sandbox::host_problem(path.as_deref()).is_some() {
            let status: String = store
                .conn()
                .query_row(
                    "SELECT status FROM tasks WHERE task_id = ?1",
                    [&task],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(status, "ready", "no check sandbox: no bind");
            return;
        }
        // The launch has no herdr, so the attempt ends crashed in the same
        // pass and rung 6 of the stall ladder makes the task ready after a
        // backoff (T-307).
        let (status, not_before, via, outcome): (String, Option<i64>, String, String) = store
            .conn()
            .query_row(
                "SELECT t.status, t.not_before, a.via, a.outcome
                 FROM tasks t JOIN attempts a ON a.task_id = t.task_id
                 WHERE t.task_id = ?1",
                [&task],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), via.as_str(), outcome.as_str()),
            ("ready", "push", "crashed")
        );
        assert!(not_before.is_some(), "no backoff after the crashed attempt");
    }
}
