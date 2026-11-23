use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Subcommand;
use horch_core::execution::lifecycle::ExecCli;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, ConformanceSuite, JobCtx, Runners,
};
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::conformance_runner::{
    reference_profile, ConformanceRunner,
};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::conformance::{run_suite, SuiteSubject};
use horch_core::fleetd::worker_profiles::application::docs_sync::DocsSyncRunner;
use horch_core::fleetd::worker_profiles::domain::conformance::{
    ConformanceReport, CONTEXT_LIMIT, FIRST_EVENT_LIMIT,
};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::Store;
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

pub fn job(ctx: &RuntimeContext, command: JobCommand) -> Result<ExitCode> {
    match command {
        JobCommand::Requeue { job } => requeue(ctx, job),
        JobCommand::Run { id } => run(ctx, id),
    }
}

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

fn run(ctx: &RuntimeContext, id: i64) -> Result<ExitCode> {
    let ledger = Store::open_read_only(&ctx.paths.ledger_db)?;
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
    let sandboxes = WorkerSandboxes::on_path(
        codex,
        ctx.paths.fleet_root.clone(),
        ctx.inherited.path.clone(),
    );
    let tools = super::taskledger::tools(ctx);
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &sandboxes,
        sweep: Some(&tools),
    };
    let docs = super::docscmd::AgentDocs::new()?;
    let suite = Suite { ctx, docs: &docs };
    let runners = Runners {
        checks: &checks,
        docs: Some(&docs),
        conformance: Some(&suite),
    };
    match jobs::run_job_with(&jctx, ledger.conn(), id, std::process::id(), &runners)? {
        Some(o) => output::println(&format!("job {id}: {}: {}", o.state().as_str(), o.text)),
        None => output::println(&format!(
            "job {id}: this process does not run it: no result is written"
        )),
    }
    Ok(ExitCode::SUCCESS)
}

struct Suite<'a> {
    ctx: &'a RuntimeContext,
    docs: &'a super::docscmd::AgentDocs,
}

impl Suite<'_> {
    fn runner<'b>(&'b self, teammate: &'b horch_core::roster::Teammate) -> ConformanceRunner<'b> {
        ConformanceRunner {
            ctx: self.ctx,
            teammate,
            fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
            env_rule: Vec::new(),
            scratch: self.ctx.paths.fleet_root.join("conformance"),
            first_event_limit: FIRST_EVENT_LIMIT,
        }
    }
}

fn exec_cli(cli: &str) -> Result<ExecCli> {
    Ok(match cli {
        "claude" => ExecCli::Claude,
        "codex" => ExecCli::Codex,
        "agy" => ExecCli::Antigravity,
        other => bail!("{other} has no exec adapter, so it has no conformance suite"),
    })
}

impl ConformanceSuite for Suite<'_> {
    fn version_cmd(&self, cli: &str) -> Result<String> {
        exec_cli(cli)?;
        self.docs.version_cmd(cli)
    }

    fn adapter_hash(&self, cli: &str) -> Result<String> {
        let profile = reference_profile(exec_cli(cli)?);
        self.runner(&profile).adapter_hash()
    }

    fn run(&self, subject: SuiteSubject) -> Result<ConformanceReport> {
        let profile = reference_profile(exec_cli(&subject.cli)?);
        Ok(run_suite(subject, &mut self.runner(&profile)))
    }
}
