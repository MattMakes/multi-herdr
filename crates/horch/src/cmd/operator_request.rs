//! 1 operator request of a `horch` command to fleetd (RR-5, FM-24).
//!
//! A command that changes state sends 1 request of its kind through the
//! channel client and writes no row itself. An applied request prints its
//! result and exits 0. A refused request prints `refused: <reason>` and
//! exits 1. A queued request exits 0: the client printed where it waits.

use std::process::ExitCode;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::Value;

/// What became of 1 request.
pub enum Answer {
    /// fleetd applied it, with this result.
    Applied(Value),
    /// fleetd refused it, for this reason.
    Refused(String),
    /// fleetd is down: the request waits in its request file.
    Queued,
}

/// Send 1 request of `kind` with the JSON object `payload` to fleetd.
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

/// [`request`], then the exit of the command: `print` gets the result of
/// an applied request.
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

/// The `lines` of a request result on stdout, and its `note` on stderr.
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
