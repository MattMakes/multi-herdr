//! The deterministic selectors: which bundled skills a phase brings, and the
//! legacy per-teammate selection.

use std::collections::BTreeSet;

use anyhow::{bail, Result};

use super::catalog::SkillCatalog;
use crate::harness::SkillExposure;
use crate::roster::{Phase, Teammate};

pub(crate) fn phase_skills(phase: Phase) -> &'static [&'static str] {
    match phase {
        Phase::Research => &["brainstorm", "research-codebase", "trace", "handoff"],
        Phase::Plan => &["create-plan", "pre-flight", "handoff"],
        Phase::Implementation => &["execute", "tdd", "debug", "check", "handoff"],
        Phase::Validation => &[
            "check",
            "code-analysis",
            "code-review",
            "security-review",
            "document",
            "handoff",
        ],
    }
}

/// The legacy per-teammate selection, checked against the compiled-in
/// catalog. A test oracle only (SKL-02): no production path calls it; the
/// launch, the spawn plan and `--check` resolve skills through
/// [`super::SkillResolution`].
pub fn selected(teammate: &Teammate) -> Result<Vec<String>> {
    let catalog = SkillCatalog::bundled()?;
    let mut names: BTreeSet<String> = teammate.skills.iter().cloned().collect();
    if let Some(phase) = teammate.phase {
        names.extend(phase_skills(phase).iter().map(|s| (*s).to_owned()));
    }
    names.extend(teammate.available_skills.iter().cloned());
    check(teammate, &names, &catalog)?;
    Ok(names.into_iter().collect())
}

/// The names in the teammate's `operator_skills:`, if any.
pub(crate) fn operator_names(teammate: &Teammate) -> impl Iterator<Item = &str> {
    teammate
        .operator_skills
        .iter()
        .flat_map(|o| o.names.iter().map(String::as_str))
}

/// Every selected name is in the catalog, and a teammate that selects any
/// skill, or names operator skills, can load one.
pub(crate) fn check(
    teammate: &Teammate,
    names: &BTreeSet<String>,
    catalog: &SkillCatalog,
) -> Result<()> {
    for name in names {
        if catalog.lookup(name).is_none() {
            bail!(
                "teammate '{}': unknown bundled skill '{name}'",
                teammate.name
            );
        }
    }
    check_loadable(
        teammate,
        !names.is_empty() || teammate.operator_skills.is_some(),
    )
}

/// A teammate that selects any skill (`any`) can load one.
fn check_loadable(teammate: &Teammate, any: bool) -> Result<()> {
    let exposes = teammate.agent.capabilities().skill_exposure != SkillExposure::None;
    if any && (teammate.disable_skills || !exposes) {
        bail!(
            "teammate '{}': selected skills cannot load with disabled skills or no agent",
            teammate.name
        );
    }
    Ok(())
}
