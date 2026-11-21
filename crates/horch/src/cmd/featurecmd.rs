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

#[derive(Deserialize)]
struct Abandoned {
    feature: String,
    already: bool,
    branches: Vec<Branch>,
    to_stop: Vec<String>,
}

#[derive(Deserialize)]
struct Branch {
    branch: String,
    rescue_ref: Option<String>,
}

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
