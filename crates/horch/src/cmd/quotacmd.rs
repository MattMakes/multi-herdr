//! `horch quota` - the usage pools, and what state each one is in (SPC-05).
//!
//! Plain `horch quota` never probes. `--refresh` probes when no live
//! collector holds the lock and the last probe is older than
//! `probe_on_demand_age_min` (QUO-07); with a live collector it reads what
//! the collector last wrote.

use anyhow::Result;
use horch_core::clock;
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::routing::snapshot::{self, QuotaEnv};
use horch_core::runtime::RuntimeContext;

use crate::output;

/// `policy.json` with `HORCH_BALANCE` (`settings.balance_override`) applied.
pub fn load_policy(ctx: &RuntimeContext, root: &std::path::Path) -> Result<Policy> {
    Policy::load(root, ctx.settings.balance_override.as_deref())
}

/// `HORCH_QUOTA_FILE`, `HORCH_PROBE_TIMEOUT_MS`, the temp dir and the probe
/// programs.
pub fn quota_env(ctx: &RuntimeContext) -> QuotaEnv {
    QuotaEnv::from_context(ctx)
}

/// `quota.json` with each pool's state assessed at `view.now`.
pub fn assessed(view: &QuotaView) -> QuotaFile {
    let mut file = view.file.clone();
    file.written_at = file.written_at.or_else(|| Some(clock::stamp(view.now)));
    for (name, reading) in file.pools.iter_mut() {
        let a = view.assess_pool(name, None, None);
        reading.state = a.state.as_str().to_string();
        reading.reason = Some(a.reason);
    }
    file
}

pub fn quota(ctx: &RuntimeContext, json: bool, refresh: bool) -> Result<()> {
    let root = ctx.paths.state_root.clone();
    let policy = load_policy(ctx, &root)?;
    let view = snapshot::obtain(&root, clock::now(), &policy, refresh, &quota_env(ctx))?;
    if json {
        output::println(&serde_json::to_string_pretty(&assessed(&view))?);
    } else {
        let file = assessed(&view);
        output::print(&super::telemetry::pool_table(&file.pools, view.now, 100, &[]).join("\n"));
        output::println("");
        // A fallback reading says how old it is (QUO-03).
        for (name, r) in &file.pools {
            if let (Some(src), Some(at)) = (r.source.as_deref(), r.observed_at.as_deref()) {
                if src == "rollout" {
                    let age = clock::parse(at)
                        .map(|t| format!(", {} min old", (view.now - t).num_minutes().max(0)))
                        .unwrap_or_default();
                    output::println(&format!("{name}: from a rollout written {at}{age}"));
                }
            }
            if let Some(e) = &r.error {
                output::println(&format!("{name}: {e}"));
            }
        }
    }
    Ok(())
}
