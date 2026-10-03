pub mod balancecmd;
pub mod cost;
pub mod doctor;
pub mod install;
pub mod layoutcmd;
pub mod ledgercmd;
pub mod messaging;
pub mod quotacmd;
pub mod recipes;
pub mod route;
pub mod skillscmd;
pub mod smoke;
pub mod spawn;
pub mod teammatescmd;
pub mod telemetry;
pub mod tilecmd;
pub mod usagecmd;
pub mod worker;

use std::path::Path;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::teammates::Roster;

/// The roster as this context sees it: the built-ins, `~/.config/horch/teammates`,
/// `$HORCH_TEAMMATES_DIR`, then `explicit`.
pub fn load_roster(ctx: &RuntimeContext, explicit: Option<&str>) -> Result<Roster> {
    Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        explicit,
    )
}

/// A non-empty value, as text, for a pane command line or a brief.
pub fn path_text(path: Option<&Path>) -> Option<String> {
    path.map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}
