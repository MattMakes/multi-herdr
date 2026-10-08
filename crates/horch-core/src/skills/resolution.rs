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
    /// [`SkillResolution::read`] read. Fails on an unknown skill in
    /// `skills:` or the phase, on skills the teammate cannot load, and,
    /// when the plan activates any skill (operator and plugin skills
    /// count), on the support check: the rule the launch applies.
    ///
    /// An `available_skills:` name the catalog lacks is dropped with 1
    /// `NOTE:` line on stderr: a teammate file can name a skill that this
    /// horch build does not bundle yet, and an offer must not refuse a
    /// spawn.
    pub fn plan(&self, teammate: &Teammate) -> Result<SkillActivationPlan> {
        let (offered, unknown): (Vec<String>, Vec<String>) = teammate
            .available_skills
            .iter()
            .cloned()
            .partition(|name| self.catalog.lookup(name).is_some());
        let kept;
        let teammate = if unknown.is_empty() {
            teammate
        } else {
            for name in &unknown {
                eprintln!("{}", unknown_offered_note(&teammate.name, name));
            }
            kept = Teammate {
                available_skills: offered,
                ..teammate.clone()
            };
            &kept
        };
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

/// The line [`SkillResolution::plan`] prints for an `available_skills:`
/// name the catalog lacks.
fn unknown_offered_note(who: &str, name: &str) -> String {
    format!(
        "NOTE: teammate '{who}': available skill '{name}' is not in this horch build, so \
         it is not offered. Rebuild and install horch (just install) to offer it."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANY_AGENT: SupportCheck = |_| Ok(());

    fn bundled() -> SkillResolution {
        SkillResolution::read(SkillCatalog::bundled().unwrap(), None, None, ANY_AGENT).unwrap()
    }

    /// A teammate file may offer a skill that this build does not bundle
    /// yet: the plan drops it with a note, and offers the rest.
    #[test]
    fn skl_12_unknown_available_skill_is_dropped_with_a_note() {
        let t = Teammate {
            name: "offers".into(),
            available_skills: vec!["trace".into(), "not-in-this-build".into()],
            ..Teammate::default()
        };
        let plan = bundled().plan(&t).unwrap();
        assert_eq!(plan.activated_ids(), ["trace"]);
        assert!(plan
            .available
            .iter()
            .all(|r| r.id.as_str() != "not-in-this-build"));
        let note = unknown_offered_note(&t.name, "not-in-this-build");
        assert!(note.starts_with("NOTE: "), "{note}");
        assert!(note.contains("'not-in-this-build'"), "{note}");
        assert!(note.contains("Rebuild and install horch"), "{note}");
        assert_eq!(note.lines().count(), 1, "{note}");
    }

    /// An unknown name in `skills:` still refuses: the teammate is expected
    /// to use it, so planning without it would mislead the briefing.
    #[test]
    fn skl_12_unknown_explicit_skill_still_refuses() {
        let t = Teammate {
            name: "expects".into(),
            skills: vec!["not-in-this-build".into()],
            available_skills: vec!["trace".into()],
            ..Teammate::default()
        };
        let err = format!("{:#}", bundled().plan(&t).unwrap_err());
        assert!(
            err.contains("unknown bundled skill 'not-in-this-build'"),
            "{err}"
        );
    }
}
