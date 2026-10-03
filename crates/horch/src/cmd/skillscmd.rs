//! `horch skills` - the skill catalog: the legacy listing, and the
//! marketplace install, update and check commands.
//!
//! `horch skills [list] [--phase <p>] [--json]` prints exactly what it
//! printed before the marketplace existed (the A0 oracle).

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;
use horch_core::teammates::Phase;

use crate::output;

#[derive(Subcommand)]
pub enum SkillsCommand {
    /// The bundled catalog and its estimated context cost (the default).
    List {
        #[arg(long)]
        phase: Option<Phase>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Run `horch skills`. `phase` and `json` are the group's own flags, for
/// the legacy form without a subcommand.
pub fn run(
    ctx: &RuntimeContext,
    command: Option<SkillsCommand>,
    phase: Option<Phase>,
    json: bool,
) -> Result<ExitCode> {
    let _ = ctx;
    match command {
        None => list(phase, json)?,
        Some(SkillsCommand::List { phase, json }) => list(phase, json)?,
    }
    Ok(ExitCode::SUCCESS)
}

/// The legacy listing, byte for byte.
fn list(phase: Option<Phase>, json: bool) -> Result<()> {
    let catalog = horch_core::skills::describe(phase)?;
    if json {
        output::println(&serde_json::to_string_pretty(&catalog)?);
        return Ok(());
    }
    output::println(&format!(
        "Phase: {}",
        phase.map(|p| p.to_string()).unwrap_or_else(|| "all".into())
    ));
    for skill in catalog["skills"].as_array().expect("catalog skills array") {
        output::println(&format!(
            "  {:<18} {}",
            skill["name"].as_str().unwrap(),
            skill["description"].as_str().unwrap()
        ));
    }
    output::println(&format!(
        "Metadata: {} bytes (~{} tokens, estimate only); workflows load on demand.",
        catalog["metadata_bytes"], catalog["metadata_tokens_estimate"]
    ));
    Ok(())
}
