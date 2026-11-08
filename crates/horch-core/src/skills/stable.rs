//! Persistent paths to catalog skills outside short-lived spawn bundles.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::catalog::{CatalogEntry, CatalogSource, SkillCatalog};
use super::materialize::write_embedded;

/// Return an absolute directory that holds `id` and remains valid after a
/// spawn bundle is deleted. Bundled files are stored by content digest under
/// `state_root`; marketplace skills keep their existing installed directory.
pub fn skill_dir(catalog: &SkillCatalog, state_root: &Path, id: &str) -> Result<PathBuf> {
    let entry = catalog.lookup(id).with_context(|| {
        format!("no skill '{id}' in the catalog; list skills with horch skills")
    })?;
    match entry.source {
        CatalogSource::Bundled => bundled_dir(state_root, entry),
        CatalogSource::Marketplace { .. } => installed_dir(catalog, entry),
        CatalogSource::Operator { .. } | CatalogSource::Plugin { .. } => {
            bail!("skill '{id}' is not installed in the catalog store")
        }
    }
}

fn bundled_dir(state_root: &Path, entry: &CatalogEntry) -> Result<PathBuf> {
    let parent = state_root.join("skills").join(entry.id.as_str());
    std::fs::create_dir_all(&parent)
        .with_context(|| format!("creating stable skill directory {}", parent.display()))?;
    let parent = parent.canonicalize()?;
    let target = parent.join(entry.digest.hex());
    if !target.exists() {
        materialize_bundled(entry, &target)?;
    }
    verify_bundled(entry, &target)?;
    target
        .canonicalize()
        .with_context(|| format!("resolving stable skill directory {}", target.display()))
}

fn materialize_bundled(entry: &CatalogEntry, target: &Path) -> Result<()> {
    let parent = target.parent().expect("stable skill target has a parent");
    let temporary = parent.join(format!(".tmp-{}", crate::mint_uuid()));
    let result = write_and_publish(entry, &temporary, target);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result
}

fn write_and_publish(entry: &CatalogEntry, temporary: &Path, target: &Path) -> Result<()> {
    write_embedded(entry, temporary)?;
    verify_bundled(entry, temporary)?;
    if let Err(error) = std::fs::rename(temporary, target) {
        if !target.is_dir() {
            return Err(error).with_context(|| {
                format!("publishing stable skill directory {}", target.display())
            });
        }
        std::fs::remove_dir_all(temporary)?;
    }
    Ok(())
}

fn verify_bundled(entry: &CatalogEntry, dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        bail!("stable skill directory {} does not exist", dir.display());
    }
    for (rel, expected) in &entry.files {
        let path = dir.join(rel);
        let actual = std::fs::read(&path)
            .with_context(|| format!("reading stable skill file {}", path.display()))?;
        if actual != *expected {
            bail!("stable skill file {} has changed", path.display());
        }
        verify_executable(entry, rel, &path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn verify_executable(entry: &CatalogEntry, rel: &str, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = path.metadata()?.permissions().mode() & 0o111 != 0;
    if executable != entry.executable.contains(&rel) {
        bail!(
            "stable skill file {} has the wrong execute mode",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_executable(_entry: &CatalogEntry, _rel: &str, _path: &Path) -> Result<()> {
    Ok(())
}

fn installed_dir(catalog: &SkillCatalog, entry: &CatalogEntry) -> Result<PathBuf> {
    let dir = catalog
        .store_dir(entry)?
        .with_context(|| format!("skill '{}': no marketplace store", entry.id))?;
    if !dir.is_dir() {
        bail!(
            "skill '{}' is installed but {} does not exist; run `horch skills doctor`",
            entry.id,
            dir.display()
        );
    }
    dir.canonicalize()
        .with_context(|| format!("resolving installed skill directory {}", dir.display()))
}
