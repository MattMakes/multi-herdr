//! `horch feature abandon <feature>`: the operator ends a feature
//! (`docs/specs/fleetd/design.md`, "Features, landing and integration").
//! Ticket 35 builds it and adds the role check. It refuses to run inside a
//! fleet attempt (SR-5; ticket 23 of fleetd-p6).

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;

#[derive(Subcommand)]
pub enum FeatureCommand {
    /// Abandon a feature. Each branch of the feature that is not landed
    /// gets a rescue ref, and each idea that is not merged is cancelled.
    Abandon {
        /// The feature id, for example `F-3`.
        feature: String,
    },
}

/// `horch feature`.
pub fn run(ctx: &RuntimeContext, command: FeatureCommand) -> Result<ExitCode> {
    match command {
        FeatureCommand::Abandon { .. } => {
            let command = "horch feature abandon";
            if let Some(refused) = super::idea::refuse_inside_fleet_attempt(ctx, command) {
                return Ok(refused);
            }
            Ok(super::not_built_yet(command))
        }
    }
}
