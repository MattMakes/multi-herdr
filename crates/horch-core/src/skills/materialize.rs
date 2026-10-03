//! Writing an activation plan's skills to disk for one execution.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::activation::SkillActivationPlan;
use super::catalog::{CatalogSource, SkillCatalog};

/// An owned directory `<state_root>/skill-bundles/<execution_id>/`.
/// Dropping it deletes that directory, which only this value created.
#[derive(Debug)]
pub struct MaterializedSkills {
    pub root: PathBuf,
}

impl MaterializedSkills {
    /// Write every activated skill's files under `<root>/skills/<id>/`.
    /// `None` when the plan activates nothing. Fails if the directory
    /// exists, so two executions never share one.
    pub fn materialize(
        plan: &SkillActivationPlan,
        catalog: &SkillCatalog,
        state_root: &Path,
        execution_id: &str,
    ) -> Result<Option<Self>> {
        if plan.activated.is_empty() {
            return Ok(None);
        }
        if execution_id.is_empty()
            || matches!(execution_id, "." | "..")
            || execution_id.contains(['/', '\\', '\0'])
        {
            bail!("execution id '{execution_id}' cannot name a skill bundle directory");
        }
        let parent = state_root.join("skill-bundles");
        std::fs::create_dir_all(&parent)?;
        let root = parent.canonicalize()?.join(execution_id);
        std::fs::create_dir(&root)
            .with_context(|| format!("creating skill bundle {}", root.display()))?;
        let out = Self { root };
        for skill in &plan.activated {
            let entry = catalog
                .get(&skill.id)
                .with_context(|| format!("skill '{}' is not in the catalog", skill.id))?;
            if entry.version != skill.version {
                bail!(
                    "skill '{}': the plan has {} but the catalog has {}",
                    skill.id,
                    skill.version,
                    entry.version
                );
            }
            if let CatalogSource::Marketplace { .. } = entry.source {
                // Copying from the marketplace store arrives with harness
                // exposure (A10) and the marketplace CLI (A11).
                bail!(
                    "skill '{}' {} comes from the marketplace store, which launch cannot read yet",
                    skill.id,
                    skill.version
                );
            }
            for (rel, bytes) in &entry.files {
                let target = out.skills_dir().join(skill.id.as_str()).join(rel);
                std::fs::create_dir_all(target.parent().expect("skill path has parent"))?;
                std::fs::write(target, bytes)?;
            }
        }
        Ok(Some(out))
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }
}

impl Drop for MaterializedSkills {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
