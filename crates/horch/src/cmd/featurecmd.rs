//! `horch feature abandon <feature>`: the operator ends a feature
//! (`docs/specs/fleetd/design.md`, "The feature state machine", row
//! `abandoned`; "A cancel"; I10; FM-43; ticket 40 of
//! `docs/tickets/fleetd-p4/`).
//!
//! The command writes a rescue ref of each task branch of the feature that
//! is not landed and reads it back, then sends 1 `feature_abandon` request
//! (RR-5). fleetd checks each ref in git, then in 1 transaction moves each
//! such branch to `abandoned`, cancels each task of the run that is not
//! terminal, and moves the feature to `abandoned`. A task with a live
//! attempt waits for its stop: the pass of fleetd stops its processes and
//! cancels it when they are gone. A crash before the request is completed
//! by the next abandon, with the same rescue refs.
//!
//! It refuses to run inside a fleet attempt (SR-5; ticket 23 of
//! fleetd-p6). fleetd decides who may send the request (SR-8): the command
//! reads no role.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::fleetd::integration::application::commands;
use horch_core::fleetd::integration::application::landing::GitBranchRescues;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use serde::Deserialize;
use serde_json::{json, Value};

use super::branchcmd::{rescue_git, rescue_payload};
use super::operator_request;
use crate::output;

#[derive(Subcommand)]
pub enum FeatureCommand {
    /// Abandon a feature. Each branch of the feature that is not landed
    /// gets a rescue ref first, and each task of the feature that is not
    /// done is cancelled.
    Abandon {
        /// The feature id, for example `F-3`.
        feature: String,
    },
}

/// `horch feature`.
pub fn run(ctx: &RuntimeContext, command: FeatureCommand) -> Result<ExitCode> {
    match command {
        FeatureCommand::Abandon { feature } => {
            let command = "horch feature abandon";
            if let Some(refused) = super::idea::refuse_inside_fleet_attempt(ctx, command) {
                return Ok(refused);
            }
            let payload = rescued(ctx, &feature)?;
            operator_request::run(ctx, Kind::FeatureAbandon, payload, |r| {
                for line in lines(&serde_json::from_value(r.clone())?) {
                    output::println(&line);
                }
                Ok(())
            })
        }
    }
}

/// The payload of the abandon of `feature`: the rescue ref of each branch
/// that is not landed, written and read back first (I10), with its tip.
fn rescued(ctx: &RuntimeContext, feature: &str) -> Result<Value> {
    let store = taskledger::open_existing_read_only(ctx)?.with_context(|| {
        format!(
            "the task ledger {} does not exist. fleetd creates it at its start: run horch fleetd check",
            ctx.paths.ledger_db.display()
        )
    })?;
    let repo = commands::feature_repo(store.conn(), feature)?;
    let git = rescue_git(ctx, &repo, feature)?;
    let rescues = GitBranchRescues { git: &git };
    let branches = commands::rescue_feature(store.conn(), &rescues, &repo, feature)?
        .into_iter()
        .map(|r| rescue_payload(ctx, &repo, r))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "feature": feature, "branches": branches }))
}

/// The result of a `feature_abandon` request.
#[derive(Deserialize)]
struct Abandoned {
    feature: String,
    already: bool,
    branches: Vec<Branch>,
    /// The workers of the live attempts of the cancelled tasks.
    to_stop: Vec<String>,
}

#[derive(Deserialize)]
struct Branch {
    branch: String,
    rescue_ref: Option<String>,
}

/// The lines of an abandon: each branch, the feature, and the stops that
/// the pass of fleetd does.
fn lines(done: &Abandoned) -> Vec<String> {
    let mut out: Vec<String> = done
        .branches
        .iter()
        .map(|b| match &b.rescue_ref {
            Some(name) => format!(
                "{} is abandoned; its rescue ref {name} keeps its tip.",
                b.branch
            ),
            None => format!(
                "{} is abandoned; git has no such branch, so it has no tip to keep.",
                b.branch
            ),
        })
        .collect();
    out.push(if done.already {
        format!("{} was abandoned already.", done.feature)
    } else {
        format!("{} is abandoned.", done.feature)
    });
    if !done.to_stop.is_empty() {
        out.push(format!(
            "The next pass of fleetd stops {} and cancels each of their tasks when the process is gone.",
            done.to_stop.join(", ")
        ));
    }
    out
}
