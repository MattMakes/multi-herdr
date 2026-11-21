use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::{json, Map};

#[derive(Args)]
pub struct ReviseArgs {
    /// The idea id.
    pub id: String,
    /// What to change.
    pub text: String,
}

pub fn run(ctx: &RuntimeContext, args: ReviseArgs) -> Result<ExitCode> {
    let mut payload = Map::new();
    payload.insert("idea".into(), json!(args.id));
    payload.insert("text".into(), json!(args.text));
    super::send(ctx, "horch idea revise", Kind::IdeaRevise, payload, |r| {
        println!(
            "{}: sent back ({})",
            super::text(r, "idea"),
            super::text(r, "state")
        );
    })
}
