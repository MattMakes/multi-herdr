pub mod agentlist;
pub mod balancecmd;
pub mod channel;
pub mod compact;
pub mod context;
pub mod cost;
pub mod delivery_probe;
pub mod doctor;
pub mod install;
pub mod layoutcmd;
pub mod mail;
pub mod marketplacecmd;
pub mod messaging;
pub mod mode;
pub mod quotacmd;
pub mod recipes;
pub mod restore;
pub mod route;
pub mod selfupdate;
pub mod sessionscmd;
pub mod skillscmd;
pub mod smoke;
pub mod spawn;
pub mod taskledger;
pub mod taskledger_complexity;
pub mod taskledger_finish;
pub mod taskledger_spawn;
pub mod taskledger_wake;
pub mod taskledger_watch;
pub mod teammatescmd;
pub mod telemetry;
pub mod tilecmd;
pub mod update;
pub mod usagecmd;
pub mod worker;
pub mod worktree;

use std::path::Path;

use anyhow::Result;
use horch_core::roster::Roster;
use horch_core::runtime::RuntimeContext;
use horch_core::skills::SkillCatalog;

pub fn load_roster(ctx: &RuntimeContext, explicit: Option<&str>) -> Result<Roster> {
    let roster = load_roster_unwarned(ctx, explicit)?;
    for w in roster.load_warnings() {
        eprintln!("warning: {w}");
    }
    Ok(roster)
}

pub fn load_roster_unwarned(ctx: &RuntimeContext, explicit: Option<&str>) -> Result<Roster> {
    Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        explicit,
    )
    .map(|r| match SkillCatalog::installed(&ctx.paths.data_root) {
        Ok(catalog) => r.with_skill_catalog(catalog),
        Err(_) => r,
    })
}

pub fn path_text(path: Option<&Path>) -> Option<String> {
    path.map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}
