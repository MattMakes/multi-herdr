//! `horch job requeue` and `horch job run` (`docs/specs/fleetd/design.md`,
//! "The kinds of job").
//!
//! `horch job requeue <job>` is the exit of a job that ended `abandoned`
//! `abandoned_run_limit` times in a row: it queues 1 more run of its key
//! with the count at 0, and resolves its needs-you item. `horch job run
//! <id>` is the job process that fleetd starts through the launcher; it is
//! hidden from the help.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, CheckRunner, JobCtx, Runners};
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::CodexSandbox;
use serde_json::json;

use super::operator_request;
use crate::output;

#[derive(Subcommand)]
pub enum JobCommand {
    /// Queue 1 more run of a job and set its count of abandoned runs to 0.
    /// The exit of a job on the needs-you list.
    Requeue {
        /// The id of a run of the job.
        job: i64,
    },

    /// Run the job process of the run <ID>. fleetd starts it.
    #[command(hide = true)]
    Run { id: i64 },
}

/// `horch job`.
pub fn job(ctx: &RuntimeContext, command: JobCommand) -> Result<ExitCode> {
    match command {
        JobCommand::Requeue { job } => requeue(ctx, job),
        JobCommand::Run { id } => run(ctx, id),
    }
}

/// `horch job requeue <job>`: 1 `job_requeue` request to fleetd, then the
/// new run. fleetd decides who may send it (`docs/specs/fleetd/design.md`,
/// "The command channel"): the command reads no role.
fn requeue(ctx: &RuntimeContext, job: i64) -> Result<ExitCode> {
    operator_request::run(ctx, Kind::JobRequeue, json!({ "job": job }), |r| {
        output::println(&format!(
            "Queued run {} of the {} job of {} as job {}.",
            r["run"],
            r["kind"].as_str().unwrap_or("?"),
            r["subject"].as_str().unwrap_or("?"),
            r["job"]
        ));
        Ok(())
    })
}

/// `horch job run <id>`: run the executor of the job and write its result.
fn run(ctx: &RuntimeContext, id: i64) -> Result<ExitCode> {
    let mut conn = super::fleetd::ledger_conn(&ctx.paths.ledger_db)?;
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let jctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let codex = CodexSandbox {
        bin: ctx.bins.harness.codex.clone(),
        state_root: ctx.paths.state_root.clone(),
        personas: super::taskledger::codex_personas(ctx),
    };
    let sandboxes = WorkerSandboxes::new(codex, ctx.paths.fleet_root.clone());
    let tools = super::taskledger::tools(ctx);
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &sandboxes,
        sweep: Some(&tools),
    };
    // The fetch of the docs is in horch, not in horch-core (NFR-06).
    let docs = super::docscmd::AgentDocs::new()?;
    let runners = Runners {
        checks: &checks,
        docs: Some(&docs),
    };
    match jobs::run_job_with(&jctx, &mut conn, id, std::process::id(), &runners)? {
        Some(o) => output::println(&format!("job {id}: {}: {}", o.state().as_str(), o.text)),
        None => output::println(&format!(
            "job {id}: this process does not run it: no result is written"
        )),
    }
    Ok(ExitCode::SUCCESS)
}
