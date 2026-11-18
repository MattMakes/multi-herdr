//! `horch idea amend <id> <text>`: add text to an idea
//! (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 23 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct AmendArgs {
    /// The idea id.
    pub id: String,
    /// The text to add to the idea.
    pub text: String,
}

/// `horch idea amend`.
pub fn run(_ctx: &RuntimeContext, _args: AmendArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea amend", 23)
}
