//! `horch job requeue` and `horch job run` (`docs/specs/fleetd/design.md`,
//! "The kinds of job").
//!
//! `horch job requeue <job>` is the exit of a job that ended `abandoned`
//! `abandoned_run_limit` times in a row: it queues 1 more run of its key
//! with the count at 0, and resolves its needs-you item. `horch job run
//! <id>` is the job process that fleetd starts through the launcher; it is
//! hidden from the help.

use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::adapters::{config_file, jobs_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobKey, Queued, Runners,
};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::open_existing;

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

/// `horch job requeue <job>`: print the new run. The exit is the
/// operator's, so a worker pane is refused before the ledger opens. Until
/// the operator request of the command channel exists, the role of the
/// pane is the guard (`docs/specs/fleetd/design.md`, "The command
/// channel").
fn requeue(ctx: &RuntimeContext, job: i64) -> Result<ExitCode> {
    let role = ctx.worker.as_ref().and_then(|w| w.role.as_deref());
    if let Some(role) = role.filter(|r| *r != "orchestrator") {
        bail!("horch job requeue is the operator's: worker {role} cannot run it");
    }
    let Some(mut store) = open_existing(ctx)? else {
        bail!(
            "no job {job}: the task ledger {} does not exist",
            ctx.paths.ledger_db.display()
        );
    };
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let jctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let queued = store.immediate(|tx| {
        let row = jobs_store::get(tx, job)?.with_context(|| format!("no job {job}"))?;
        let key = JobKey {
            kind: row.kind,
            subject: &row.subject,
            input_key: &row.input_key,
        };
        jobs::requeue(&jctx, tx, &key)
    })?;
    match queued {
        Queued::New(id) => {
            let row = jobs_store::get(store.conn(), id)?.with_context(|| format!("no job {id}"))?;
            output::println(&format!(
                "Queued run {} of the {} job of {} as job {id}.",
                row.run, row.kind, row.subject
            ));
            Ok(ExitCode::SUCCESS)
        }
        Queued::Active(row) => bail!(
            "job {} of the same kind and subject is {}: no new run",
            row.id,
            row.state.as_str()
        ),
        Queued::Reused(row) | Queued::Held(row) => {
            bail!("job {}: no new run", row.id)
        }
    }
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
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &sandboxes,
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

#[cfg(test)]
mod tests {
    use horch_core::runtime::MapEnv;

    use super::*;

    /// A worker pane cannot requeue a job: the exit is the operator's. The
    /// refusal comes before the ledger opens.
    #[test]
    fn a_worker_cannot_requeue_a_job() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("ledger.sqlite3");
        let env = MapEnv::new(dir.path())
            .with("HOME", &dir.path().to_string_lossy())
            .with("HORCH_LEDGER_DB", &ledger.to_string_lossy())
            .with("HORCH_ROLE", "sonnet-1");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let err = job(&ctx, JobCommand::Requeue { job: 1 }).unwrap_err();
        assert!(err.to_string().contains("worker sonnet-1"), "{err:#}");
        assert!(!ledger.exists());
    }
}
