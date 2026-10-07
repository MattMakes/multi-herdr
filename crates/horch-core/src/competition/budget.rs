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
//! `committed` (the coordinator's policy, the orchestrator's decision until
//! Spec B §budget says otherwise): each running candidate counts the max of
//! what it has spent and its projected cost ([`UsageMeter::projected`]),
//! minus what it has spent. The judge's
//! reserve is held until the judge completes; it is already in `limit`.
//! So a round whose projected spend crosses the limit stops launching
//! before its measured spend does.
//!
//! A hard ceiling of 0 means none is configured. Preflight (PRE-09) refuses
//! such a run, so this rule never sees it in a real round; if it does, the
//! limit is at most 0 and the action is CancelRunning.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::competition::config::{BudgetConfig, ExpectedTokens};
use crate::competition::preflight::{TokenEstimate, DEFAULT_TOKEN_ESTIMATE};
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
    /// The expected tokens of one candidate, by model: the estimates PRE-09
    /// resolved for the round. A model missing here projects
    /// `DEFAULT_TOKEN_ESTIMATE`.
    pub estimates: BTreeMap<String, TokenEstimate>,
}

impl Default for UsageMeter {
    fn default() -> Self {
        UsageMeter {
            prices: builtin_prices(),
            estimates: BTreeMap::new(),
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

    /// The projected cost of one candidate on `model`: its entry in
    /// `estimates` (what [`resolve_estimate`] gave PRE-09), else the default
    /// token estimate, at the table price. 0 when the model has no price;
    /// its measured spend still counts.
    pub fn projected(&self, model: &str) -> MicroUsd {
        let estimate = self
            .estimates
            .get(model)
            .copied()
            .unwrap_or(DEFAULT_TOKEN_ESTIMATE);
        estimate_cost(&self.prices, model, estimate)
            .map_or(MicroUsd(0), NanoUsd::to_micro_half_even)
    }
}

/// The cost of `estimate` on `model`: the one cost model that preflight
/// (PRE-09) and [`UsageMeter::projected`] share. `None` when the model has
/// no price, or a price is not a whole number of n$ per token.
pub(crate) fn estimate_cost(
    prices: &BTreeMap<String, Price>,
    model: &str,
    estimate: TokenEstimate,
) -> Option<NanoUsd> {
    let tokens = Tokens {
        input: estimate.input,
        cache_write_5m: estimate.cache_write_5m,
        cache_write_1h: estimate.cache_write_1h,
        cache_read: estimate.cache_read,
        output: estimate.output,
    };
    price_for(prices, model).and_then(|p| nano_cost(&p, &tokens))
}

/// Earlier candidates of a task needed on one model before PRE-09 trusts
/// their measured usage. 1 run can be an outlier; 3 show a range.
pub const MEASURED_MIN_RUNS: u32 = 3;
/// The most recent candidates of a task, per model, that the measured
/// estimate looks at. An older run can predate a change of the task's code.
pub const MEASURED_WINDOW: usize = 10;

/// Where PRE-09 found a candidate's expected tokens, in order of precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum EstimateSource {
    /// The plan's own per-label estimate (tests and callers that know better).
    Plan,
    /// `budget.expected_tokens.models.<model>`.
    ConfigModel,
    /// `budget.expected_tokens.all`.
    ConfigAll,
    /// The earlier candidates of the same task on the same model.
    Measured { runs: u32 },
    /// `DEFAULT_TOKEN_ESTIMATE`.
    Default,
}

impl fmt::Display for EstimateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EstimateSource::Plan => write!(f, "plan"),
            EstimateSource::ConfigModel => write!(f, "config model"),
            EstimateSource::ConfigAll => write!(f, "config all"),
            EstimateSource::Measured { runs } => write!(f, "measured {runs} runs"),
            EstimateSource::Default => write!(f, "default"),
        }
    }
}

/// The measured usage of one model on one task: the per-kind maximum over
/// the latest [`MEASURED_WINDOW`] runs, and how many runs that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasuredTokens {
    pub runs: u32,
    pub estimate: TokenEstimate,
}

/// The tokens PRE-09 expects of one candidate on `model`, and their source.
/// The order: the plan's per-label estimate, `budget.expected_tokens` (the
/// model entry, then `all`), the measured usage when it has at least
/// [`MEASURED_MIN_RUNS`] runs, then `DEFAULT_TOKEN_ESTIMATE`.
pub fn resolve_estimate(
    model: &str,
    per_label: Option<TokenEstimate>,
    configured: &ExpectedTokens,
    measured: &BTreeMap<String, MeasuredTokens>,
) -> (TokenEstimate, EstimateSource) {
    if let Some(e) = per_label {
        return (e, EstimateSource::Plan);
    }
    if let Some(e) = configured.models.get(model) {
        return (*e, EstimateSource::ConfigModel);
    }
    if let Some(e) = configured.all {
        return (e, EstimateSource::ConfigAll);
    }
    match measured.get(model) {
        Some(m) if m.runs >= MEASURED_MIN_RUNS => {
            (m.estimate, EstimateSource::Measured { runs: m.runs })
        }
        _ => (DEFAULT_TOKEN_ESTIMATE, EstimateSource::Default),
    }
}

/// The measured estimate per model from `runs`: (model, tokens) of each
/// earlier candidate of one task, oldest first. Runs with no token are
/// left out (no transcript was found).
///
/// The estimate is the maximum of each token kind over the latest
/// [`MEASURED_WINDOW`] runs, not the mean: PRE-09 checks a ceiling, and with
/// 3 to 10 runs a high percentile is the maximum. The LA runs of one small
/// task varied by 30 % in cache reads.
pub fn measured_from_runs(
    runs: impl IntoIterator<Item = (String, TokenEstimate)>,
) -> BTreeMap<String, MeasuredTokens> {
    let mut by_model: BTreeMap<String, Vec<TokenEstimate>> = BTreeMap::new();
    for (model, tokens) in runs {
        if !tokens.is_zero() {
            by_model.entry(model).or_default().push(tokens);
        }
    }
    by_model
        .into_iter()
        .map(|(model, all)| {
            let latest = &all[all.len().saturating_sub(MEASURED_WINDOW)..];
            let estimate = latest
                .iter()
                .fold(TokenEstimate::default(), |m, t| TokenEstimate {
                    input: m.input.max(t.input),
                    cache_write_5m: m.cache_write_5m.max(t.cache_write_5m),
                    cache_write_1h: m.cache_write_1h.max(t.cache_write_1h),
                    cache_read: m.cache_read.max(t.cache_read),
                    output: m.output.max(t.output),
                });
            let runs = latest.len() as u32;
            (model, MeasuredTokens { runs, estimate })
        })
        .collect()
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
