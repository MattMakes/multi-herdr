//! A teammate's skills, resolved once for every caller: the spawn plan, the
//! competition coordinator, the orchestrator record, the launch bundle and
//! `horch teammates --check`.
//!
//! [`SkillResolution::read`] reads what the host adds to the caller's base
//! catalog (operator skills, plugin skills). [`SkillResolution::plan`] is
//! pure: it plans the activation and applies the 1 support rule. So the
//! record and the launch see the same skills (SKL-04), and the spawn plan
//! rejects exactly what the launch rejects (SKL-12). The caller keeps the
//! choice of the base catalog: the launch fails on a bad lock
//! (`SkillCatalog::installed`), planning falls back to the bundled catalog
//! (`Roster::skill_catalog`).

use std::path::Path;

use anyhow::Result;

use super::activation::{plan_activation, SkillActivationPlan};
use super::catalog::SkillCatalog;
use crate::roster::Teammate;

/// Fails when `teammate`'s agent cannot load the skills it activates on
/// this host. Production passes [`harness_support`]; a test passes a fake.
pub type SupportCheck = fn(&Teammate) -> Result<()>;

/// The teammate's harness adapter decides.
pub fn harness_support(teammate: &Teammate) -> Result<()> {
    teammate.agent.adapter().ensure_skills_supported()
}

/// The base catalog extended with one teammate's host skills, and the
/// support check its plan applies.
#[derive(Debug, Clone)]
pub struct SkillResolution {
    catalog: SkillCatalog,
    supported: SupportCheck,
}

impl SkillResolution {
    /// `base` plus the `operator_skills:` and `plugin_skills:` of
    /// `teammate`, read from disk with `~/` expanded against `home`
    /// (`SkillCatalog::with_host_skills`). `None` reads nothing: a spawn
    /// of a teammate the roster lacks fails in planning.
    pub fn read(
        base: SkillCatalog,
        teammate: Option<&Teammate>,
        home: Option<&Path>,
        supported: SupportCheck,
    ) -> Result<Self> {
        let catalog = match teammate {
            Some(t) => base.with_host_skills(t, home)?,
            None => base,
        };
        Ok(Self { catalog, supported })
    }

    /// The activation plan of `teammate` in its phase. Give the teammate
    /// that will launch (after `Roster::for_launch`), the one
    /// [`SkillResolution::read`] read. Fails on an unknown skill, on skills
    /// the teammate cannot load, and, when the plan activates any skill
    /// (operator and plugin skills count), on the support check: the rule
    /// the launch applies.
    pub fn plan(&self, teammate: &Teammate) -> Result<SkillActivationPlan> {
        let plan = plan_activation(teammate, teammate.phase, &self.catalog)?;
        if !plan.activated.is_empty() {
            (self.supported)(teammate)?;
        }
        Ok(plan)
    }

    /// The extended catalog. Its `skipped_operator` lists the operator
    /// skills this host does not have.
    pub fn catalog(&self) -> &SkillCatalog {
        &self.catalog
    }

    pub fn into_catalog(self) -> SkillCatalog {
        self.catalog
    }
}
