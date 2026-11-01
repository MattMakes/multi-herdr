//! Writing an activation plan's skills to disk for one execution.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use super::activation::{ResolvedSkillRef, SkillActivationPlan};
use super::catalog::{CatalogEntry, CatalogSource, SkillCatalog};

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
        check_execution_id(execution_id)?;
        let parent = state_root.join("skill-bundles");
        std::fs::create_dir_all(&parent)?;
        let root = parent.canonicalize()?.join(execution_id);
        std::fs::create_dir(&root)
            .with_context(|| format!("creating skill bundle {}", root.display()))?;
        let out = Self { root };
        let skills_dir = out.skills_dir();
        for skill in &plan.activated {
            materialize_skill(catalog, skill, &skills_dir)?;
        }
        Ok(Some(out))
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }
}

fn check_execution_id(id: &str) -> Result<()> {
    if matches!(id, "" | "." | "..") || id.contains(['/', '\\', '\0']) {
        bail!("execution id '{id}' cannot name a skill bundle directory");
    }
    Ok(())
}

/// Write one activated skill under `skills_dir`, from its directory or from
/// the files compiled into the catalog.
fn materialize_skill(
    catalog: &SkillCatalog,
    skill: &ResolvedSkillRef,
    skills_dir: &Path,
) -> Result<()> {
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
    // A plugin skill loads from the harness's filtered copy of its plugin,
    // which checks that copy's bytes; the bundle's own skills directory
    // never holds it.
    if entry.is_plugin() {
        return Ok(());
    }
    let target = skills_dir.join(skill.id.as_str());
    match source_dir(catalog, entry)? {
        Some(from) => copy_verified(entry, &from, &target),
        None => write_embedded(entry, &target),
    }
}

/// The directory a skill is copied from. `None` for a skill whose files are
/// compiled in (bundled) or that the harness loads itself (plugin).
fn source_dir(catalog: &SkillCatalog, entry: &CatalogEntry) -> Result<Option<PathBuf>> {
    match &entry.source {
        // The store path validates the lock's id and version.
        CatalogSource::Marketplace { .. } => catalog
            .store_dir(entry)?
            .with_context(|| {
                format!(
                    "skill '{}' {} comes from the marketplace store, and this catalog has none",
                    entry.id, entry.version
                )
            })
            .map(Some),
        CatalogSource::Operator { .. } => Ok(entry.operator_dir()),
        CatalogSource::Bundled | CatalogSource::Plugin { .. } => Ok(None),
    }
}

/// Copy `from` to `target` and check the copy against the pinned digest.
fn copy_verified(entry: &CatalogEntry, from: &Path, target: &Path) -> Result<()> {
    let operator = entry.is_operator();
    if !from.is_dir() {
        let msg = if operator {
            format!(
                "skill '{}' {}: {} is missing; run `horch teammates --check`",
                entry.id,
                entry.version,
                from.display()
            )
        } else {
            format!(
                "skill '{}' {} is locked but {} is missing; run `horch marketplace refresh`",
                entry.id,
                entry.version,
                from.display()
            )
        };
        bail!("{msg}");
    }
    copy_tree(from, target)?;
    // Check the copy, not the source, so what launch reads is what the lock
    // (or, for an operator skill, the plan) pinned.
    let actual = horch_marketplace::integrity::tree_digest(target)
        .map_err(|e| anyhow!("skill '{}': {e}", entry.id))?;
    if actual != entry.digest.to_string() {
        let msg = if operator {
            format!(
                "skill '{}' {}: the copy holds {actual}, the plan pins {}; \
                 the operator directory changed during the launch",
                entry.id, entry.version, entry.digest
            )
        } else {
            format!(
                "skill '{}' {}: the store holds {actual}, the lock pins {}; run `horch skills doctor`",
                entry.id, entry.version, entry.digest
            )
        };
        bail!("{msg}");
    }
    Ok(())
}

/// Write the files compiled into a bundled skill under `target`.
fn write_embedded(entry: &CatalogEntry, target: &Path) -> Result<()> {
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
    Ok(())
}

/// Copy the regular files and directories under `from` to `to`. A symlink
/// or any other file type fails: the store never holds one.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for item in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        copy_item(item?, to)?;
    }
    Ok(())
}

/// Copy one directory entry into `to`.
fn copy_item(item: std::fs::DirEntry, to: &Path) -> Result<()> {
    let kind = item.file_type()?;
    let target = to.join(item.file_name());
    if kind.is_dir() {
        copy_tree(&item.path(), &target)
    } else if kind.is_file() {
        std::fs::copy(item.path(), &target)
            .map(drop)
            .with_context(|| format!("copying {}", item.path().display()))
    } else {
        bail!("{} is not a regular file", item.path().display());
    }
}

impl Drop for MaterializedSkills {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
