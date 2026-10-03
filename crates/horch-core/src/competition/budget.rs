//! The live budget rule (B3, CMP-10). Pure: the coordinator passes in what
//! the round has spent and what its running candidates are expected to
//! spend, and applies the action.
//!
//! The candidates may use the hard ceiling minus the judge reserve; the
//! reserve keeps the judge affordable. [`UsageMeter`] gives `spent`. With `limit = hard − judge_reserve`:
//!
//! - `spent ≥ limit`: [`BudgetAction::CancelRunning`]. Cancel every running
//!   candidate, keep its worktree and data, launch nothing.
//! - `spent + committed ≥ limit`: [`BudgetAction::StopLaunches`]. Running
//!   candidates go on; no new candidate starts.
//! - otherwise: [`BudgetAction::Continue`].
//!
//! A hard ceiling of 0 means none is configured. Preflight (PRE-09) refuses
//! such a run, so this rule never sees it in a real round; if it does, the
//! limit is at most 0 and the action is CancelRunning.

use std::collections::BTreeMap;

use crate::competition::config::BudgetConfig;
use crate::usage::money::{nano_per_token, CostSource, MicroUsd, NanoUsd};
use crate::usage::{builtin_prices, price_for, Price, Tokens, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAction {
    Continue,
    StopLaunches,
    CancelRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetPolicy;

impl BudgetPolicy {
    /// What the coordinator does at this spend. `spent` is what the round
    /// has used; `committed` is the expected further spend of candidates
    /// already running or about to launch.
    pub fn check(spent: MicroUsd, committed: MicroUsd, config: &BudgetConfig) -> BudgetAction {
        let limit = config
            .hard_usd_micro
            .saturating_sub(config.judge_reserve_usd_micro);
        if spent.0 >= limit {
            BudgetAction::CancelRunning
        } else if spent.0.saturating_add(committed.0) >= limit {
            BudgetAction::StopLaunches
        } else {
            BudgetAction::Continue
        }
    }
}

/// What one candidate session has used so far, read from its transcript.
pub(crate) trait UsageSource {
    /// The session's usage, or `None` when nothing is readable yet (no
    /// session id, no transcript).
    fn usage(&self, harness: &str, session_id: Option<&str>) -> Option<Usage>;
}

/// The live meter (CMP-10): prices what each candidate session has used,
/// in whole µ$, with the compiled-in price table. Exact: every price is
/// turned into an integer n$ per token, and the sum is rounded once.
#[derive(Debug, Clone)]
pub struct UsageMeter {
    pub prices: BTreeMap<String, Price>,
}

impl Default for UsageMeter {
    fn default() -> Self {
        UsageMeter {
            prices: builtin_prices(),
        }
    }
}

impl UsageMeter {
    /// The cost of `usage`. A model without a price, or a price that is not
    /// a whole number of n$ per token, counts 0 and makes the source
    /// `Unpriced`; the tokens still count.
    pub fn price(&self, usage: &Usage) -> (MicroUsd, CostSource) {
        let mut total = NanoUsd(0);
        let mut priced = true;
        for (model, tokens) in &usage.by_model {
            match price_for(&self.prices, model).and_then(|p| nano_cost(&p, tokens)) {
                Some(n) => total += n,
                None => priced = false,
            }
        }
        let source = if priced {
            CostSource::PriceTable {
                date: PRICE_TABLE_DATE.to_string(),
            }
        } else {
            CostSource::Unpriced
        };
        (total.to_micro_half_even(), source)
    }
}

/// The date of [`builtin_prices`].
pub(crate) const PRICE_TABLE_DATE: &str = "2026-09-24";

/// `t` at `p`, exactly, or `None` when a price is not a whole n$ per token.
fn nano_cost(p: &Price, t: &Tokens) -> Option<NanoUsd> {
    let w5 = p.cache_write_5m.unwrap_or(p.input * 1.25);
    let w1h = p.cache_write_1h.unwrap_or(p.input * 2.0);
    let mut n = NanoUsd(0);
    for (count, per_mtok) in [
        (t.input, p.input),
        (t.cache_write_5m, w5),
        (t.cache_write_1h, w1h),
        (t.cache_read, p.cache_read),
        (t.output, p.output),
    ] {
        n.add_tokens(count, nano_per_token(per_mtok).ok()?);
    }
    Some(n)
}
