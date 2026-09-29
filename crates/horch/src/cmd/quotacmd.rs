//! `horch quota` - the usage pools, and what state each one is in (SPC-05).
//!
//! Plain `horch quota` never probes. `--refresh` probes when no live
//! collector holds the lock and the last probe is older than
//! `probe_on_demand_age_min` (QUO-07); with a live collector it reads what
//! the collector last wrote.

use anyhow::Result;
use horch_core::clock;
use horch_core::ledger::state_root;
use horch_core::policy::Policy;
use horch_core::quota::{self, QuotaFile, QuotaView};

use crate::output;

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

pub fn quota(json: bool, refresh: bool) -> Result<()> {
    let root = state_root();
    let policy = Policy::load(&root)?;
    let view = quota::current_view(&root, clock::now(), &policy, refresh)?;
    if json {
        output::println(&serde_json::to_string_pretty(&assessed(&view))?);
    } else {
        let file = assessed(&view);
        output::print(&super::telemetry::pool_table(&file.pools, view.now, 100).join("\n"));
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
