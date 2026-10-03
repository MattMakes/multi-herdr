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
//! Labels follow slot order. The judge's anonymous shuffle comes later.

use std::collections::{BTreeMap, BTreeSet};

use crate::competition::config::DatasetConfig;
use crate::competition::diversity::{pick_diverse, Coverage};
use crate::competition::model::CandidateLabel;
use crate::harness::HarnessKind;
use crate::ids::{ModelId, RoundId, TeammateName};
use crate::measure::digest::sha256_bytes;
use crate::measure::event::{CandidatePlanned, RoundCreated, SlotKind};
use crate::measure::testkit::{seed_from_digest, SplitMix64};
use crate::roster::{Roster, Teammate};
use crate::routing::decision::{decide, resolve, GateFlags, RoutingDecision};
use crate::routing::eligible::{roster_eligibility, EligibilityFilter, EligibleEntry};
use crate::routing::policy::BalanceMode;
use crate::routing::quota::QuotaView;
use crate::teacher::TeacherRef;

/// Labels are `A`, `B`, … in slot order (`CandidateLabel::from_index`).
///
/// SPEC-TODO(Spec B label policy): the version string is not in the master
/// plan; it names the export directory, so it must stay path-safe.
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

/// The baseline: the gate's decision, and the slot it launches, if any.
/// The requested name is returned too, so the other slots skip it.
fn route_baseline(input: &PlanInput) -> (Baseline, Option<Pick>, Vec<String>) {
    let Some(name) = input
        .baseline
        .clone()
        .or_else(|| input.config.baseline.clone())
    else {
        return (Baseline::NotRequested, None, Vec::new());
    };
    let requested = name.to_string();
    let unusable = |reason: String| Baseline::Unusable {
        requested: requested.clone(),
        reason,
    };
    let Some(req) = input.roster.get(name.as_str()) else {
        return (
            unusable("not in the roster".to_string()),
            None,
            vec![requested.clone()],
        );
    };
    // Mode auto, no flags: what `horch spawn <name>` decides.
    let decision = decide(
        req,
        input.roster,
        input.view,
        BalanceMode::Auto,
        GateFlags::default(),
    );
    let routed = RoutingDecision::from(&decision);
    let mut taken = vec![requested.clone()];
    let Some(launch) = resolve(req, input.roster, &decision) else {
        return (Baseline::Routed(routed), None, taken);
    };
    let resolved = match &routed {
        RoutingDecision::Substitute { resolved, .. } => resolved.clone(),
        _ => requested.clone(),
    };
    taken.push(resolved.clone());
    match baseline_pick(&launch, &resolved) {
        Ok(pick) => (Baseline::Routed(routed), Some(pick), taken),
        Err(reason) => (unusable(reason), None, taken),
    }
}

/// The slot a resolved baseline launches: `resolved`'s name with the merged
/// teammate's harness, model and effort.
fn baseline_pick(launch: &Teammate, resolved: &str) -> Result<Pick, String> {
    Roster::is_spawnable(launch).map_err(|e| e.to_string())?;
    let model = ModelId::new(launch.model.as_deref().unwrap_or_default())
        .map_err(|_| "has no model of its own".to_string())?;
    Ok(Pick {
        slot: SlotKind::Baseline,
        teammate: TeammateName::new(resolved).map_err(|e| e.to_string())?,
        harness: launch.agent,
        model,
        effort: launch.effort.clone(),
        propensity: 1.0,
    })
}

/// Plan one round. The same input always gives the same plan; a different
/// `round_id` changes only the exploration slot.
pub fn plan_round(input: &PlanInput) -> RoundPlan {
    let mut filter = input.filter.clone();
    filter.excluded.extend(input.config.exclude.iter().cloned());
    let eligible_set = roster_eligibility(input.roster, input.view, &filter);
    let seed = round_seed(input.round_id);
    let n = input.n as usize;

    let (baseline, base_pick, taken) = route_baseline(input);
    let mut picks: Vec<Pick> = Vec::new();
    let mut covered = Coverage::default();
    if n > 0 {
        if let Some(p) = base_pick {
            covered.add(p.harness.as_str(), p.model.as_str(), p.effort.as_deref());
            picks.push(p);
        }
    }

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

    let rest = open(&planned);
    if picks.len() < n && !rest.is_empty() {
        let k = rest.len();
        let i = SplitMix64::new(seed).below(k as u64) as usize;
        picks.extend(Pick::from_entry(
            &rest[i],
            SlotKind::Exploration,
            1.0 / k as f64,
        ));
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
