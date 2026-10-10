//! Writing an activation plan's skills to disk for one execution.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

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
            // A plugin skill loads from the harness's filtered copy of its
            // plugin, which checks that copy's bytes; the bundle's own
            // skills directory never holds it.
            if entry.is_plugin() {
                continue;
            }
            let target = out.skills_dir().join(skill.id.as_str());
            let from = match &entry.source {
                CatalogSource::Bundled => None,
                // The store path validates the lock's id and version.
                CatalogSource::Marketplace { .. } => {
                    Some(catalog.store_dir(entry)?.with_context(|| {
                        format!(
                        "skill '{}' {} comes from the marketplace store, and this catalog has none",
                        skill.id, skill.version
                    )
                    })?)
                }
                CatalogSource::Operator { .. } => entry.operator_dir(),
                CatalogSource::Plugin { .. } => unreachable!("skipped above"),
            };
            if let Some(from) = from {
                if !from.is_dir() {
                    if entry.is_operator() {
                        bail!(
                            "skill '{}' {}: {} is missing; run `horch teammates --check`",
                            skill.id,
                            skill.version,
                            from.display()
                        );
                    }
                    bail!(
                        "skill '{}' {} is locked but {} is missing; run `horch marketplace refresh`",
                        skill.id,
                        skill.version,
                        from.display()
                    );
                }
                copy_tree(&from, &target)?;
                // Check the copy, not the source, so what launch reads is
                // what the lock (or, for an operator skill, the plan) pinned.
                let actual = horch_marketplace::integrity::tree_digest(&target)
                    .map_err(|e| anyhow!("skill '{}': {e}", skill.id))?;
                if actual != entry.digest.to_string() {
                    if entry.is_operator() {
                        bail!(
                            "skill '{}' {}: the copy holds {actual}, the plan pins {}; \
                             the operator directory changed during the launch",
                            skill.id,
                            skill.version,
                            entry.digest
                        );
                    }
                    bail!(
                        "skill '{}' {}: the store holds {actual}, the lock pins {}; run `horch skills doctor`",
                        skill.id,
                        skill.version,
                        entry.digest
                    );
                }
                continue;
            }
            for (rel, bytes) in &entry.files {
                let target = target.join(rel);
                std::fs::create_dir_all(target.parent().expect("skill path has parent"))?;
                std::fs::write(&target, bytes)?;
                // The digest covers the execute bit, so the bundle keeps it.
                #[cfg(unix)]
                if entry.executable.contains(rel) {
                    use std::os::unix::fs::PermissionsExt as _;
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
                }
            }
        }
        Ok(Some(out))
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }
}

/// Copy the regular files and directories under `from` to `to`. A symlink
/// or any other file type fails: the store never holds one.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for item in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let item = item?;
        let kind = item.file_type()?;
        let target = to.join(item.file_name());
        if kind.is_dir() {
            copy_tree(&item.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(item.path(), &target)
                .with_context(|| format!("copying {}", item.path().display()))?;
        } else {
            bail!("{} is not a regular file", item.path().display());
        }
    }
    Ok(())
}

impl Drop for MaterializedSkills {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
