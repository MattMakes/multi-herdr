use std::path::Path;

use anyhow::Result;

use super::activation::{plan_activation, SkillActivationPlan};
use super::catalog::SkillCatalog;
use crate::roster::Teammate;

pub type SupportCheck = fn(&Teammate) -> Result<()>;

pub fn harness_support(teammate: &Teammate) -> Result<()> {
    teammate.agent.adapter().ensure_skills_supported()
}

#[derive(Debug, Clone)]
pub struct SkillResolution {
    catalog: SkillCatalog,
    supported: SupportCheck,
}

impl SkillResolution {
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

    pub fn catalog(&self) -> &SkillCatalog {
        &self.catalog
    }

    pub fn into_catalog(self) -> SkillCatalog {
        self.catalog
    }
}

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
