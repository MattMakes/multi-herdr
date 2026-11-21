use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use horch_marketplace::SkillVersion;
use serde::{Deserialize, Serialize};

use super::catalog::{CatalogEntry, SkillCatalog};
use super::selection::{check, operator_names, phase_skills};
use crate::digest::Digest;
use crate::ids::SkillId;
use crate::roster::{Phase, Teammate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationPolicy {
    Explicit,
    Deterministic,
    Offered,
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSkillRef {
    pub id: SkillId,
    pub version: SkillVersion,
    pub digest: Digest,
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
    pub activated: Vec<ResolvedSkillRef>,
    pub available: Vec<ResolvedSkillRef>,
    pub plugin_skills: BTreeMap<String, Vec<String>>,
}

impl SkillActivationPlan {
    pub fn activated_ids(&self) -> Vec<&str> {
        self.activated.iter().map(|r| r.id.as_str()).collect()
    }
}

pub(crate) fn plan_activation(
    teammate: &Teammate,
    phase: Option<Phase>,
    catalog: &SkillCatalog,
) -> Result<SkillActivationPlan> {
    let mut explicit: BTreeSet<String> = teammate.skills.iter().cloned().collect();
    let phased = phased_names(phase);
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
    let plugin = plugin_names(teammate);
    let (activated, available) = split_catalog(catalog, &explicit, &phased, &offered, &plugin);
    Ok(SkillActivationPlan {
        activated,
        available,
        plugin_skills: teammate.plugin_skills.clone(),
    })
}

fn phased_names(phase: Option<Phase>) -> BTreeSet<String> {
    phase
        .map(|p| phase_skills(p).iter().map(|s| (*s).to_owned()).collect())
        .unwrap_or_default()
}

fn plugin_names(teammate: &Teammate) -> BTreeSet<String> {
    teammate
        .plugin_skills
        .iter()
        .flat_map(|(p, skills)| skills.iter().map(move |s| format!("{p}:{s}")))
        .collect()
}

fn split_catalog(
    catalog: &SkillCatalog,
    explicit: &BTreeSet<String>,
    phased: &BTreeSet<String>,
    offered: &BTreeSet<String>,
    plugin: &BTreeSet<String>,
) -> (Vec<ResolvedSkillRef>, Vec<ResolvedSkillRef>) {
    let mut activated = Vec::new();
    let mut available = Vec::new();
    for entry in catalog.entries() {
        let name = entry.id.as_str();
        if entry.is_plugin() {
            if plugin.contains(name) {
                activated.push(ResolvedSkillRef::new(entry, InvocationPolicy::Explicit));
            }
        } else if explicit.contains(name) {
            activated.push(ResolvedSkillRef::new(entry, InvocationPolicy::Explicit));
        } else if phased.contains(name) {
            activated.push(ResolvedSkillRef::new(
                entry,
                InvocationPolicy::Deterministic,
            ));
        } else if offered.contains(name) {
            activated.push(ResolvedSkillRef::new(entry, InvocationPolicy::Offered));
        } else {
            available.push(ResolvedSkillRef::new(entry, InvocationPolicy::Available));
        }
    }
    (activated, available)
}
