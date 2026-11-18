//! `horch idea amend <id> <text>`: add text to an idea
//! (`docs/specs/fleetd/design.md`, "Ideas"). It sends `idea_amend` to
//! fleetd. After merge, fleetd makes a new idea that amends it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::{json, Map};

#[derive(Args)]
pub struct AmendArgs {
    /// The idea id.
    pub id: String,
    /// The text to add to the idea.
    pub text: String,
}

/// `horch idea amend`.
pub fn run(ctx: &RuntimeContext, args: AmendArgs) -> Result<ExitCode> {
    let mut payload = Map::new();
    payload.insert("idea".into(), json!(args.id));
    payload.insert("text".into(), json!(args.text));
    super::send(ctx, "horch idea amend", Kind::IdeaAmend, payload, |r| {
        let idea = super::text(r, "idea");
        if r.get("amends").is_some() {
            println!(
                "{idea}: a new idea in the feature {} amends {}",
                super::text(r, "feature"),
                super::text(r, "amends")
            );
        } else {
            println!("{idea}: amended ({})", super::text(r, "state"));
        }
    })
}
