//! The eligible set (ARC-13): which teammates a spawn may run as, and why
//! each other one is excluded. Pure: no I/O, no clock read, no environment;
//! the time arrives inside the [`QuotaView`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::harness::HarnessKind;
use crate::ids::{ModelId, TeammateName};
use crate::roster::{effort_problem, Roster, Teammate};
use crate::routing::quota::{QuotaView, State, POOL_UNKNOWN};

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
    /// The price table has no price for the model, so no budget can count
    /// it. The planner applies this rule.
    Unpriced,
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
            ExclusionReason::Unpriced => "unpriced",
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
    t.trains_on_input || t.agent == HarnessKind::OpenCode
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
    } else if Roster::is_routable(f).is_err() {
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
    } else if t.agent == HarnessKind::None {
        Some(ExclusionReason::AgentNone)
    } else if Roster::is_routable(t).is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::policy::Policy;
    use crate::routing::quota::QuotaFile;

    fn view(name: &str) -> QuotaView {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/telemetry/quota")
            .join(format!("{name}.json"));
        QuotaView::new(
            QuotaFile::read(&path).unwrap(),
            crate::clock::parse("2026-09-28T18:00:00Z").unwrap(),
            Policy::default(),
            true,
        )
    }

    /// One way to break one `roster_rule` rule, applied to a clean teammate.
    struct Breaker {
        reason: ExclusionReason,
        teammate: fn(&mut Teammate),
        filter: fn(&mut EligibilityFilter),
        pool: bool,
    }

    const NOTHING_T: fn(&mut Teammate) = |_| {};
    const NOTHING_F: fn(&mut EligibilityFilter) = |_| {};

    /// In the order `roster_rule` applies the rules.
    fn breakers() -> Vec<Breaker> {
        use ExclusionReason as R;
        let b = |reason, teammate, filter, pool| Breaker {
            reason,
            teammate,
            filter,
            pool,
        };
        vec![
            b(
                R::ExcludedByConfig,
                NOTHING_T,
                |f| {
                    f.excluded.insert(TeammateName::new("sonnet").unwrap());
                },
                false,
            ),
            b(R::Hidden, |t| t.hidden = true, NOTHING_F, false),
            b(R::AgentNone, |t| t.agent = HarnessKind::None, NOTHING_F, false),
            b(
                R::ReservedTier,
                |t| t.model = Some("gpt-6-astra".into()),
                NOTHING_F,
                false,
            ),
            b(R::Unspawnable, |t| t.model = None, NOTHING_F, false),
            b(
                R::EffortUnsupported,
                |t| t.effort = Some("bogus".into()),
                NOTHING_F,
                false,
            ),
            b(R::TrainsOnInput, |t| t.trains_on_input = true, NOTHING_F, false),
            b(
                R::HarnessUnavailable,
                NOTHING_T,
                |f| f.available_harnesses = Some(Vec::new()),
                false,
            ),
            b(R::PoolBlocked, NOTHING_T, NOTHING_F, true),
            b(
                R::OverBudget,
                NOTHING_T,
                |f| {
                    f.max_cost_microusd = Some(1);
                    f.estimated_cost_microusd
                        .insert(TeammateName::new("sonnet").unwrap(), 2);
                },
                false,
            ),
        ]
    }

    /// The rule a clean `sonnet` breaks after the chosen breakers apply.
    fn rule_after(chosen: &[&Breaker]) -> Option<ExclusionReason> {
        let mut t = Roster::builtin().unwrap().require("sonnet").unwrap().clone();
        let mut filter = EligibilityFilter::default();
        let mut blocked = false;
        for b in chosen {
            (b.teammate)(&mut t);
            (b.filter)(&mut filter);
            blocked |= b.pool;
        }
        let view = view(if blocked { "all-exhausted" } else { "all-ok" });
        let e = entry(&t, &view, None).unwrap();
        roster_rule(&t, &e, &filter)
    }

    #[test]
    fn a_clean_teammate_breaks_no_rule() {
        assert_eq!(rule_after(&[]), None);
    }

    #[test]
    fn each_breaker_trips_only_its_own_rule() {
        let all = breakers();
        for b in &all {
            assert_eq!(rule_after(&[b]), Some(b.reason), "{}", b.reason);
        }
    }

    /// A teammate that breaks 2 rules gets the reason of the earlier rule.
    #[test]
    fn the_earlier_rule_names_the_reason() {
        use ExclusionReason as R;
        let all = breakers();
        for (i, first) in all.iter().enumerate() {
            for second in &all[i + 1..] {
                // Clearing the model also clears a reserved model.
                if (first.reason, second.reason) == (R::ReservedTier, R::Unspawnable) {
                    continue;
                }
                assert_eq!(
                    rule_after(&[first, second]),
                    Some(first.reason),
                    "{} before {}",
                    first.reason,
                    second.reason
                );
            }
        }
    }

    #[test]
    fn every_reason_has_its_serde_name() {
        let names = [
            ExclusionReason::NotInRoster,
            ExclusionReason::Hidden,
            ExclusionReason::ReservedTier,
            ExclusionReason::TrainsOnInput,
            ExclusionReason::PoolBlocked,
            ExclusionReason::Unspawnable,
            ExclusionReason::EffortUnsupported,
            ExclusionReason::HarnessUnavailable,
            ExclusionReason::AgentNone,
            ExclusionReason::ExcludedByConfig,
            ExclusionReason::OverBudget,
            ExclusionReason::Unpriced,
        ];
        assert_eq!(names.len(), 12);
        for r in names {
            let json = serde_json::to_string(&r).unwrap();
            assert_eq!(r.as_str(), json.trim_matches('"'));
        }
    }
}
