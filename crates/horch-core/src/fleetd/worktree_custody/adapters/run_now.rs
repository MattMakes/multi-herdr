//! Queue, start and wait for a worktree job from a command.
//!
//! Until Phase 4 a `horch` command runs the worktree jobs that it needs
//! (MAP.md of fleetd-p3, Q1): it queues the job in its own transaction,
//! then calls [`run_now`]. `run_now` starts the run with the start code of
//! the pass when no pass did, and waits for its end with no transaction
//! open. It observes the run with the function of the pass, so a run whose
//! process is gone ends `abandoned` the same way.

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

/// The time between 2 reads of the run.
const TICK: Duration = Duration::from_millis(100);

/// What [`run_now`] starts a job process with.
pub struct Starter<'a> {
    pub ctx: &'a JobCtx<'a>,
    /// The launcher.
    pub host: &'a dyn ProcessHost,
    /// The `horch` binary that runs `horch job run <id>`.
    pub horch: &'a Path,
    /// The file that the job process appends its output to.
    pub output: Option<PathBuf>,
}

/// Run the job `job_id` to its end from a command, and give its ended row.
/// [`jobs::outcome`] and [`jobs::result_data`] read its result.
///
/// Each 100 ms it reads the run:
/// - `queued`: start it through the launcher when no other git writer of
///   its repository runs. The move to `running` is a compare-and-set, so a
///   run that the pass started first gets no second process. While another
///   git writer runs, observe that run instead.
/// - `running`: observe it in 1 short transaction.
/// - ended: give it.
///
/// No transaction is open while it waits. A `queued` run that waits for
/// its backoff is an error: the pass starts it at the end of the backoff.
pub fn run_now(conn: &mut Connection, job_id: i64, s: &Starter<'_>) -> Result<JobRow> {
    let mut runner = JobRunner::new(s.host);
    loop {
        // An ended job process of this command is gone, not a zombie, for
        // the process scan.
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

/// Start the `queued` run `row`, or observe the git writers that hold its
/// repository.
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
    // `false`: the pass started it first, or a git writer of its repository
    // started; the next read of the run tells which.
    runner.start(s.ctx, conn, row, &spec, clock)?;
    Ok(())
}

/// Observe the `running` runs `runs` with the function of the pass, in 1
/// short transaction, then stop each run past its deadline.
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
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let seen = jobs::observe(ctx, &tx, &facts, clock)?;
    tx.commit()?;
    for stop in &seen.stops {
        jobs::stop(stop);
    }
    Ok(())
}

/// The fleet clock. A ledger that no fleetd ever ticked has no `fleetd`
/// row: its clock is 0.
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
