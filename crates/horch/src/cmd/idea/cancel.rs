//! `horch idea cancel <id> <reason>`: cancel an idea
//! (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 23 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct CancelArgs {
    /// The idea id.
    pub id: String,
    /// Why the idea is cancelled.
    pub reason: String,
}

/// `horch idea cancel`.
pub fn run(_ctx: &RuntimeContext, _args: CancelArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea cancel", 23)
}
