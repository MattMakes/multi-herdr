//! `horch idea park <id> [<reason>]`: park an idea
//! (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 23 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct ParkArgs {
    /// The idea id.
    pub id: String,
    /// Why the idea is parked.
    pub reason: Option<String>,
}

/// `horch idea park`.
pub fn run(_ctx: &RuntimeContext, _args: ParkArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea park", 23)
}
