//! The skill catalog: every skill a teammate could be given, with an
//! immutable resolved identity.
//!
//! Bundled skills come from the compiled-in `skills/` tree and are versioned
//! `bundled+<digest12>`. Marketplace lock entries merge in on top. Only
//! [`SkillCatalog::installed`] and [`check_store`] read the filesystem: the
//! marketplace store under the data root (OD3). Nothing here reads the
//! environment.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use horch_marketplace::{
    BundledFile, BundledSkill, Catalog, GitRunner, Installer, LockEntry, Lockfile, SkillManifest,
    Store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{BUNDLED_PROVENANCE, BUNDLED_SKILL_FILES};
use crate::ids::SkillId;
use crate::measure::digest::Digest;

/// The marketplace API, for the `horch` binary, which depends on core only.
pub use horch_marketplace as marketplace;
use horch_marketplace::SkillVersion;

/// Where a catalog entry's files come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatalogSource {
    /// Compiled into the binary from `skills/`.
    Bundled,
    /// Installed by the marketplace; `source` is the lock's source spec.
    /// `resolved_commit` is the 40-hex commit of a git source, `None` for a
    /// local one.
    Marketplace {
        source: String,
        resolved_commit: Option<String>,
    },
}

/// Where a bundled skill was adapted from, per `skills/provenance.json`. A
/// skill written in this repository has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub repository: String,
    pub revision: String,
    pub source_path: String,
    /// sha256 hex of the full original upstream SKILL.md.
    pub source_sha256: String,
    pub adaptation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: SkillId,
    pub version: SkillVersion,
    pub source: CatalogSource,
    /// The marketplace tree digest of the skill directory.
    pub digest: Digest,
    /// The SKILL.md `description`, as written. Empty for a marketplace entry:
    /// the lock does not carry it.
    pub description: String,
    pub provenance: Option<Provenance>,
    /// Byte length of SKILL.md; 0 for a marketplace entry.
    pub skill_file_bytes: usize,
    /// `(path relative to the skill dir, bytes)`; empty for a marketplace
    /// entry, whose files live in the marketplace store.
    pub(crate) files: Vec<(&'static str, &'static [u8])>,
}

impl CatalogEntry {
    /// The source label a `ResolvedSkillRef` records: `bundled`, or the
    /// lock's source spec with `@<commit>` for a git source.
    pub fn source_label(&self) -> String {
        match &self.source {
            CatalogSource::Bundled => "bundled".into(),
            CatalogSource::Marketplace {
                source,
                resolved_commit: Some(commit),
            } => format!("{source}@{commit}"),
            CatalogSource::Marketplace { source, .. } => source.clone(),
        }
    }
}

/// Bundled entries plus marketplace lock entries, keyed by skill id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    entries: BTreeMap<String, CatalogEntry>,
    /// The marketplace store root (`<data_root>`) that marketplace entries'
    /// files live under. `None` for a catalog built from a lock in memory.
    store_root: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct ProvenanceFile {
    source_repository: String,
    source_revision: String,
    skills: Vec<ProvenanceSkill>,
}

/// A skill may name its own upstream; `source_path: null` marks a skill
/// written in this repository.
#[derive(Deserialize)]
struct ProvenanceSkill {
    name: String,
    source_repository: Option<String>,
    source_revision: Option<String>,
    source_path: Option<String>,
    source_sha256: Option<String>,
    adaptation: String,
}

impl SkillCatalog {
    /// The compiled-in skills, validated, with their upstream provenance.
    pub fn bundled() -> Result<SkillCatalog> {
        let provenance: ProvenanceFile =
            serde_json::from_str(BUNDLED_PROVENANCE).context("skills/provenance.json")?;
        let mut entries = BTreeMap::new();
        for (path, bytes) in BUNDLED_SKILL_FILES {
            let Some(name) = path.strip_suffix("/SKILL.md") else {
                continue;
            };
            if name.contains('/') {
                continue;
            }
            let meta = validate_skill_md(path, name, bytes)?;
            let prefix = format!("{name}/");
            let files: Vec<(&'static str, &'static [u8])> = BUNDLED_SKILL_FILES
                .iter()
                .filter_map(|(p, b)| p.strip_prefix(&prefix).map(|rel| (rel, *b)))
                .collect();
            let digest = tree_digest(&files);
            let id = SkillId::new(name).with_context(|| format!("{path}: skill id"))?;
            let provenance = provenance
                .skills
                .iter()
                .find(|s| s.name == name)
                .and_then(|s| {
                    Some(Provenance {
                        repository: s
                            .source_repository
                            .clone()
                            .unwrap_or_else(|| provenance.source_repository.clone()),
                        revision: s
                            .source_revision
                            .clone()
                            .unwrap_or_else(|| provenance.source_revision.clone()),
                        source_path: s.source_path.clone()?,
                        source_sha256: s.source_sha256.clone()?,
                        adaptation: s.adaptation.clone(),
                    })
                });
            entries.insert(
                name.to_owned(),
                CatalogEntry {
                    id,
                    version: SkillVersion(format!("bundled+{}", digest.short12())),
                    source: CatalogSource::Bundled,
                    digest,
                    description: meta.description,
                    provenance,
                    skill_file_bytes: bytes.len(),
                    files,
                },
            );
        }
        Ok(SkillCatalog {
            entries,
            store_root: None,
        })
    }

    /// The catalog a launch sees: the bundled skills plus the marketplace
    /// lock under `data_root` (`${XDG_DATA_HOME:-~/.local/share}/horch/`),
    /// when it exists. A marketplace entry gets its description from the
    /// installed SKILL.md; a missing version directory leaves it empty, and
    /// materializing that entry fails.
    pub fn installed(data_root: &Path) -> Result<SkillCatalog> {
        let lock_path = Store::new(data_root).lock_path();
        let lock =
            Lockfile::read(&lock_path).map_err(|e| anyhow!("{}: {e}", lock_path.display()))?;
        let mut catalog = Self::bundled()?.with_lock(&lock.skills)?;
        catalog.store_root = Some(data_root.to_path_buf());
        for entry in catalog.entries.values_mut() {
            if entry.source == CatalogSource::Bundled {
                continue;
            }
            let dir = version_dir(data_root, entry.id.as_str(), &entry.version.0)?;
            if let Ok(manifest) = SkillManifest::read(&dir, Some(entry.id.as_str())) {
                entry.description = manifest.description;
                entry.skill_file_bytes = std::fs::metadata(dir.join("SKILL.md"))
                    .map(|m| m.len() as usize)
                    .unwrap_or(0);
            }
        }
        Ok(catalog)
    }

    /// `<data_root>/skills/<id>/<version>/` of a marketplace entry. `None`
    /// for a bundled entry, or when this catalog has no store root.
    pub fn store_dir(&self, entry: &CatalogEntry) -> Result<Option<PathBuf>> {
        match (&entry.source, &self.store_root) {
            (CatalogSource::Marketplace { .. }, Some(root)) => {
                version_dir(root, entry.id.as_str(), &entry.version.0).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// The bundled skills as the installer's catalog, so a `bundled:<id>`
    /// source installs the compiled-in copy.
    pub(crate) fn marketplace_catalog(&self) -> Result<Catalog> {
        let mut out = Catalog::new();
        for entry in self.entries.values() {
            if entry.source != CatalogSource::Bundled {
                continue;
            }
            out = out.with_bundled(BundledSkill {
                id: horch_marketplace::SkillId::parse(entry.id.as_str())
                    .map_err(|e| anyhow!("{e}"))?,
                files: entry
                    .files
                    .iter()
                    .map(|(path, bytes)| BundledFile {
                        path: (*path).to_owned(),
                        bytes: Cow::Borrowed(*bytes),
                    })
                    .collect(),
            });
        }
        Ok(out)
    }

    /// Merge marketplace lock entries in.
    ///
    /// A lock entry whose source is `bundled:<id>` records an install of the
    /// compiled-in copy; it never replaces the bundled entry. Any other lock
    /// entry is an explicit install and wins over a bundled skill of the
    /// same id.
    pub fn with_lock(mut self, lock: &[LockEntry]) -> Result<SkillCatalog> {
        for entry in lock {
            if entry.source.starts_with("bundled:") {
                continue;
            }
            // The SKILL.md name rules: an id later names a directory.
            horch_marketplace::SkillId::parse(&entry.id)
                .map_err(|e| anyhow::anyhow!("marketplace.lock: {e}"))?;
            let id = SkillId::new(entry.id.as_str())
                .with_context(|| format!("marketplace.lock: skill id '{}'", entry.id))?;
            let digest: Digest = entry
                .digest
                .parse()
                .with_context(|| format!("marketplace.lock: '{}' digest", entry.id))?;
            if entry.version.is_empty() {
                bail!("marketplace.lock: '{}' has no version", entry.id);
            }
            self.entries.insert(
                entry.id.clone(),
                CatalogEntry {
                    id,
                    version: SkillVersion(entry.version.clone()),
                    source: CatalogSource::Marketplace {
                        source: entry.source.clone(),
                        resolved_commit: entry.resolved_commit.clone(),
                    },
                    digest,
                    description: String::new(),
                    provenance: None,
                    skill_file_bytes: 0,
                    files: Vec::new(),
                },
            );
        }
        Ok(self)
    }

    pub fn get(&self, id: &SkillId) -> Option<&CatalogEntry> {
        self.entries.get(id.as_str())
    }

    /// Look a skill up by a name that may not be a valid id.
    pub fn lookup(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.get(name)
    }

    /// Every entry, sorted by id.
    pub fn entries(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The marketplace installer over the store at `data_root`, running `git`
/// (the caller resolves `$HORCH_GIT_BIN`), with the bundled skills as its
/// catalog.
pub fn installer(data_root: &Path, git: &Path) -> Result<Installer> {
    Ok(Installer::new(data_root.to_path_buf(), GitRunner::new(git))
        .with_catalog(SkillCatalog::bundled()?.marketplace_catalog()?))
}

/// What `horch skills doctor` found for one lock entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreState {
    /// The version directory exists and matches the locked digest.
    Ok,
    /// The version directory does not exist.
    Missing,
    /// The files on disk have a different digest, or fail the store's
    /// tree rules (`actual` says why).
    Tampered { actual: String },
    /// The lock entry cannot name a directory.
    Invalid(String),
}

/// Check every lock entry under `data_root` against the files on disk.
/// Needs no network. An absent lock is an empty store.
pub fn check_store(data_root: &Path) -> Result<Vec<(LockEntry, PathBuf, StoreState)>> {
    let lock_path = Store::new(data_root).lock_path();
    let lock = Lockfile::read(&lock_path).map_err(|e| anyhow!("{}: {e}", lock_path.display()))?;
    let mut out = Vec::new();
    for entry in lock.skills {
        let (dir, state) = match version_dir(data_root, &entry.id, &entry.version) {
            Err(e) => (PathBuf::new(), StoreState::Invalid(format!("{e:#}"))),
            Ok(dir) if !dir.is_dir() => (dir, StoreState::Missing),
            Ok(dir) => {
                let state = match horch_marketplace::integrity::tree_digest(&dir) {
                    Ok(actual) if actual == entry.digest => StoreState::Ok,
                    Ok(actual) => StoreState::Tampered { actual },
                    Err(e) => StoreState::Tampered {
                        actual: e.to_string(),
                    },
                };
                (dir, state)
            }
        };
        out.push((entry, dir, state));
    }
    Ok(out)
}

/// `<root>/skills/<id>/<version>/`, after checking that `id` and `version`
/// are plain path segments: the lock is an operator-writable file.
fn version_dir(root: &Path, id: &str, version: &str) -> Result<PathBuf> {
    horch_marketplace::SkillId::parse(id).map_err(|e| anyhow!("marketplace.lock: {e}"))?;
    if version.is_empty()
        || matches!(version, "." | "..")
        || version.contains(['/', '\\', '\0', ':'])
    {
        bail!("marketplace.lock: '{id}' version '{version}' cannot name a directory");
    }
    Ok(root.join("skills").join(id).join(version))
}

/// The SKILL.md rules: YAML frontmatter whose `name` equals the directory
/// name and follows `[a-z0-9-]` without a leading, trailing or doubled `-`,
/// and a non-empty `description` of at most 1024 bytes.
fn validate_skill_md(path: &str, name: &str, bytes: &[u8]) -> Result<Metadata> {
    let text = std::str::from_utf8(bytes)
        .context("skill is not UTF-8")?
        .replace("\r\n", "\n");
    let front = text
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .with_context(|| format!("{path}: missing YAML frontmatter"))?;
    let meta: Metadata =
        serde_yaml::from_str(front).with_context(|| format!("{path}: invalid metadata"))?;
    if meta.name != name
        || name.is_empty()
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || meta.description.trim().is_empty()
        || meta.description.len() > 1024
    {
        bail!("{path}: invalid skill name or description");
    }
    Ok(meta)
}

/// The marketplace tree-digest rule (`horch_marketplace::integrity::tree_digest`)
/// over in-memory files: sha256 over the sorted lines
/// `<relative path>\0<hex sha256 of the bytes>\n`.
fn tree_digest(files: &[(&str, &[u8])]) -> Digest {
    let mut lines: Vec<(&str, String)> = files
        .iter()
        .map(|(rel, bytes)| (*rel, crate::measure::digest::sha256_bytes(bytes).hex()))
        .collect();
    lines.sort();
    let mut tree = Sha256::new();
    for (rel, hash) in &lines {
        tree.update(rel.as_bytes());
        tree.update([0u8]);
        tree.update(hash.as_bytes());
        tree.update(b"\n");
    }
    Digest(tree.finalize().into())
}
