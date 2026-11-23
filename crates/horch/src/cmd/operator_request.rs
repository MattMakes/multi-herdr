use std::process::ExitCode;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::Value;

pub enum Answer {
    Applied(Value),
    Refused(String),
    Queued,
}

pub fn request(ctx: &RuntimeContext, kind: Kind, payload: Value) -> Result<Answer> {
    let Value::Object(payload) = payload else {
        bail!("the payload of a request is a JSON object");
    };
    Ok(match channel_client::send(ctx, kind, payload)? {
        Sent::Replied(reply) => match reply.outcome {
            Outcome::Applied(result) => Answer::Applied(result),
            Outcome::Refused(reason) => Answer::Refused(reason),
        },
        Sent::Queued { .. } => Answer::Queued,
    })
}

pub fn run(
    ctx: &RuntimeContext,
    kind: Kind,
    payload: Value,
    print: impl FnOnce(&Value) -> Result<()>,
) -> Result<ExitCode> {
    match request(ctx, kind, payload)? {
        Answer::Applied(result) => {
            print(&result)?;
            Ok(ExitCode::SUCCESS)
        }
        Answer::Refused(reason) => {
            eprintln!("refused: {reason}");
            Ok(ExitCode::from(1))
        }
        Answer::Queued => Ok(ExitCode::SUCCESS),
    }
}

pub fn print_lines(result: &Value) {
    if let Some(note) = result.get("note").and_then(Value::as_str) {
        eprintln!("horch: {note}");
    }
    for line in result["lines"].as_array().into_iter().flatten() {
        if let Some(line) = line.as_str() {
            crate::output::println(line);
        }
    }
}
