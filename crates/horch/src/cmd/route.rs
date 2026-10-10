//! `horch route <teammate>` - what the spawn gate would do right now, with no
//! spawn (BAL-06). The same `decide()` the gate calls, on the same readings.

use std::process::ExitCode;

use anyhow::Result;
use horch_core::clock;
use horch_core::routing::balance::touched_pools;
use horch_core::routing::decision::{self, Decision, GateFlags};
use horch_core::routing::snapshot;
use horch_core::runtime::RuntimeContext;
use serde_json::json;

use crate::output;

pub fn route(
    ctx: &RuntimeContext,
    name: &str,
    json_out: bool,
    exact: bool,
    force: bool,
) -> Result<ExitCode> {
    let roster_dir = super::path_text(ctx.bins.roster_override.as_deref());
    let roster = super::load_roster(ctx, roster_dir.as_deref())?;
    let teammate = roster.require(name)?.clone();
    let root = ctx.paths.state_root.clone();
    let policy = super::quotacmd::load_policy(ctx, &root)?;
    let view = snapshot::obtain(
        &root,
        clock::now(),
        &policy,
        true,
        &super::quotacmd::quota_env(ctx),
    )?;
    let decision = decision::decide(
        &teammate,
        &roster,
        &view,
        policy.balance_mode,
        GateFlags { exact, force },
    );

    // Every pool the decision could touch: the teammate's and its fallbacks'.
    let pools = touched_pools(&teammate, &roster, &view);
    let refused = matches!(decision, Decision::Refuse { .. });
    if json_out {
        let (kind, via, reason) = match &decision {
            Decision::Spawn { note, .. } => ("spawn", None, note.clone()),
            Decision::Substitute { via, reason, .. } => {
                ("substitute", Some(via.clone()), Some(reason.clone()))
            }
            Decision::Refuse { reason, .. } => ("refuse", None, Some(reason.clone())),
        };
        output::println(&serde_json::to_string_pretty(&json!({
            "decision": kind,
            "teammate": teammate.name,
            "via": via,
            "reason": reason,
            "line": decision.line(),
            "mode": policy.balance_mode.as_str(),
            "pools": pools,
        }))?);
    } else {
        output::println(
            &decision
                .line()
                .unwrap_or_else(|| format!("SPAWN: {} runs as itself.", teammate.name)),
        );
        for p in &pools {
            output::println(&format!("  {:<13} {:<10} {}", p.pool, p.state, p.detail));
        }
    }
    Ok(if refused {
        ExitCode::from(horch::exit::REFUSED)
    } else {
        ExitCode::SUCCESS
    })
}
