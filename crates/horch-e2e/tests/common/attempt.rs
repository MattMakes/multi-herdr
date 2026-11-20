#![allow(dead_code)]

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::effects::{write_launch, LaunchRecord};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use horch_e2e::harness::{wait_for_or, Harness};
use horch_e2e::ledger_seed::seed_run;

const SHORT_JOB_LIMITS_TOML: &str =
    "pass_period_s = 1\nstop_grace_s = 1\n[job_limit_s]\ndocs_sync = 1\nconformance = 1\n";

pub fn limit_jobs(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("fleet.toml"), SHORT_JOB_LIMITS_TOML).unwrap();
}

pub fn wait_for_quiet_jobs(db: &Path) {
    let mut quiet = None;
    wait_for_or(
        "the jobs of the test fleetd to end",
        || {
            let conn = rusqlite::Connection::open(db).ok()?;
            let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).ok();
            let total = count("SELECT COUNT(*) FROM jobs")?;
            let active = count(
                "SELECT COUNT(*) FROM jobs
                 WHERE state = 'running' OR (state = 'queued' AND not_before IS NULL)",
            )?;
            if active > 0 || total == 0 {
                quiet = None;
                return None;
            }
            let since = *quiet.get_or_insert((total, Instant::now()));
            if since.0 != total {
                quiet = Some((total, Instant::now()));
                return None;
            }
            (since.1.elapsed() >= Duration::from_secs(3)).then_some(())
        },
        || {
            let conn = rusqlite::Connection::open(db).unwrap();
            let mut st = conn
                .prepare("SELECT id, kind, subject, state, not_before FROM jobs")
                .unwrap();
            let rows: Vec<String> = st
                .query_map([], |r| {
                    Ok(format!(
                        "{} {} {} {} nb{:?}",
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<i64>>(4)?
                    ))
                })
                .unwrap()
                .flatten()
                .collect();
            rows.join("\n")
        },
    );
}

pub fn spawn_attempt(h: &Harness, name: &str) -> Output {
    limit_jobs(&h.fleet_root());
    h.serve_fleetd();
    wait_for_quiet_jobs(&h.ledger_db());
    spawn_served_attempt(h, name)
}

pub fn spawn_served_attempt(h: &Harness, name: &str) -> Output {
    let run = seed_run(h, "skl", Some(1), "w1:p1");
    let ticket = h.project.join("skl-1.md");
    std::fs::write(&ticket, "# Probe\n\n**Check:** `true`\n").unwrap();
    let mut store = Store::open(&h.ledger_db()).unwrap();
    let new = NewTask {
        worktree_mode: WorktreeMode::Shared,
        ..NewTask::work("Probe", &ticket.to_string_lossy(), name)
    };
    let task = store
        .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
        .unwrap();
    let attempt = format!("{}/A1", task.task_id);
    store
        .immediate(|tx| {
            let worker = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: name.into(),
                    role: Some(format!("{name}-1")),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            tx.execute(
                "UPDATE workers SET attempt = 1 WHERE worker_id = ?1",
                [&worker],
            )?;
            tx.execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, via, started_at)
                 VALUES (?1, ?2, ?3, 'launching', 'push', ?4)",
                rusqlite::params![attempt, task.task_id, worker, util::now()],
            )?;
            tx.execute(
                "UPDATE tasks SET status = 'running', attempts = 1, current_worker_id = ?2
                 WHERE task_id = ?1",
                rusqlite::params![task.task_id, worker],
            )?;
            Ok(())
        })
        .unwrap();
    write_launch(
        &h.fleet_root(),
        &LaunchRecord {
            attempt: attempt.clone(),
            launched_at: FleetSeconds(0),
            process: None,
        },
    )
    .unwrap();
    h.run(&[
        "spawn",
        name,
        "--run",
        "skl",
        "--exact",
        "--attempt",
        &attempt,
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ])
}
