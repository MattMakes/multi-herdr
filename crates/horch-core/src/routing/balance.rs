//! Gate glue that `horch route` shows: every pool a decision could touch.
//! Named apart from the crate's `balance.rs`, which evens grid columns.

use crate::roster::{Roster, Teammate};
use crate::routing::decision::{summary, PoolLine};
use crate::routing::quota::QuotaView;

/// The teammate's pool, then each fallback's, once each, in `fallbacks:`
/// order. Missing fallbacks are skipped.
pub fn touched_pools(teammate: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<PoolLine> {
    let mut pools: Vec<PoolLine> = Vec::new();
    for t in
        std::iter::once(teammate).chain(teammate.fallbacks.iter().filter_map(|f| roster.get(f)))
    {
        let a = view.assess(t.agent.as_str(), t.model.as_deref().unwrap_or_default());
        if !pools.iter().any(|p| p.pool == a.pool) {
            pools.push(PoolLine {
                pool: a.pool.clone(),
                state: a.state.to_string(),
                detail: summary(&a, view),
            });
        }
    }
    pools
}
