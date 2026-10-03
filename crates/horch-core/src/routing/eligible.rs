//! The eligible set (ARC-13): which teammates a spawn may run as, and why
//! each other one is excluded. Pure: no I/O, no clock read, no environment;
//! the time arrives inside the [`QuotaView`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::harness::HarnessKind;
use crate::ids::{ModelId, TeammateName};
use crate::routing::quota::{QuotaView, State, POOL_UNKNOWN};
use crate::teammates::{effort_problem, Agent, Roster, Teammate};

/// Why a teammate is not in the eligible set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// A `fallbacks:` entry names no teammate.
    NotInRoster,
    /// Hidden teammates are spawned by name only, never chosen.
    Hidden,
    /// The model is an orchestrator tier.
    ReservedTier,
    /// The provider trains on its input (BAL-09).
    TrainsOnInput,
    /// The teammate's pool is exhausted, broken or cooling.
    PoolBlocked,
    /// The teammate has no model of its own (a recipe supplies one).
    Unspawnable,
    /// The teammate's effort is not a level its agent and model take.
    EffortUnsupported,
    /// The caller says this harness is not installed.
    HarnessUnavailable,
    /// The `none` agent: the smoke fake, never a real worker.
    AgentNone,
    /// The caller's config excludes the teammate.
    ExcludedByConfig,
    /// The caller's cost estimate exceeds its budget.
    OverBudget,
}

impl ExclusionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ExclusionReason::NotInRoster => "not_in_roster",
            ExclusionReason::Hidden => "hidden",
            ExclusionReason::ReservedTier => "reserved_tier",
            ExclusionReason::TrainsOnInput => "trains_on_input",
            ExclusionReason::PoolBlocked => "pool_blocked",
            ExclusionReason::Unspawnable => "unspawnable",
            ExclusionReason::EffortUnsupported => "effort_unsupported",
            ExclusionReason::HarnessUnavailable => "harness_unavailable",
            ExclusionReason::AgentNone => "agent_none",
            ExclusionReason::ExcludedByConfig => "excluded_by_config",
            ExclusionReason::OverBudget => "over_budget",
        }
    }
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `{"verdict":"eligible"}` or `{"verdict":"excluded","reason":"hidden"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", content = "reason", rename_all = "snake_case")]
pub enum Verdict {
    Eligible,
    Excluded(ExclusionReason),
}

impl Verdict {
    pub fn is_eligible(&self) -> bool {
        *self == Verdict::Eligible
    }
}

/// One teammate in an eligible set, with its pool and its verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibleEntry {
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    /// `None` when the teammate has no model of its own, or is not in the
    /// roster.
    pub model: Option<ModelId>,
    pub effort: Option<String>,
    /// `None` = the requested teammate; `Some(i)` = `fallbacks[i]`.
    pub fallback_index: Option<u32>,
    pub pool: String,
    pub pool_state: State,
    #[serde(flatten)]
    pub verdict: Verdict,
}

impl EligibleEntry {
    pub fn is_eligible(&self) -> bool {
        self.verdict.is_eligible()
    }
}

/// The caller's exclusions (B3 config). Empty excludes nothing.
#[derive(Debug, Clone, Default)]
pub struct EligibilityFilter {
    pub excluded: BTreeSet<TeammateName>,
    /// The budget per spawn. A teammate whose estimate exceeds it is
    /// [`ExclusionReason::OverBudget`].
    pub max_cost_microusd: Option<i64>,
    /// The caller's cost estimate per teammate. No estimate is never over
    /// budget.
    pub estimated_cost_microusd: BTreeMap<TeammateName, i64>,
    /// `Some`: only these harnesses are installed.
    pub available_harnesses: Option<Vec<HarnessKind>>,
}

/// Whether a teammate's provider trains on its input: every OpenCode one
/// (BAL-09). Never chosen automatically.
pub fn trains_on_input(t: &Teammate) -> bool {
    t.trains_on_input || t.agent == Agent::OpenCode
}

/// An entry for a roster teammate, with its pool assessed.
fn entry(t: &Teammate, view: &QuotaView, fallback_index: Option<u32>) -> Option<EligibleEntry> {
    let model = t.model.as_deref().unwrap_or_default();
    let a = view.assess(t.agent.as_str(), model);
    Some(EligibleEntry {
        teammate: TeammateName::new(t.name.as_str()).ok()?,
        harness: t.agent,
        model: ModelId::new(model).ok(),
        effort: t.effort.clone(),
        fallback_index,
        pool: a.pool,
        pool_state: a.state,
        verdict: Verdict::Eligible,
    })
}

/// The first rule a fallback breaks, in the order the gate always applied
/// them.
fn fallback_rule(f: &Teammate) -> Option<ExclusionReason> {
    if f.hidden {
        Some(ExclusionReason::Hidden)
    } else if Roster::is_spawnable(f).is_err() {
        Some(ExclusionReason::ReservedTier)
    } else if trains_on_input(f) {
        Some(ExclusionReason::TrainsOnInput)
    } else {
        None
    }
}

fn verdict(reason: Option<ExclusionReason>) -> Verdict {
    reason.map_or(Verdict::Eligible, Verdict::Excluded)
}

/// The requested teammate, then its fallbacks, in `fallbacks:` order.
///
/// The fallbacks the gate may use are exactly the eligible entries with a
/// `fallback_index`: the same order and the same drop rules as the gate's
/// candidate list before A5. Pool states are reported, not judged; the gate
/// decides on them. The requested teammate is spawned by name, so only the
/// reserved-tier rule applies to it.
pub fn eligible_fallbacks(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<EligibleEntry> {
    let mut out = Vec::new();
    if let Some(mut e) = entry(req, view, None) {
        e.verdict = verdict(
            Roster::is_spawnable(req)
                .is_err()
                .then_some(ExclusionReason::ReservedTier),
        );
        out.push(e);
    }
    for (i, name) in req.fallbacks.iter().enumerate() {
        let index = Some(i as u32);
        match roster.get(name) {
            Some(f) => {
                if let Some(mut e) = entry(f, view, index) {
                    e.verdict = verdict(fallback_rule(f));
                    out.push(e);
                }
            }
            None => {
                let Ok(teammate) = TeammateName::new(name.as_str()) else {
                    continue;
                };
                out.push(EligibleEntry {
                    teammate,
                    harness: HarnessKind::None,
                    model: None,
                    effort: None,
                    fallback_index: index,
                    pool: POOL_UNKNOWN.to_string(),
                    pool_state: State::Unknown,
                    verdict: Verdict::Excluded(ExclusionReason::NotInRoster),
                });
            }
        }
    }
    out
}

/// The first rule a roster teammate breaks for a planner that chooses on
/// its own (B3).
fn roster_rule(
    t: &Teammate,
    e: &EligibleEntry,
    filter: &EligibilityFilter,
) -> Option<ExclusionReason> {
    let model = t.model.as_deref().unwrap_or_default();
    if filter.excluded.contains(&e.teammate) {
        Some(ExclusionReason::ExcludedByConfig)
    } else if t.hidden {
        Some(ExclusionReason::Hidden)
    } else if t.agent == Agent::None {
        Some(ExclusionReason::AgentNone)
    } else if Roster::is_spawnable(t).is_err() {
        Some(ExclusionReason::ReservedTier)
    } else if model.is_empty() {
        Some(ExclusionReason::Unspawnable)
    } else if t
        .effort
        .as_deref()
        .is_some_and(|eff| effort_problem(t.agent, Some(model), eff).is_some())
    {
        Some(ExclusionReason::EffortUnsupported)
    } else if trains_on_input(t) {
        Some(ExclusionReason::TrainsOnInput)
    } else if filter
        .available_harnesses
        .as_ref()
        .is_some_and(|h| !h.contains(&t.agent))
    {
        Some(ExclusionReason::HarnessUnavailable)
    } else if e.pool_state.blocks() {
        Some(ExclusionReason::PoolBlocked)
    } else if filter.max_cost_microusd.is_some_and(|max| {
        filter
            .estimated_cost_microusd
            .get(&e.teammate)
            .is_some_and(|cost| *cost > max)
    }) {
        Some(ExclusionReason::OverBudget)
    } else {
        None
    }
}

/// Every roster teammate, sorted by name, each with a verdict.
pub fn roster_eligibility(
    roster: &Roster,
    view: &QuotaView,
    filter: &EligibilityFilter,
) -> Vec<EligibleEntry> {
    roster
        .names()
        .into_iter()
        .filter_map(|name| roster.get(name))
        .filter_map(|t| {
            let mut e = entry(t, view, None)?;
            e.verdict = verdict(roster_rule(t, &e, filter));
            Some(e)
        })
        .collect()
}
