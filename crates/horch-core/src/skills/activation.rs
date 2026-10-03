//! Which skills a teammate gets in a phase, and why.
//!
//! Pure: only the teammate's `skills:`, the phase and the catalog decide. No
//! NLP and no model call.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::catalog::{CatalogEntry, SkillCatalog, SkillVersion};
use super::selection::{check, phase_skills};
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
    /// Explicit and Deterministic skills, sorted by id.
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

/// Teammate `skills:` map to Explicit, the phase's skills to Deterministic,
/// and every other catalog skill to Available. A skill named both ways is
/// Explicit. Fails as the legacy selection does: on an unknown skill, or on
/// skills for a teammate that cannot load them.
pub fn plan_activation(
    teammate: &Teammate,
    phase: Option<Phase>,
    catalog: &SkillCatalog,
) -> Result<SkillActivationPlan> {
    let explicit: BTreeSet<String> = teammate.skills.iter().cloned().collect();
    let mut names = explicit.clone();
    if let Some(phase) = phase {
        names.extend(phase_skills(phase).iter().map(|s| (*s).to_owned()));
    }
    check(teammate, &names, catalog)?;
    let mut activated = Vec::new();
    let mut available = Vec::new();
    for entry in catalog.entries() {
        let name = entry.id.as_str();
        if explicit.contains(name) {
            activated.push(ResolvedSkillRef::new(entry, InvocationPolicy::Explicit));
        } else if names.contains(name) {
            activated.push(ResolvedSkillRef::new(
                entry,
                InvocationPolicy::Deterministic,
            ));
        } else {
            available.push(ResolvedSkillRef::new(entry, InvocationPolicy::Available));
        }
    }
    Ok(SkillActivationPlan {
        activated,
        available,
        plugin_skills: teammate.plugin_skills.clone(),
    })
}
