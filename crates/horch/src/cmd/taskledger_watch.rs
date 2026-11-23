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

const FLEETD_WATCHES: &str = "fleetd does the watch of each run: it checks the workers, reaps the dead and wakes the orchestrator.";

pub struct LiveEffects<'a> {
    pub ctx: &'a RuntimeContext,
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
    if !ctx.settings.no_watchdog {
        output::println(&fleetd::describe(&fleetd::run_check(ctx)?));
    }
    Ok(ExitCode::SUCCESS)
}

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
    use horch_core::workspace::PaneHost;
    use std::path::Path;
    #[cfg(unix)]
    use std::time::{Duration, UNIX_EPOCH};

    fn worker(dir: &Path) -> Worker {
        let mut store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir, &new_run)?;
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

    #[cfg(unix)]
    fn herdr(dir: &Path) -> PaneHost {
        let bin = dir.join("herdr");
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
        PaneHost::herdr_bin(bin)
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
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                let mut a = NewTask::work("a", "a.md", "dev");
                a.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &a, Actor::Orchestrator)?.task_id;
                let new_worker = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "dev".into(),
                    ..Default::default()
                };
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
        let generation: i64 = store
            .conn()
            .query_row("SELECT generation FROM fleetd", [], |r| r.get(0))
            .unwrap();
        assert_eq!(generation, held.generation.0);
        assert!(!ctx.paths.fleet_root.join("run/heartbeat.json").exists());
        let after = workers::find(store.conn(), &worker).unwrap().unwrap();
        assert_eq!(after.status, before.status);
    }

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
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let failing_job_host = bin.join("horch");
        std::fs::write(&failing_job_host, "#!/bin/sh\nexit 1\n").unwrap();
        horch_platform::fs::make_executable(&failing_job_host).unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let env = MapEnv::new(dir)
            .with_exe(bin.join("horch-test"))
            .with("PATH", &path)
            .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
            .with("HORCH_DATA_DIR", dir.join("data").to_str().unwrap())
            .with("HORCH_LEDGER_DB", db.to_str().unwrap())
            .with("HORCH_HERDR_BIN", dir.join("no-herdr").to_str().unwrap());
        let ctx = RuntimeContext::from_env(&env).unwrap();
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
