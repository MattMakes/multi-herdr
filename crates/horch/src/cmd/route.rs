//! `horch route <teammate>` - what the spawn gate would do right now, with no
//! spawn (BAL-06). The same `decide()` the gate calls, on the same readings.

use std::process::ExitCode;

use anyhow::Result;
use horch_core::clock;
use horch_core::routing::balance::touched_pools;
use horch_core::routing::decision::{self, Decision, GateFlags, PoolLine};
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
        let mode = policy.balance_mode.as_str();
        let view = decision_json(&decision, &teammate.name, mode, &pools);
        output::println(&serde_json::to_string_pretty(&view)?);
    } else {
        print_decision(&decision, &teammate.name, &pools);
    }
    Ok(if refused {
        ExitCode::from(horch::exit::REFUSED)
    } else {
        ExitCode::SUCCESS
    })
}

/// The `--json` view of a decision.
fn decision_json(
    decision: &Decision,
    teammate: &str,
    mode: &str,
    pools: &[PoolLine],
) -> serde_json::Value {
    let (kind, via, reason) = match decision {
        Decision::Spawn { note, .. } => ("spawn", None, note.clone()),
        Decision::Substitute { via, reason, .. } => {
            ("substitute", Some(via.clone()), Some(reason.clone()))
        }
        Decision::Refuse { reason, .. } => ("refuse", None, Some(reason.clone())),
    };
    json!({
        "decision": kind,
        "teammate": teammate,
        "via": via,
        "reason": reason,
        "line": decision.line(),
        "mode": mode,
        "pools": pools,
    })
}

/// The text view: the decision line, then one line per pool.
fn print_decision(decision: &Decision, teammate: &str, pools: &[PoolLine]) {
    output::println(
        &decision
            .line()
            .unwrap_or_else(|| format!("SPAWN: {teammate} runs as itself.")),
    );
    for p in pools {
        output::println(&format!("  {:<13} {:<10} {}", p.pool, p.state, p.detail));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pool() -> PoolLine {
        PoolLine {
            pool: "claude".into(),
            state: "ok".into(),
            detail: "plenty".into(),
        }
    }

    #[test]
    fn the_json_view_names_each_decision_kind() {
        let spawn = Decision::Spawn {
            teammate: "opus".into(),
            note: Some("careful".into()),
        };
        assert_eq!(
            decision_json(&spawn, "opus", "auto", &[pool()]),
            json!({
                "decision": "spawn", "teammate": "opus", "via": null,
                "reason": "careful", "line": "NOTE: careful", "mode": "auto",
                "pools": [{"pool": "claude", "state": "ok", "detail": "plenty"}],
            })
        );
        let substitute = Decision::Substitute {
            original: "opus".into(),
            via: "sonnet".into(),
            reason: "quota".into(),
        };
        let view = decision_json(&substitute, "opus", "auto", &[]);
        assert_eq!(
            (&view["decision"], &view["via"], &view["reason"]),
            (&json!("substitute"), &json!("sonnet"), &json!("quota"))
        );
        assert_eq!(
            view["line"],
            "SUBSTITUTED: opus runs on sonnet. Reason: quota."
        );
        let refuse = Decision::Refuse {
            teammate: "opus".into(),
            reason: "empty".into(),
            pools: vec![],
        };
        let view = decision_json(&refuse, "opus", "exact", &[]);
        assert_eq!(
            (
                &view["decision"],
                &view["via"],
                &view["reason"],
                &view["mode"]
            ),
            (
                &json!("refuse"),
                &json!(null),
                &json!("empty"),
                &json!("exact")
            )
        );
    }
}
