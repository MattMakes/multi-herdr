//! `horch idea cancel <id> <reason>`: cancel an idea
//! (`docs/specs/fleetd/design.md`, "Ideas"). It sends `idea_cancel` to
//! fleetd, which cancels each task of the idea that is not terminal.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::{json, Map, Value};

const COMMAND: &str = "horch idea cancel";

#[derive(Args)]
pub struct CancelArgs {
    /// The idea id.
    pub id: String,
    /// Why the idea is cancelled.
    pub reason: String,
}

/// `horch idea cancel`.
pub fn run(ctx: &RuntimeContext, args: CancelArgs) -> Result<ExitCode> {
    if args.reason.trim().is_empty() {
        eprintln!(
            "{COMMAND}: give a nonempty reason for the cancel of {}",
            args.id
        );
        return Ok(ExitCode::from(1));
    }
    let mut payload = Map::new();
    payload.insert("idea".into(), json!(args.id));
    payload.insert("reason".into(), json!(args.reason));
    super::send(ctx, COMMAND, Kind::IdeaCancel, payload, |r| {
        let idea = super::text(r, "idea");
        if r.get("cancelled").and_then(Value::as_bool) == Some(true) {
            println!("{idea}: cancelled");
        } else {
            let stop = r.get("stop").and_then(Value::as_array).map_or(0, Vec::len);
            println!("{idea}: cancel requested. The idea is cancelled when its tasks end ({stop} workers to stop).");
        }
    })
}
