use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::harness::HarnessKind;
use crate::ids::{ModelId, TeammateName};
use crate::roster::{effort_problem, Roster, Teammate};
use crate::routing::quota::{QuotaView, State, POOL_UNKNOWN};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    NotInRoster,
    Hidden,
    ReservedTier,
    TrainsOnInput,
    PoolBlocked,
    Unspawnable,
    EffortUnsupported,
    HarnessUnavailable,
    AgentNone,
    ExcludedByConfig,
    OverBudget,
    Unpriced,
}

const NAMES: [&str; 12] = [
    "not_in_roster",
    "hidden",
    "reserved_tier",
    "trains_on_input",
    "pool_blocked",
    "unspawnable",
    "effort_unsupported",
    "harness_unavailable",
    "agent_none",
    "excluded_by_config",
    "over_budget",
    "unpriced",
];

impl ExclusionReason {
    pub fn as_str(self) -> &'static str {
        NAMES[self as usize]
    }
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibleEntry {
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: Option<ModelId>,
    pub effort: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct EligibilityFilter {
    pub excluded: BTreeSet<TeammateName>,
    pub max_cost_microusd: Option<i64>,
    pub estimated_cost_microusd: BTreeMap<TeammateName, i64>,
    pub available_harnesses: Option<Vec<HarnessKind>>,
}

pub fn trains_on_input(t: &Teammate) -> bool {
    t.trains_on_input || t.agent == HarnessKind::OpenCode
}

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

fn effort_unsupported(t: &Teammate, model: &str) -> bool {
    t.effort
        .as_deref()
        .is_some_and(|eff| effort_problem(t.agent, Some(model), eff).is_some())
}

fn harness_unavailable(t: &Teammate, filter: &EligibilityFilter) -> bool {
    filter
        .available_harnesses
        .as_ref()
        .is_some_and(|h| !h.contains(&t.agent))
}

fn over_budget(e: &EligibleEntry, filter: &EligibilityFilter) -> bool {
    filter.max_cost_microusd.is_some_and(|max| {
        filter
            .estimated_cost_microusd
            .get(&e.teammate)
            .is_some_and(|cost| *cost > max)
    })
}

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
    } else if effort_unsupported(t, model) {
        Some(ExclusionReason::EffortUnsupported)
    } else if trains_on_input(t) {
        Some(ExclusionReason::TrainsOnInput)
    } else {
        availability_rule(t, e, filter)
    }
}

fn availability_rule(
    t: &Teammate,
    e: &EligibleEntry,
    filter: &EligibilityFilter,
) -> Option<ExclusionReason> {
    if harness_unavailable(t, filter) {
        Some(ExclusionReason::HarnessUnavailable)
    } else if e.pool_state.blocks() {
        Some(ExclusionReason::PoolBlocked)
    } else if over_budget(e, filter) {
        Some(ExclusionReason::OverBudget)
    } else {
        None
    }
}

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

    struct Breaker {
        reason: ExclusionReason,
        teammate: fn(&mut Teammate),
        filter: fn(&mut EligibilityFilter),
        pool: bool,
    }

    const NOTHING_T: fn(&mut Teammate) = |_| {};
    const NOTHING_F: fn(&mut EligibilityFilter) = |_| {};

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
            b(
                R::AgentNone,
                |t| t.agent = HarnessKind::None,
                NOTHING_F,
                false,
            ),
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
            b(
                R::TrainsOnInput,
                |t| t.trains_on_input = true,
                NOTHING_F,
                false,
            ),
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

    fn rule_after(chosen: &[&Breaker]) -> Option<ExclusionReason> {
        let mut t = Roster::builtin()
            .unwrap()
            .require("sonnet")
            .unwrap()
            .clone();
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

    #[test]
    fn the_earlier_rule_names_the_reason() {
        use ExclusionReason as R;
        let all = breakers();
        for (i, first) in all.iter().enumerate() {
            for second in &all[i + 1..] {
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
        assert_eq!(names.len(), NAMES.len());
        for r in names {
            let json = serde_json::to_string(&r).unwrap();
            assert_eq!(r.as_str(), json.trim_matches('"'));
        }
    }
}
