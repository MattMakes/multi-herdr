use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self as store, JobRow};
use crate::fleetd::fleet_runtime::adapters::scans::{self, ProcessItem, ScanClock};
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobRunner};
use crate::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::State;
use crate::fleetd::worker_profiles::application::ports::ProcessHost;

const TICK: Duration = Duration::from_millis(100);

pub struct Starter<'a> {
    pub ctx: &'a JobCtx<'a>,
    pub host: &'a dyn ProcessHost,
    pub horch: &'a Path,
    pub output: Option<PathBuf>,
}

pub fn run_now(conn: &mut Connection, job_id: i64, s: &Starter<'_>) -> Result<JobRow> {
    let mut runner = JobRunner::new(s.host);
    loop {
        runner.reap();
        let row = store::get(conn, job_id)?.with_context(|| format!("no job {job_id}"))?;
        let clock = fleet_clock(conn)?;
        match row.state {
            State::Queued => start(conn, &row, s, &mut runner, clock)?,
            State::Running => observe(conn, s.ctx, &[row], clock)?,
            State::Passed | State::Failed | State::Abandoned => return Ok(row),
        }
        std::thread::sleep(TICK);
    }
}

fn start(
    conn: &mut Connection,
    row: &JobRow,
    s: &Starter<'_>,
    runner: &mut JobRunner<'_>,
    clock: FleetSeconds,
) -> Result<()> {
    if let Some(at) = row.not_before.filter(|at| *at > clock) {
        bail!(
            "job {} waits for its backoff until {} s on the fleet clock (now {} s); \
             the pass starts it then",
            row.id,
            at.0,
            clock.0
        );
    }
    if let (true, Some(repo)) = (jobs::is_git_writer(row.kind), row.repo.as_deref()) {
        let writers = jobs::git_writers_running(conn, repo)?;
        if !writers.is_empty() {
            return observe(conn, s.ctx, &writers, clock);
        }
    }
    let spec = jobs::job_spec(s.horch, row.id, s.output.clone());
    runner.start(s.ctx, conn, row, &spec, clock)?;
    Ok(())
}

fn observe(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    runs: &[JobRow],
    clock: FleetSeconds,
) -> Result<()> {
    let items: Vec<ProcessItem> = runs
        .iter()
        .filter_map(|r| {
            Some(ProcessItem {
                key: r.id.to_string(),
                pid: r.pid?,
                pid_start: r.pid_start,
            })
        })
        .collect();
    let start = Instant::now();
    let mut now = Instant::now;
    let mut scan_clock = ScanClock {
        deadline: Deadline::pass(start, ctx.config.pass_time_budget_s)
            .scan(start, ctx.config.scan_budget_ms),
        now: &mut now,
        stamp: "run_now",
    };
    let facts = scans::scan_processes(&items, &mut Cursor::new("run_now"), &mut scan_clock).facts;
    jobs::end_from_files(ctx, conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let seen = jobs::observe(ctx, &tx, &facts, clock)?;
    tx.commit()?;
    for stop in &seen.stops {
        jobs::stop(stop);
    }
    Ok(())
}

fn fleet_clock(conn: &Connection) -> Result<FleetSeconds> {
    match clock_store::read(conn) {
        Ok(clock) => Ok(clock),
        Err(e) if no_row(&e) => Ok(FleetSeconds(0)),
        Err(e) => Err(e),
    }
}

fn no_row(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::QueryReturnedNoRows)
    )
}
