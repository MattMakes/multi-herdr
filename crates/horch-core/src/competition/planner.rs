//! The candidate planner (B3, CMP-06, EXP-02). Pure: the roster, the quota
//! view and the config arrive as input; no I/O, no clock, no environment.
//!
//! A round of `n` slots is planned in this order:
//!
//! 1. Baseline: `--baseline`, else the config's `baseline`, routed through
//!    the spawn gate exactly as `horch spawn` does (balance mode auto, no
//!    flags). A substitution runs the fallback; a refusal leaves no baseline
//!    slot, and [`RoundPlan::baseline`] says why.
//! 2. Diversity: greedy over the eligible set (`diversity.rs`).
//! 3. Exploration (when `n` ≥ 2): 1 entry drawn uniformly from the eligible
//!    entries not yet planned, with SplitMix64 seeded by `sha256(round_id)`.
//!    Its propensity is 1/k for k such entries; every other slot has 1.0.
//!
//! Every slot has a price. An eligible entry whose model the price table
//! does not know becomes [`ExclusionReason::Unpriced`] in the eligible set,
//! and an unpriced baseline is [`Baseline::Unusable`]: the budget cannot
//! count a candidate it cannot price.
//!
//! Labels follow slot order. The judge's anonymous shuffle comes later.

use std::collections::{BTreeMap, BTreeSet};

use crate::competition::budget::estimate_cost;
use crate::competition::config::DatasetConfig;
use crate::competition::diversity::{pick_diverse, Coverage};
use crate::competition::model::CandidateLabel;
use crate::competition::preflight::DEFAULT_TOKEN_ESTIMATE;
use crate::harness::HarnessKind;
use crate::ids::{ModelId, RoundId, TeammateName};
use crate::measure::digest::sha256_bytes;
use crate::measure::event::{CandidatePlanned, RoundCreated, SlotKind};
use crate::measure::testkit::{seed_from_digest, SplitMix64};
use crate::roster::{Roster, Teammate};
use crate::routing::decision::{decide, resolve, GateFlags, RoutingDecision};
use crate::routing::eligible::{
    roster_eligibility, EligibilityFilter, EligibleEntry, ExclusionReason, Verdict,
};
use crate::routing::policy::BalanceMode;
use crate::routing::quota::QuotaView;
use crate::teacher::TeacherRef;
use crate::usage::{builtin_prices, Price};

/// Labels are `A`, `B`, … in slot order (`CandidateLabel::from_index`).
/// The version names the export directory, so it is one path component
/// (dataset design 4.10.1).
pub const LABEL_POLICY_VERSION: &str = "slot-order-1";

/// Everything one round's plan depends on.
#[derive(Debug, Clone)]
pub struct PlanInput<'a> {
    pub round_id: &'a RoundId,
    /// The round's index in its experiment.
    pub index: u32,
    pub base_sha: &'a str,
    /// Slots to fill. Fewer are planned when the roster runs out.
    pub n: u32,
    /// `--baseline`; `None` falls back to `config.baseline`.
    pub baseline: Option<TeammateName>,
    pub roster: &'a Roster,
    pub view: &'a QuotaView,
    /// The caller's exclusions; `config.exclude` is added to them.
    pub filter: &'a EligibilityFilter,
    pub config: &'a DatasetConfig,
}

/// What became of the baseline slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Baseline {
    /// Neither `--baseline` nor the config names one.
    NotRequested,
    /// The gate's decision: spawn, substitute or refuse.
    Routed(RoutingDecision),
    /// The gate never ran: the teammate is unknown or cannot launch.
    Unusable { requested: String, reason: String },
}

impl Baseline {
    /// Why there is no baseline slot, or `None` when there is one.
    pub fn missing_reason(&self) -> Option<String> {
        match self {
            Baseline::NotRequested => Some("no baseline requested".to_string()),
            Baseline::Routed(RoutingDecision::Refuse {
                requested, reason, ..
            }) => Some(format!("{requested} refused: {reason}")),
            Baseline::Routed(_) => None,
            Baseline::Unusable { requested, reason } => Some(format!("{requested}: {reason}")),
        }
    }
}

/// One planned candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSlot {
    pub label: CandidateLabel,
    pub slot: SlotKind,
    /// The teammate that launches. For a substituted baseline, the fallback.
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    /// The chance this slot was planned: 1.0, or 1/k for exploration.
    pub propensity: f64,
    /// `<teammate>|<harness>|<model>|<effort or ->`.
    pub config_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoundPlan {
    pub index: u32,
    pub base_sha: String,
    pub candidates: Vec<PlannedSlot>,
    /// The whole roster with a verdict each, as the planner saw it.
    pub eligible_set: Vec<EligibleEntry>,
    /// Per label.
    pub propensities: BTreeMap<String, f64>,
    pub seed: u64,
    pub teacher: TeacherRef,
    pub label_policy_version: String,
    pub baseline: Baseline,
}

/// `<teammate>|<harness>|<model>|<effort or ->`.
pub fn config_id(
    teammate: &TeammateName,
    harness: HarnessKind,
    model: &ModelId,
    effort: Option<&str>,
) -> String {
    format!(
        "{teammate}|{}|{model}|{}",
        harness.as_str(),
        effort.unwrap_or("-")
    )
}

/// The exploration seed of a round: the first 8 bytes of `sha256(round_id)`.
pub fn round_seed(round_id: &RoundId) -> u64 {
    seed_from_digest(&sha256_bytes(round_id.as_str().as_bytes()))
}

/// Whether the budget can price a candidate on `model`.
fn priced(prices: &BTreeMap<String, Price>, model: &str) -> bool {
    estimate_cost(prices, model, DEFAULT_TOKEN_ESTIMATE).is_some()
}

/// A slot's launch settings, before its label.
struct Pick {
    slot: SlotKind,
    teammate: TeammateName,
    harness: HarnessKind,
    model: ModelId,
    effort: Option<String>,
    propensity: f64,
}

impl Pick {
    fn from_entry(e: &EligibleEntry, slot: SlotKind, propensity: f64) -> Option<Pick> {
        Some(Pick {
            slot,
            teammate: e.teammate.clone(),
            harness: e.harness,
            model: e.model.clone()?,
            effort: e.effort.clone(),
            propensity,
        })
    }
}

/// What the baseline routing found: the gate's decision, the slot it
/// launches (if any), and the names the other slots skip.
struct BaselineRoute {
    baseline: Baseline,
    pick: Option<Pick>,
    taken: Vec<String>,
}

impl BaselineRoute {
    fn not_requested() -> Self {
        BaselineRoute {
            baseline: Baseline::NotRequested,
            pick: None,
            taken: Vec::new(),
        }
    }

    fn unusable(requested: &str, reason: String, taken: Vec<String>) -> Self {
        BaselineRoute {
            baseline: Baseline::Unusable {
                requested: requested.to_string(),
                reason,
            },
            pick: None,
            taken,
        }
    }

    fn routed(decision: RoutingDecision, pick: Option<Pick>, taken: Vec<String>) -> Self {
        BaselineRoute {
            baseline: Baseline::Routed(decision),
            pick,
            taken,
        }
    }
}

/// The baseline: the gate's decision, and the slot it launches, if any.
/// The requested name is returned too, so the other slots skip it.
fn route_baseline(input: &PlanInput, prices: &BTreeMap<String, Price>) -> BaselineRoute {
    if let Some(name) = input.baseline.as_ref().or(input.config.baseline.as_ref()) {
        route_requested(input, prices, name.as_str())
    } else {
        BaselineRoute::not_requested()
    }
}

fn route_requested(
    input: &PlanInput,
    prices: &BTreeMap<String, Price>,
    name: &str,
) -> BaselineRoute {
    if let Some(req) = input.roster.get(name) {
        route_known(input, prices, req, name)
    } else {
        BaselineRoute::unusable(
            name,
            "not in the roster".to_string(),
            vec![name.to_string()],
        )
    }
}

/// Mode auto, no flags: what `horch spawn <name>` decides.
fn route_known(
    input: &PlanInput,
    prices: &BTreeMap<String, Price>,
    req: &Teammate,
    requested: &str,
) -> BaselineRoute {
    let decision = decide(
        req,
        input.roster,
        input.view,
        BalanceMode::Auto,
        GateFlags::default(),
    );
    let routed = RoutingDecision::from(&decision);
    if let Some(launch) = resolve(req, input.roster, &decision) {
        route_resolved(prices, routed, &launch, requested)
    } else {
        BaselineRoute::routed(routed, None, vec![requested.to_string()])
    }
}

/// The gate resolved the baseline to `launch`: its slot, unless the name or
/// the model cannot be planned.
fn route_resolved(
    prices: &BTreeMap<String, Price>,
    routed: RoutingDecision,
    launch: &Teammate,
    requested: &str,
) -> BaselineRoute {
    let resolved = if let RoutingDecision::Substitute { resolved, .. } = &routed {
        resolved.clone()
    } else {
        requested.to_string()
    };
    let taken = vec![requested.to_string(), resolved.clone()];
    match baseline_pick(launch, &resolved, prices) {
        Ok(pick) => BaselineRoute::routed(routed, Some(pick), taken),
        Err(reason) => BaselineRoute::unusable(requested, reason, taken),
    }
}

/// The slot a resolved baseline launches: `resolved`'s name with the merged
/// teammate's harness, model and effort.
fn baseline_pick(
    launch: &Teammate,
    resolved: &str,
    prices: &BTreeMap<String, Price>,
) -> Result<Pick, String> {
    Roster::is_routable(launch).map_err(|e| e.to_string())?;
    let model = ModelId::new(launch.model.as_deref().unwrap_or_default())
        .map_err(|_| "has no model of its own".to_string())?;
    let pick = Pick {
        slot: SlotKind::Baseline,
        teammate: TeammateName::new(resolved).map_err(|e| e.to_string())?,
        harness: launch.agent,
        model,
        effort: launch.effort.clone(),
        propensity: 1.0,
    };
    if priced(prices, pick.model.as_str()) {
        Ok(pick)
    } else {
        Err(format!("model {} has no price", pick.model))
    }
}

/// An eligible entry whose model the budget cannot price is excluded.
fn mark_unpriced(set: &mut [EligibleEntry], prices: &BTreeMap<String, Price>) {
    for e in set {
        if e.is_eligible()
            && e.model
                .as_ref()
                .is_some_and(|m| !priced(prices, m.as_str()))
        {
            e.verdict = Verdict::Excluded(ExclusionReason::Unpriced);
        }
    }
}

/// The baseline slot opens the round, when it launches and there is room.
fn seed_picks(n: usize, base_pick: Option<Pick>, covered: &mut Coverage) -> Vec<Pick> {
    base_pick
        .filter(|_| n > 0)
        .inspect(|p| covered.add(p.harness.as_str(), p.model.as_str(), p.effort.as_deref()))
        .into_iter()
        .collect()
}

/// One of `rest`, drawn by the round's seed; its propensity is 1/k.
fn exploration_pick(rest: &[EligibleEntry], seed: u64) -> Option<Pick> {
    let k = rest.len();
    if k == 0 {
        return None;
    }
    let i = SplitMix64::new(seed).below(k as u64) as usize;
    Pick::from_entry(&rest[i], SlotKind::Exploration, 1.0 / k as f64)
}

/// Plan one round. The same input always gives the same plan; a different
/// `round_id` changes only the exploration slot.
pub fn plan_round(input: &PlanInput) -> RoundPlan {
    let mut filter = input.filter.clone();
    filter.excluded.extend(input.config.exclude.iter().cloned());
    let prices = builtin_prices();
    let mut eligible_set = roster_eligibility(input.roster, input.view, &filter);
    mark_unpriced(&mut eligible_set, &prices);
    let seed = round_seed(input.round_id);
    let n = input.n as usize;

    let BaselineRoute {
        baseline,
        pick: base_pick,
        taken,
    } = route_baseline(input, &prices);
    let mut covered = Coverage::default();
    let mut picks = seed_picks(n, base_pick, &mut covered);

    // Eligible entries with a model, not yet planned.
    let mut planned: BTreeSet<String> = taken.into_iter().collect();
    let open = |planned: &BTreeSet<String>| -> Vec<EligibleEntry> {
        eligible_set
            .iter()
            .filter(|e| e.is_eligible() && e.model.is_some())
            .filter(|e| !planned.contains(e.teammate.as_str()))
            .cloned()
            .collect()
    };

    let explore = usize::from(n >= 2);
    let diverse = n.saturating_sub(picks.len() + explore);
    for e in pick_diverse(&open(&planned), &mut covered, diverse) {
        planned.insert(e.teammate.to_string());
        picks.extend(Pick::from_entry(&e, SlotKind::Diversity, 1.0));
    }

    if picks.len() < n {
        picks.extend(exploration_pick(&open(&planned), seed));
    }

    let candidates: Vec<PlannedSlot> = picks
        .into_iter()
        .enumerate()
        .map(|(i, p)| PlannedSlot {
            label: CandidateLabel::from_index(i),
            config_id: config_id(&p.teammate, p.harness, &p.model, p.effort.as_deref()),
            slot: p.slot,
            teammate: p.teammate,
            harness: p.harness,
            model: p.model,
            effort: p.effort,
            propensity: p.propensity,
        })
        .collect();
    let propensities = candidates
        .iter()
        .map(|c| (c.label.to_string(), c.propensity))
        .collect();
    RoundPlan {
        index: input.index,
        base_sha: input.base_sha.to_string(),
        candidates,
        eligible_set,
        propensities,
        seed,
        teacher: TeacherRef::none(),
        label_policy_version: LABEL_POLICY_VERSION.to_string(),
        baseline,
    }
}

/// The `round.created` payload of a plan.
pub fn round_created_payload(plan: &RoundPlan) -> RoundCreated {
    RoundCreated {
        index: plan.index,
        base_sha: plan.base_sha.clone(),
        labels: plan
            .candidates
            .iter()
            .map(|c| c.label.to_string())
            .collect(),
        eligible_set: plan.eligible_set.clone(),
        propensities: plan.propensities.clone(),
        teacher: plan.teacher.clone(),
        seed: plan.seed,
        label_policy_version: plan.label_policy_version.clone(),
    }
}

/// The `candidate.planned` payload of one slot.
pub fn candidate_planned_payload(slot: &PlannedSlot) -> CandidatePlanned {
    CandidatePlanned {
        label: slot.label.to_string(),
        teammate: slot.teammate.clone(),
        harness: slot.harness,
        model: slot.model.clone(),
        effort: slot.effort.clone(),
        slot: slot.slot,
        propensity: slot.propensity,
        config_id: slot.config_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::paths::component;
    use crate::routing::policy::Policy;
    use crate::routing::quota::QuotaFile;

    #[test]
    fn arc_28_a_seat_never_competes() {
        let mut roster = Roster::builtin().unwrap();
        let mut seat = roster.require("opus").unwrap().clone();
        seat.name = "fable-creative".into();
        seat.model = Some("fable".into());
        seat.fallbacks.clear();
        assert!(baseline_pick(&seat, "fable-creative", &builtin_prices()).is_err());
        roster.insert_for_test(seat);
        let view = QuotaView::new(
            QuotaFile::default(),
            crate::clock::parse("2026-10-08T00:00:00Z").unwrap(),
            Policy::default(),
            true,
        );
        let entries = roster_eligibility(&roster, &view, &EligibilityFilter::default());
        let seat = entries
            .iter()
            .find(|e| e.teammate.as_str() == "fable-creative")
            .unwrap();
        assert_eq!(
            seat.verdict,
            Verdict::Excluded(ExclusionReason::ReservedTier)
        );
    }

    /// The label policy version is `slot-order-1`, a valid path component
    /// (dataset design 4.10.1).
    #[test]
    fn label_policy_version_is_slot_order_1_and_path_safe() {
        assert_eq!(LABEL_POLICY_VERSION, "slot-order-1");
        assert_eq!(
            component("label policy version", LABEL_POLICY_VERSION).unwrap(),
            LABEL_POLICY_VERSION
        );
    }
}
