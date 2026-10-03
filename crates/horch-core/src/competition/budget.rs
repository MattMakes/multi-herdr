//! The live budget rule (B3, CMP-10). Pure: the coordinator passes in what
//! the round has spent and what its running candidates are expected to
//! spend, and applies the action.
//!
//! The candidates may use the hard ceiling minus the judge reserve; the
//! reserve keeps the judge affordable. With `limit = hard − judge_reserve`:
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

use crate::competition::config::BudgetConfig;
use crate::usage::money::MicroUsd;

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
