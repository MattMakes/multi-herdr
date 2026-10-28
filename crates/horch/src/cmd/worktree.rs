//! Worker worktree creation, landing, and inspection.

use anyhow::{anyhow, Result};
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::landing::{ClaimState, Landing};

use super::messaging;

fn landing(ctx: &RuntimeContext) -> Landing {
    Landing::new(ctx.bins.harness.git.clone(), ctx.paths.state_root.clone())
}

fn role(ctx: &RuntimeContext) -> &str {
    ctx.worker
        .as_ref()
        .and_then(|worker| worker.role.as_deref())
        .unwrap_or("orchestrator")
}

pub fn new(ctx: &RuntimeContext, name: &str) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let record_id = ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.record_id.as_deref());
    let claim = landing(ctx)
        .create(&dir, role(ctx), record_id, name)
        .map_err(|error| {
            let text = format!("{error:#}");
            if text.contains("Operation not permitted") {
                anyhow!("{text}\nThe sandbox cannot write .git here. A Codex pane writes .git only in its \
                         own folder, under permission_mode auto or acceptEdits (the fleet_write profile).")
            } else {
                error
            }
        })?;
    println!("path: {}", claim.path.display());
    println!("branch: {}", claim.branch);
    println!("base: {}", claim.base);
    println!("next: cd {}", claim.path.display());
    Ok(())
}

pub fn land(ctx: &RuntimeContext, name: Option<&str>, check: Option<&str>) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let result = landing(ctx).land(&dir, role(ctx), name, check)?;
    let short: String = result.commit.chars().take(7).collect();
    let line = format!(
        "LANDED {} -> {} {} ({} commits)",
        result.branch, result.base, short, result.commits
    );
    println!("{line}");
    if let Err(error) = messaging::tell(ctx, "orchestrator", &format!("NOTE: {line}")) {
        eprintln!("horch worktree land: could not report landing ({error:#})");
    }
    Ok(())
}

pub fn list(ctx: &RuntimeContext) -> Result<()> {
    let dir = ctx.paths.current_dir()?;
    let landing = landing(ctx);
    for claim in landing.claims(&dir)? {
        let status = landing.status(&claim)?;
        let state = match claim.state {
            ClaimState::Open => "open",
            ClaimState::Landed => "landed",
            ClaimState::Unlanded => "unlanded",
        };
        println!(
            "{} {} {} {} {} ahead={} dirty={} contained={}",
            claim.name,
            claim.role,
            state,
            claim.branch,
            claim.base,
            status.ahead,
            status.dirty,
            status.contained
        );
    }
    let unregistered = landing.unregistered(&dir)?;
    if unregistered.is_empty() {
        println!("unregistered: none");
    } else {
        println!("unregistered:");
        for entry in unregistered {
            println!(
                "{} branch={} ahead={} dirty={}",
                entry.path.display(),
                entry.branch.as_deref().unwrap_or("detached"),
                entry.ahead,
                entry.dirty
            );
        }
    }
    Ok(())
}
