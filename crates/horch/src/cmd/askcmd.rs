//! `horch ask "<question>"`: the worker writes 1 question file,
//! `outbox/questions/<n>.json`, of its own attempt (the `fleet ask` helper
//! of `docs/specs/fleetd/design.md`, protocol rule 5;
//! `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q5).
//!
//! The command finds its attempt by its parent chain, through the outbox
//! writer. It reads no environment variable for the attempt, and it opens
//! the ledger only to read. `n` is 1 more than the highest file there.

use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Args;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts;
use horch_core::fleetd::task_execution::adapters::outbox_writer::{NoLiveAttempt, OutboxWriter};
use horch_core::runtime::RuntimeContext;

use crate::output;

#[derive(Args)]
pub struct AskArgs {
    /// The question, for the orchestrator.
    #[arg(required = true, num_args = 1.., value_name = "QUESTION")]
    pub question: Vec<String>,
}

/// `horch ask`.
pub fn run(ctx: &RuntimeContext, args: AskArgs) -> Result<ExitCode> {
    let question = args.question.join(" ");
    if question.trim().is_empty() {
        bail!("give the question");
    }
    let out = match own_writer(ctx)? {
        Ok(out) => out,
        Err(no) => {
            eprintln!("horch ask: {no}");
            return Ok(ExitCode::from(1));
        }
    };
    let path = out.write_question(&horch_core::taskledger::util::now(), &question)?;
    output::println(&format!("wrote {}", path.display()));
    Ok(ExitCode::SUCCESS)
}

/// The outbox writer of the own attempt of this process: the live attempt
/// whose wrapper is in its parent chain. The inner `Err` says why this
/// process has no attempt; no ledger file is no attempt too. The ledger is
/// opened only to read.
pub(crate) fn own_writer(ctx: &RuntimeContext) -> Result<Result<OutboxWriter, NoLiveAttempt>> {
    let Some(store) = horch_core::taskledger::open_existing_read_only(ctx)? else {
        return Ok(Err(NoLiveAttempt { detail: None }));
    };
    let config = config_file::load(&ctx.paths.fleet_root)?;
    match OutboxWriter::of_this_process(&ctx.paths.fleet_root, store.conn(), &config, &PlatformFacts)
    {
        Ok(out) => Ok(Ok(out)),
        Err(e) => match e.downcast::<NoLiveAttempt>() {
            Ok(no) => Ok(Err(no)),
            Err(e) => Err(e),
        },
    }
}
