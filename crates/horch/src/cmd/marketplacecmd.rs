//! `horch marketplace` - the installed-skill store under the data root
//! (`${XDG_DATA_HOME:-~/.local/share}/horch/`).

use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;
use horch_core::skills::catalog::{self, StoreState};
use serde_json::json;

use crate::output;

#[derive(Subcommand)]
pub enum MarketplaceCommand {
    /// The skills pinned in `marketplace.lock`.
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Rebuild any locked skill whose version directory is missing, at its
    /// pinned commit. Present versions are only digest-checked.
    Refresh,
}

pub fn run(ctx: &RuntimeContext, command: MarketplaceCommand) -> Result<ExitCode> {
    match command {
        MarketplaceCommand::List { json } => list(ctx, json)?,
        MarketplaceCommand::Refresh => refresh(ctx)?,
    }
    Ok(ExitCode::SUCCESS)
}

/// JSON: `{"store", "skills": [{id, source, requested_revision,
/// resolved_commit, version, digest, installed_at, path, present}]}`.
fn list(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let checks = catalog::check_store(&ctx.paths.data_root)?;
    if json {
        let skills: Vec<_> = checks
            .iter()
            .map(|(e, dir, state)| {
                json!({
                    "id": e.id,
                    "source": e.source,
                    "requested_revision": e.requested_revision,
                    "resolved_commit": e.resolved_commit,
                    "version": e.version,
                    "digest": e.digest,
                    "installed_at": e.installed_at,
                    "path": dir.display().to_string(),
                    "present": matches!(state, StoreState::Ok | StoreState::Tampered { .. }),
                })
            })
            .collect();
        let view = json!({
            "store": ctx.paths.data_root.display().to_string(),
            "skills": skills,
        });
        output::println(&serde_json::to_string_pretty(&view)?);
        return Ok(());
    }
    output::println(&format!("Store: {}", ctx.paths.data_root.display()));
    if checks.is_empty() {
        output::println("  no installed skills");
    }
    for (e, _, state) in &checks {
        output::println(&format!(
            "  {:<18} {:<18} {}{}",
            e.id,
            e.version,
            e.source,
            if *state == StoreState::Missing {
                " (missing)"
            } else {
                ""
            }
        ));
    }
    Ok(())
}

fn refresh(ctx: &RuntimeContext) -> Result<()> {
    let installer = catalog::installer(&ctx.paths.data_root, &ctx.bins.harness.git)?;
    let entries = installer
        .reinstall_from_lock()
        .map_err(|e| anyhow!("refresh: {e}"))?;
    for e in &entries {
        output::println(&format!("ok {} {}", e.id, e.version));
    }
    output::println(&format!("{} skill(s) present and verified", entries.len()));
    Ok(())
}
