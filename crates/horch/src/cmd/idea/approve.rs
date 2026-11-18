//! `horch idea approve <id>`: approve the assumptions of an idea
//! (`docs/specs/fleetd/design.md`, "Quick approval stays with you").
//! Ticket 23 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct ApproveArgs {
    /// The idea id.
    pub id: String,
}

/// `horch idea approve`.
pub fn run(_ctx: &RuntimeContext, _args: ApproveArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea approve", 23)
}
