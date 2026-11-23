use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::ideas::application::queries::idea_view;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::open_existing_read_only;
use serde_json::{json, Map};

const COMMAND: &str = "horch idea approve";

#[derive(Args)]
pub struct ApproveArgs {
    /// The idea id.
    pub id: String,
}

pub fn run(ctx: &RuntimeContext, args: ApproveArgs) -> Result<ExitCode> {
    if let Some(refused) = super::refuse_inside_fleet_attempt(ctx, COMMAND) {
        return Ok(refused);
    }
    let view = match open_existing_read_only(ctx)? {
        Some(store) => idea_view(store.conn(), &ctx.paths.fleet_root, &args.id)?,
        None => None,
    };
    let Some(view) = view else {
        eprintln!("{COMMAND}: no idea {}", args.id);
        return Ok(ExitCode::from(1));
    };
    let Some(assumptions) = view.assumptions else {
        eprintln!(
            "{COMMAND}: the idea {} has no assumptions.md to approve",
            args.id
        );
        return Ok(ExitCode::from(1));
    };
    print!("{}", assumptions.text);
    if !assumptions.text.ends_with('\n') {
        println!();
    }
    println!("sha256: {}", assumptions.sha256);
    let mut payload = Map::new();
    payload.insert("idea".into(), json!(args.id));
    payload.insert("sha256".into(), json!(assumptions.sha256));
    super::send(ctx, COMMAND, Kind::IdeaApprove, payload, |r| {
        println!(
            "{}: approved ({})",
            super::text(r, "idea"),
            super::text(r, "state")
        );
    })
}
