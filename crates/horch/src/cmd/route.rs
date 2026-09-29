//! `horch route <teammate>` - what the spawn gate would do right now, with no
//! spawn (BAL-06). The same `decide()` the gate calls, on the same readings.

use std::process::ExitCode;

use anyhow::Result;
use horch_core::balance_policy::{self, Decision, GateFlags};
use horch_core::clock;
use horch_core::ledger::state_root;
use horch_core::policy::Policy;
use horch_core::quota;
use horch_core::teammates::Roster;
use serde_json::json;

use crate::output;

pub fn route(name: &str, json_out: bool, exact: bool, force: bool) -> Result<ExitCode> {
    let roster = Roster::load_with(std::env::var("HORCH_TEAMMATES_DIR").ok().as_deref())?;
    let teammate = roster.require(name)?.clone();
    let root = state_root();
    let policy = Policy::load(&root)?;
    let view = quota::current_view(&root, clock::now(), &policy, true)?;
    let decision = balance_policy::decide(
        &teammate,
        &roster,
        &view,
        policy.balance_mode,
        GateFlags { exact, force },
    );

    // Every pool the decision could touch: the teammate's and its fallbacks'.
    let mut pools = Vec::new();
    for t in
        std::iter::once(&teammate).chain(teammate.fallbacks.iter().filter_map(|f| roster.get(f)))
    {
        let a = view.assess(t.agent.as_str(), t.model.as_deref().unwrap_or_default());
        if !pools
            .iter()
            .any(|p: &balance_policy::PoolLine| p.pool == a.pool)
        {
            pools.push(balance_policy::PoolLine {
                pool: a.pool.clone(),
                state: a.state.to_string(),
                detail: balance_policy::summary(&a, &view),
            });
        }
    }
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
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}
