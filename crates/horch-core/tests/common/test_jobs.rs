use std::path::Path;

use anyhow::{Context, Result};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobRunner, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::application::create::RunJob;
use horch_core::taskledger::workers::Worker;
use rusqlite::Connection;

pub struct TestJobs;

impl RunJob for TestJobs {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow> {
        let job = get(conn, job_id)?;
        if job.state == State::Queued {
            let spec = LaunchSpec {
                program: "true".into(),
                args: Vec::new(),
                cwd: None,
                env_rule: Default::default(),
                detach: false,
                output: None,
            };
            JobRunner::new(&Launcher).start(ctx, conn, &job, &spec, FleetSeconds(0))?;
        }
        let job = get(conn, job_id)?;
        if job.state == State::Running {
            let checks = CheckRunner {
                host: &Launcher,
                profiles: &NoProfiles,
                sweep: None,
            };
            let pid = job.pid.context("a running job has a pid")?;
            jobs::run_job(ctx, conn, job_id, pid, &checks)?;
            jobs::end_from_files(ctx, conn)?;
        }
        get(conn, job_id)
    }
}

fn get(conn: &Connection, job_id: i64) -> Result<JobRow> {
    jobs_store::get(conn, job_id)?.with_context(|| format!("no job {job_id}"))
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> Result<Option<LaunchSpec>> {
        Ok(None)
    }
}
