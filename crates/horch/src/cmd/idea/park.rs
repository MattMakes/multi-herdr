use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::{json, Map, Value};

const NO_REASON: &str = "parked by the operator";

#[derive(Args)]
pub struct ParkArgs {
    /// The idea id.
    pub id: String,
    /// Why the idea is parked.
    pub reason: Option<String>,
}

pub fn run(ctx: &RuntimeContext, args: ParkArgs) -> Result<ExitCode> {
    let reason = args
        .reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| NO_REASON.to_string());
    let mut payload = Map::new();
    payload.insert("idea".into(), json!(args.id));
    payload.insert("reason".into(), json!(reason));
    super::send(ctx, "horch idea park", Kind::IdeaPark, payload, |r| {
        let rescues = r
            .get("rescues")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        println!(
            "{}: parked ({rescues} worktrees rescued)",
            super::text(r, "idea")
        );
    })
}
