//! Which skills a teammate gets in a phase, and why.
//!
//! Pure: only the teammate's `skills:`, `available_skills:`, the phase and
//! the catalog decide. No NLP and no model call.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use horch_marketplace::SkillVersion;
use serde::{Deserialize, Serialize};

use super::catalog::{CatalogEntry, SkillCatalog};
use super::selection::{check, operator_names, phase_skills};
use crate::ids::SkillId;
use crate::measure::digest::Digest;
use crate::roster::{Phase, Teammate};

/// Why a skill is in a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationPolicy {
    /// Named in the teammate's `skills:`; the briefing says it is expected.
    Explicit,
    /// Brought in by the phase selector.
    Deterministic,
    /// Named in the teammate's `available_skills:`: materialized, and named
    /// in the briefing without a description.
    Offered,
    /// In the catalog, not activated.
    Available,
}

/// One skill at the exact version a plan resolved it to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSkillRef {
    pub id: SkillId,
    pub version: SkillVersion,
    pub digest: Digest,
    /// `bundled`, or the marketplace source spec with `@<commit>` for git.
    pub source: String,
    pub policy: InvocationPolicy,
}

impl ResolvedSkillRef {
    fn new(entry: &CatalogEntry, policy: InvocationPolicy) -> Self {
        Self {
            id: entry.id.clone(),
            version: entry.version.clone(),
            digest: entry.digest,
            source: entry.source_label(),
            policy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillActivationPlan {
    /// Explicit, Deterministic and Offered skills, sorted by id.
    pub activated: Vec<ResolvedSkillRef>,
    /// Every other catalog skill, sorted by id.
    pub available: Vec<ResolvedSkillRef>,
    /// External Claude plugins and their named skills. They are not catalog
    /// skills and never enter `activated` or `available` (SKL-07).
    pub plugin_skills: BTreeMap<String, Vec<String>>,
}

impl SkillActivationPlan {
    pub fn activated_ids(&self) -> Vec<&str> {
        self.activated.iter().map(|r| r.id.as_str()).collect()
    }
}

/// Teammate `skills:` and operator skills map to Explicit, the phase's
/// skills to Deterministic, `available_skills:` to Offered, and every other
/// catalog skill to Available. A skill named more than one way takes the
/// first of that order. Fails as the legacy selection does: on an unknown
/// skill, or on skills for a teammate that cannot load them.
///
/// Operator skills are Explicit only when `catalog` holds them, that is,
/// when the caller extended it with `SkillCatalog::with_operator_skills`.
pub fn plan_activation(
    teammate: &Teammate,
    phase: Option<Phase>,
    catalog: &SkillCatalog,
) -> Result<SkillActivationPlan> {
    let mut explicit: BTreeSet<String> = teammate.skills.iter().cloned().collect();
    let phased: BTreeSet<String> = phase
        .map(|p| phase_skills(p).iter().map(|s| (*s).to_owned()).collect())
        .unwrap_or_default();
    let offered: BTreeSet<String> = teammate.available_skills.iter().cloned().collect();
    let names: BTreeSet<String> = explicit
        .iter()
        .chain(&phased)
        .chain(&offered)
        .cloned()
        .collect();
    check(teammate, &names, catalog)?;
    explicit.extend(
        operator_names(teammate)
            .filter(|n| catalog.lookup(n).is_some_and(CatalogEntry::is_operator))
            .map(str::to_owned),
    );
    let mut activated = Vec::new();
    let mut available = Vec::new();
    for entry in catalog.entries() {
        let name = entry.id.as_str();
        let policy = if explicit.contains(name) {
            InvocationPolicy::Explicit
        } else if phased.contains(name) {
            InvocationPolicy::Deterministic
        } else if offered.contains(name) {
            InvocationPolicy::Offered
        } else {
            available.push(ResolvedSkillRef::new(entry, InvocationPolicy::Available));
            continue;
        };
        activated.push(ResolvedSkillRef::new(entry, policy));
    }
    Ok(SkillActivationPlan {
        activated,
        available,
        plugin_skills: teammate.plugin_skills.clone(),
    })
}
