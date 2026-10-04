//! The skill catalog: every skill a teammate could be given, with an
//! immutable resolved identity.
//!
//! Bundled skills come from the compiled-in `skills/` tree and are versioned
//! `bundled+<digest12>`. Marketplace lock entries merge in on top. A
//! teammate's `operator_skills:` merge in per launch, versioned
//! `operator+<digest12>`, and its `plugin_skills:` as `<plugin>:<skill>`,
//! versioned `<plugin version>+<digest12>`. Only [`SkillCatalog::installed`],
//! [`check_store`] and [`SkillCatalog::with_operator_skills`] read the
//! filesystem. Nothing here reads the environment.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
use crate::roster::Teammate;

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
    /// A teammate's `operator_skills:`: read at launch from `<dir>/<id>/` on
    /// the operator's machine. `dir` is already expanded.
    Operator { dir: PathBuf },
    /// A skill a teammate names in `plugin_skills:`, read at launch from an
    /// external Claude plugin. `plugin` is the plugin's id
    /// (`<plugin>@<marketplace>`, or `<plugin>@inline` for a `plugin_dirs`
    /// entry); `dir` is the skill's directory in the original plugin. The
    /// harness loads it from a filtered copy, not from the bundle.
    Plugin { plugin: String, dir: PathBuf },
}

/// One upstream file a bundled skill was adapted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub repository: String,
    pub revision: String,
    pub path: String,
    /// sha256 hex of the full original upstream file.
    pub sha256: String,
    /// The upstream license, when the entry records it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

/// Where a bundled skill was adapted from, per `skills/provenance.json`.
/// A skill written in this repository has no sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub sources: Vec<SourceRef>,
    pub adaptation: String,
    /// A copy of an upstream skill directory (with its LICENSE). A vendored
    /// skill is exempt from the bundled size budget.
    #[serde(default)]
    pub vendored: bool,
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
            CatalogSource::Operator { dir } => format!("operator:{}", dir.display()),
            CatalogSource::Plugin { plugin, .. } => format!("plugin:{plugin}"),
        }
    }

    /// Whether this entry is a skill of an external plugin (`plugin_skills:`).
    pub fn is_plugin(&self) -> bool {
        matches!(self.source, CatalogSource::Plugin { .. })
    }

    /// Whether this entry came from a teammate's `operator_skills:`.
    pub fn is_operator(&self) -> bool {
        matches!(self.source, CatalogSource::Operator { .. })
    }

    /// `<dir>/<id>/` of an operator entry; `None` for any other entry.
    pub fn operator_dir(&self) -> Option<PathBuf> {
        match &self.source {
            CatalogSource::Operator { dir } => Some(dir.join(self.id.as_str())),
            _ => None,
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
    /// Operator skills that [`SkillCatalog::with_operator_skills`] skipped
    /// because this host does not have them.
    skipped_operator: Vec<SkippedOperatorSkill>,
}

/// An operator skill that this host does not have: its directory, or its
/// `<dir>/<name>/`, does not exist. The launch goes on without it, the
/// briefing says so, and `horch teammates --check` warns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedOperatorSkill {
    pub name: String,
    /// `<dir>/<name>` as the teammate file writes it, `~/` unexpanded.
    pub path: String,
}

impl SkippedOperatorSkill {
    /// The sentence for the briefing and the `--check` warning.
    pub fn note(&self) -> String {
        format!(
            "operator skill {} is not installed on this host (no {}): ask the operator to run \
             `xcrun agent skills export --output-dir <dir>` (Xcode 27 or later)",
            self.name, self.path
        )
    }
}

#[derive(Debug, Deserialize)]
struct Metadata {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct ProvenanceFile {
    /// Defaults for an entry that uses the single-source fields.
    source_repository: Option<String>,
    source_revision: Option<String>,
    skills: Vec<ProvenanceSkill>,
}

/// A skill lists its upstream files in `sources` (empty for a skill written
/// in this repository), or uses the single-source fields, which map to a
/// 1-item list. `source_path: null` marks a skill written in this repository.
#[derive(Deserialize)]
struct ProvenanceSkill {
    name: String,
    sources: Option<Vec<SourceRef>>,
    source_repository: Option<String>,
    source_revision: Option<String>,
    source_path: Option<String>,
    source_sha256: Option<String>,
    adaptation: String,
    #[serde(default)]
    vendored: bool,
}

/// Parse the text of `skills/provenance.json` into one entry per skill name.
pub fn parse_provenance(text: &str) -> Result<BTreeMap<String, Provenance>> {
    let file: ProvenanceFile = serde_json::from_str(text).context("skills/provenance.json")?;
    let mut out = BTreeMap::new();
    for s in file.skills {
        let name = s.name;
        let sources = match s.sources {
            Some(sources) => {
                if s.source_path.is_some()
                    || s.source_sha256.is_some()
                    || s.source_repository.is_some()
                    || s.source_revision.is_some()
                {
                    bail!("provenance '{name}': use either `sources` or the single-source fields");
                }
                sources
            }
            None => match s.source_path {
                None => Vec::new(),
                Some(path) => vec![SourceRef {
                    repository: s
                        .source_repository
                        .or_else(|| file.source_repository.clone())
                        .with_context(|| format!("provenance '{name}': no repository"))?,
                    revision: s
                        .source_revision
                        .or_else(|| file.source_revision.clone())
                        .with_context(|| format!("provenance '{name}': no revision"))?,
                    path,
                    sha256: s
                        .source_sha256
                        .with_context(|| format!("provenance '{name}': no source_sha256"))?,
                    license: None,
                }],
            },
        };
        for src in &sources {
            if src.sha256.len() != 64 || !src.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!(
                    "provenance '{name}': '{}' sha256 is not 64 hex digits",
                    src.path
                );
            }
        }
        let entry = Provenance {
            sources,
            adaptation: s.adaptation,
            vendored: s.vendored,
        };
        if out.insert(name.clone(), entry).is_some() {
            bail!("provenance '{name}': duplicate entry");
        }
    }
    Ok(out)
}

impl SkillCatalog {
    /// The compiled-in skills, validated, with their upstream provenance.
    /// Built once per process: the files are compiled in, and digesting them
    /// on every call made `Roster::check` (one call per teammate) slow. A
    /// clone is cheap, because the entries hold `&'static` file bytes.
    pub fn bundled() -> Result<SkillCatalog> {
        static BUNDLED: OnceLock<std::result::Result<SkillCatalog, String>> = OnceLock::new();
        BUNDLED
            .get_or_init(|| Self::bundled_uncached().map_err(|e| format!("{e:#}")))
            .clone()
            .map_err(|msg| anyhow!(msg))
    }

    fn bundled_uncached() -> Result<SkillCatalog> {
        let mut provenance = parse_provenance(BUNDLED_PROVENANCE)?;
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
            let provenance = provenance.remove(name);
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
            ..SkillCatalog::default()
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

    /// This catalog plus the skills `teammate` brings from the operator's
    /// machine, read from disk: its `operator_skills:` and its
    /// `plugin_skills:` ([`SkillCatalog::with_plugin_skills`]). `~/`
    /// expands against `home`, the launch's home. The spawn, `horch fleet`,
    /// the competition coordinator and the launch all extend the catalog
    /// through this call, so the ledger record and the launch's skill check
    /// (SKL-04) see the same skills.
    ///
    /// A skill this host does not have (no directory, or no `<dir>/<name>/`)
    /// is skipped and recorded in [`SkillCatalog::skipped_operator`]: the
    /// skills are exported per host with Xcode, so one host may lack them.
    /// Fails on a name that a catalog skill already has (one bundle
    /// directory cannot hold both), a name given twice, a subagent skill, a
    /// directory or skill that exists but cannot be read, an invalid
    /// SKILL.md, or a tree the marketplace digest rules refuse (a symlink,
    /// say). The name rules apply on every host, so a clash fails even where
    /// the skill is missing. A teammate without the field changes nothing.
    pub fn with_operator_skills(
        self,
        teammate: &Teammate,
        home: Option<&Path>,
    ) -> Result<SkillCatalog> {
        self.with_operator_dir(teammate, home)?
            .with_plugin_skills(teammate, home)
    }

    /// This catalog plus `teammate`'s `plugin_skills:`, one entry per named
    /// skill: id `<plugin>:<skill>` (the name the agent lists it under),
    /// version `<plugin version>+<digest12>` (`plugin+<digest12>` when the
    /// plugin has no version), and the marketplace tree digest of the skill
    /// directory in the original plugin. A catalog id has no `:`, so a
    /// plugin skill never shadows one. Nothing changes for a teammate
    /// without the field, with `disable_skills`, or on an agent that does
    /// not load skills as plugins. Fails on a plugin or skill that does not
    /// resolve, as the launch would.
    pub fn with_plugin_skills(
        mut self,
        teammate: &Teammate,
        home: Option<&Path>,
    ) -> Result<SkillCatalog> {
        if teammate.plugin_skills.is_empty()
            || teammate.disable_skills
            || teammate.agent.adapter().skill_namespace().is_none()
        {
            return Ok(self);
        }
        let who = &teammate.name;
        for named in crate::harness::claude_plugins::named_skills(teammate, home)
            .with_context(|| format!("{who}: plugin_skills"))?
        {
            let name = format!("{}:{}", named.plugin, named.skill);
            let id = SkillId::new(name.as_str())
                .with_context(|| format!("{who}: plugin skill '{name}'"))?;
            let digest: Digest = named
                .digest
                .parse()
                .with_context(|| format!("{who}: plugin skill '{name}' digest"))?;
            let version = format!(
                "{}+{}",
                named.version.as_deref().unwrap_or("plugin"),
                digest.short12()
            );
            self.entries.insert(
                name,
                CatalogEntry {
                    id,
                    version: SkillVersion(version),
                    source: CatalogSource::Plugin {
                        plugin: named.plugin_key,
                        dir: named.dir,
                    },
                    digest,
                    description: named.description,
                    provenance: None,
                    skill_file_bytes: named.skill_file_bytes,
                    files: Vec::new(),
                },
            );
        }
        Ok(self)
    }

    /// This catalog plus `teammate`'s `operator_skills:`.
    fn with_operator_dir(
        mut self,
        teammate: &Teammate,
        home: Option<&Path>,
    ) -> Result<SkillCatalog> {
        let Some(operator) = &teammate.operator_skills else {
            return Ok(self);
        };
        let who = &teammate.name;
        if operator.names.is_empty() {
            bail!("{who}: operator_skills names no skill");
        }
        for (i, name) in operator.names.iter().enumerate() {
            if operator.names[..i].contains(name) {
                bail!("{who}: operator skill '{name}' is named twice");
            }
            if self.entries.contains_key(name) {
                bail!(
                    "{who}: operator skill '{name}' has the name of a catalog skill; \
                     rename one, or name the catalog skill in skills:"
                );
            }
            SkillId::new(name.as_str())
                .with_context(|| format!("{who}: operator skill '{name}'"))?;
            if let Some(why) = subagent_skill(name, b"") {
                bail!(
                    "{who}: operator skill '{name}' is a subagent skill ({why}); \
                     fleet panes start no subagents"
                );
            }
        }
        let dir = crate::roster::expand_home(&operator.dir, home);
        let skip_all = !exists(&dir, || {
            format!("{who}: operator_skills dir '{}'", operator.dir)
        })?;
        if !skip_all {
            if !dir.is_dir() {
                bail!(
                    "{who}: operator_skills dir '{}' is not a directory",
                    operator.dir
                );
            }
            std::fs::read_dir(&dir).with_context(|| {
                format!(
                    "{who}: operator_skills dir '{}' cannot be read",
                    operator.dir
                )
            })?;
        }
        for name in &operator.names {
            let skill_dir = dir.join(name);
            let shown = format!("{}/{name}", operator.dir.trim_end_matches('/'));
            if skip_all || !exists(&skill_dir, || format!("{who}: operator skill '{name}'"))? {
                self.skipped_operator.push(SkippedOperatorSkill {
                    name: name.clone(),
                    path: shown,
                });
                continue;
            }
            let id = SkillId::new(name.as_str())
                .with_context(|| format!("{who}: operator skill '{name}'"))?;
            let skill_md = skill_dir.join("SKILL.md");
            let bytes = std::fs::read(&skill_md).map_err(|e| {
                anyhow!(
                    "{who}: operator skill '{name}' in '{}' has no readable {}: {e}",
                    operator.dir,
                    skill_md.display()
                )
            })?;
            let meta = validate_skill_md(&skill_md.display().to_string(), name, &bytes)?;
            if let Some(why) = subagent_skill(name, &bytes) {
                bail!(
                    "{who}: operator skill '{name}' is a subagent skill ({why}); \
                     fleet panes start no subagents"
                );
            }
            let digest: Digest = horch_marketplace::integrity::tree_digest(&skill_dir)
                .map_err(|e| anyhow!("{who}: operator skill '{name}': {e}"))?
                .parse()
                .with_context(|| format!("{who}: operator skill '{name}' digest"))?;
            self.entries.insert(
                name.clone(),
                CatalogEntry {
                    id,
                    version: SkillVersion(format!("operator+{}", digest.short12())),
                    source: CatalogSource::Operator { dir: dir.clone() },
                    digest,
                    description: meta.description,
                    provenance: None,
                    skill_file_bytes: bytes.len(),
                    files: Vec::new(),
                },
            );
        }
        Ok(self)
    }

    /// The operator skills that [`SkillCatalog::with_operator_skills`]
    /// skipped because this host does not have them, in `names:` order.
    pub fn skipped_operator(&self) -> &[SkippedOperatorSkill] {
        &self.skipped_operator
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

/// Whether `path` exists. `false` only for "not found"; any other error
/// (a parent that cannot be searched, say) is a real error, named by `what`.
fn exists(path: &Path, what: impl FnOnce() -> String) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow!("{}: {e}", what())),
    }
}

/// Skills that only work by starting a subagent, which a fleet pane must
/// not do. Apple's `device-interaction` is one by name. Any other skill is
/// one when its SKILL.md says so. The reason is `None` for a fleet skill.
fn subagent_skill(name: &str, skill_md: &[u8]) -> Option<&'static str> {
    if name == "device-interaction" {
        return Some("device-interaction runs as a subagent");
    }
    let body = String::from_utf8_lossy(skill_md);
    if body.contains("SUBAGENT skill") {
        Some("its SKILL.md says \"SUBAGENT skill\"")
    } else if body.contains("Agent tool") {
        Some("its SKILL.md uses the \"Agent tool\"")
    } else {
        None
    }
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
