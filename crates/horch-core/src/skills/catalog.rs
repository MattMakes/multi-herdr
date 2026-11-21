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

use super::{
    BUNDLED_COPIED, BUNDLED_SKILL_DIGESTS, BUNDLED_SKILL_EXECUTABLES, BUNDLED_SKILL_FILES,
};
use crate::digest::Digest;
use crate::ids::SkillId;
use crate::roster::{OperatorSkills, Teammate};

pub use horch_marketplace as marketplace;
use horch_marketplace::SkillVersion;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatalogSource {
    Bundled,
    Marketplace {
        source: String,
        resolved_commit: Option<String>,
    },
    Operator {
        dir: PathBuf,
    },
    Plugin {
        plugin: String,
        dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Copied {
    pub copied_files: Vec<String>,
    #[serde(default)]
    pub verbatim: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_exempt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: SkillId,
    pub version: SkillVersion,
    pub source: CatalogSource,
    pub digest: Digest,
    pub description: String,
    pub copied: Option<Copied>,
    pub skill_file_bytes: usize,
    pub(crate) files: Vec<(&'static str, &'static [u8])>,
    pub(crate) executable: Vec<&'static str>,
}

impl CatalogEntry {
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

    pub fn is_plugin(&self) -> bool {
        matches!(self.source, CatalogSource::Plugin { .. })
    }

    pub fn is_operator(&self) -> bool {
        matches!(self.source, CatalogSource::Operator { .. })
    }

    pub fn operator_dir(&self) -> Option<PathBuf> {
        match &self.source {
            CatalogSource::Operator { dir } => Some(dir.join(self.id.as_str())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    entries: BTreeMap<String, CatalogEntry>,
    store_root: Option<PathBuf>,
    skipped_operator: Vec<SkippedOperatorSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedOperatorSkill {
    pub name: String,
    pub path: String,
}

impl SkippedOperatorSkill {
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
struct CopiedFile {
    skills: Vec<CopiedSkill>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopiedSkill {
    name: String,
    copied_files: Vec<String>,
    #[serde(default)]
    verbatim: bool,
    #[serde(default)]
    budget_exempt: Option<String>,
}

pub fn parse_copied(text: &str) -> Result<BTreeMap<String, Copied>> {
    let file: CopiedFile = serde_json::from_str(text).context("skills/copied.json")?;
    let mut out = BTreeMap::new();
    for s in file.skills {
        check_copied(&s)?;
        let entry = Copied {
            copied_files: s.copied_files,
            verbatim: s.verbatim,
            budget_exempt: s.budget_exempt,
        };
        if out.insert(s.name.clone(), entry).is_some() {
            bail!("copied '{}': duplicate entry", s.name);
        }
    }
    Ok(out)
}

fn check_copied(s: &CopiedSkill) -> Result<()> {
    let name = &s.name;
    for path in &s.copied_files {
        if matches!(path.as_bytes().first(), None | Some(b'/'))
            || path.split('/').any(|c| c == "..")
        {
            bail!("copied '{name}': '{path}' is not a path inside the skill directory");
        }
    }
    if let Some(reason) = &s.budget_exempt {
        if reason.trim().is_empty() {
            bail!("copied '{name}': budget_exempt needs a reason");
        }
        if s.verbatim {
            bail!("copied '{name}': a verbatim skill is already exempt; drop budget_exempt");
        }
    }
    Ok(())
}

impl SkillCatalog {
    pub fn bundled() -> Result<SkillCatalog> {
        static BUNDLED: OnceLock<std::result::Result<SkillCatalog, String>> = OnceLock::new();
        BUNDLED
            .get_or_init(|| Self::bundled_uncached().map_err(|e| format!("{e:#}")))
            .clone()
            .map_err(|msg| anyhow!(msg))
    }

    fn bundled_uncached() -> Result<SkillCatalog> {
        let mut copied = parse_copied(BUNDLED_COPIED)?;
        let mut entries = BTreeMap::new();
        let skills = BUNDLED_SKILL_FILES.iter().filter_map(|&(path, bytes)| {
            let name = path.strip_suffix("/SKILL.md")?;
            (!name.contains('/')).then_some((path, name, bytes))
        });
        for (path, name, bytes) in skills {
            let entry = bundled_entry(path, name, bytes, copied.remove(name))?;
            entries.insert(name.to_owned(), entry);
        }
        if let Some(name) = copied.keys().next() {
            bail!("skills/copied.json: '{name}' is not a bundled skill");
        }
        Ok(SkillCatalog {
            entries,
            ..SkillCatalog::default()
        })
    }

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

    pub fn store_dir(&self, entry: &CatalogEntry) -> Result<Option<PathBuf>> {
        match (&entry.source, &self.store_root) {
            (CatalogSource::Marketplace { .. }, Some(root)) => {
                version_dir(root, entry.id.as_str(), &entry.version.0).map(Some)
            }
            _ => Ok(None),
        }
    }

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
                        executable: entry.executable.contains(path),
                    })
                    .collect(),
            });
        }
        Ok(out)
    }

    pub fn with_lock(mut self, lock: &[LockEntry]) -> Result<SkillCatalog> {
        for entry in lock {
            if entry.source.starts_with("bundled:") {
                continue;
            }
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
                    copied: None,
                    skill_file_bytes: 0,
                    files: Vec::new(),
                    executable: Vec::new(),
                },
            );
        }
        Ok(self)
    }

    pub(crate) fn with_host_skills(
        self,
        teammate: &Teammate,
        home: Option<&Path>,
    ) -> Result<SkillCatalog> {
        self.with_operator_dir(teammate, home)?
            .with_plugin_skills(teammate, home)
    }

    pub(crate) fn with_plugin_skills(
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
                    copied: None,
                    skill_file_bytes: named.skill_file_bytes,
                    files: Vec::new(),
                    executable: Vec::new(),
                },
            );
        }
        Ok(self)
    }

    fn with_operator_dir(
        mut self,
        teammate: &Teammate,
        home: Option<&Path>,
    ) -> Result<SkillCatalog> {
        let Some(operator) = &teammate.operator_skills else {
            return Ok(self);
        };
        let who = &teammate.name;
        let ids = self.check_operator_names(who, operator)?;
        let dir = operator_dir(who, operator, home)?;
        for (name, id) in operator.names.iter().zip(ids) {
            match load_operator_skill(who, operator, &dir, name, id)? {
                Some(entry) => {
                    self.entries.insert(name.clone(), entry);
                }
                None => self.skipped_operator.push(SkippedOperatorSkill {
                    name: name.clone(),
                    path: format!("{}/{name}", operator.dir.trim_end_matches('/')),
                }),
            }
        }
        Ok(self)
    }

    fn check_operator_names(&self, who: &str, operator: &OperatorSkills) -> Result<Vec<SkillId>> {
        let mut ids = Vec::new();
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
            ids.push(
                SkillId::new(name.as_str())
                    .with_context(|| format!("{who}: operator skill '{name}'"))?,
            );
            bail_if_subagent(who, name, b"")?;
        }
        Ok(ids)
    }

    pub fn skipped_operator(&self) -> &[SkippedOperatorSkill] {
        &self.skipped_operator
    }

    pub fn get(&self, id: &SkillId) -> Option<&CatalogEntry> {
        self.entries.get(id.as_str())
    }

    pub fn lookup(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.get(name)
    }

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

pub fn installer(data_root: &Path, git: &Path) -> Result<Installer> {
    Ok(Installer::new(data_root.to_path_buf(), GitRunner::new(git))
        .with_catalog(SkillCatalog::bundled()?.marketplace_catalog()?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreState {
    Ok,
    Missing,
    Tampered { actual: String },
    Invalid(String),
}

pub fn check_store(data_root: &Path) -> Result<Vec<(LockEntry, PathBuf, StoreState)>> {
    let lock_path = Store::new(data_root).lock_path();
    let lock = Lockfile::read(&lock_path).map_err(|e| anyhow!("{}: {e}", lock_path.display()))?;
    let mut out = Vec::new();
    for entry in lock.skills {
        let (dir, state) = match version_dir(data_root, &entry.id, &entry.version) {
            Err(e) => (PathBuf::new(), StoreState::Invalid(format!("{e:#}"))),
            Ok(dir) if !dir.is_dir() => (dir, StoreState::Missing),
            Ok(dir) => {
                let state = digest_state(&dir, &entry.digest);
                (dir, state)
            }
        };
        out.push((entry, dir, state));
    }
    Ok(out)
}

fn operator_dir(who: &str, operator: &OperatorSkills, home: Option<&Path>) -> Result<PathBuf> {
    let dir = crate::roster::expand_home(&operator.dir, home);
    if exists(&dir, || {
        format!("{who}: operator_skills dir '{}'", operator.dir)
    })? {
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
    Ok(dir)
}

fn load_operator_skill(
    who: &str,
    operator: &OperatorSkills,
    dir: &Path,
    name: &str,
    id: SkillId,
) -> Result<Option<CatalogEntry>> {
    let skill_dir = dir.join(name);
    if !exists(&skill_dir, || format!("{who}: operator skill '{name}'"))? {
        return Ok(None);
    }
    let (bytes, meta) = read_operator_skill_md(who, operator, name, &skill_dir)?;
    bail_if_subagent(who, name, &bytes)?;
    let digest = operator_skill_digest(who, name, &skill_dir)?;
    Ok(Some(CatalogEntry {
        id,
        version: SkillVersion(format!("operator+{}", digest.short12())),
        source: CatalogSource::Operator {
            dir: dir.to_path_buf(),
        },
        digest,
        description: meta.description,
        copied: None,
        skill_file_bytes: bytes.len(),
        files: Vec::new(),
        executable: Vec::new(),
    }))
}

fn operator_skill_digest(who: &str, name: &str, skill_dir: &Path) -> Result<Digest> {
    horch_marketplace::integrity::tree_digest(skill_dir)
        .map_err(|e| anyhow!("{who}: operator skill '{name}': {e}"))?
        .parse()
        .with_context(|| format!("{who}: operator skill '{name}' digest"))
}

fn read_operator_skill_md(
    who: &str,
    operator: &OperatorSkills,
    name: &str,
    skill_dir: &Path,
) -> Result<(Vec<u8>, Metadata)> {
    let skill_md = skill_dir.join("SKILL.md");
    let bytes = std::fs::read(&skill_md).map_err(|e| {
        anyhow!(
            "{who}: operator skill '{name}' in '{}' has no readable {}: {e}",
            operator.dir,
            skill_md.display()
        )
    })?;
    let meta = validate_skill_md(&skill_md.display().to_string(), name, &bytes)?;
    Ok((bytes, meta))
}

fn bail_if_subagent(who: &str, name: &str, skill_md: &[u8]) -> Result<()> {
    if let Some(why) = subagent_skill(name, skill_md) {
        bail!(
            "{who}: operator skill '{name}' is a subagent skill ({why}); \
             fleet panes start no subagents"
        );
    }
    Ok(())
}

fn digest_state(dir: &Path, want: &str) -> StoreState {
    let actual = horch_marketplace::integrity::tree_digest(dir).unwrap_or_else(|e| e.to_string());
    if actual == want {
        StoreState::Ok
    } else {
        StoreState::Tampered { actual }
    }
}

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

fn bundled_entry(
    path: &str,
    name: &str,
    bytes: &'static [u8],
    copied: Option<Copied>,
) -> Result<CatalogEntry> {
    let meta = validate_skill_md(path, name, bytes)?;
    let (files, executable) = bundled_files(name);
    let digest = BUNDLED_SKILL_DIGESTS
        .iter()
        .find_map(|&(n, d)| (n == name).then_some(Digest(d)))
        .with_context(|| format!("{path}: no build-time digest"))?;
    let id = SkillId::new(name).with_context(|| format!("{path}: skill id"))?;
    check_copied_present(name, copied.as_ref(), &files)?;
    Ok(CatalogEntry {
        id,
        version: SkillVersion(format!("bundled+{}", digest.short12())),
        source: CatalogSource::Bundled,
        digest,
        description: meta.description,
        copied,
        skill_file_bytes: bytes.len(),
        files,
        executable,
    })
}

#[allow(clippy::type_complexity)]
fn bundled_files(name: &str) -> (Vec<(&'static str, &'static [u8])>, Vec<&'static str>) {
    let prefix = format!("{name}/");
    let files = BUNDLED_SKILL_FILES
        .iter()
        .filter_map(|(p, b)| p.strip_prefix(&prefix).map(|rel| (rel, *b)))
        .collect();
    let executable = BUNDLED_SKILL_EXECUTABLES
        .iter()
        .filter_map(|p| p.strip_prefix(&prefix))
        .collect();
    (files, executable)
}

fn check_copied_present(
    name: &str,
    copied: Option<&Copied>,
    files: &[(&'static str, &'static [u8])],
) -> Result<()> {
    for rel in copied.iter().flat_map(|c| &c.copied_files) {
        if !files.iter().any(|(p, _)| p == rel) {
            bail!("skills/copied.json: '{name}' lists '{rel}', which the skill does not have");
        }
    }
    Ok(())
}

fn exists(path: &Path, what: impl FnOnce() -> String) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow!("{}: {e}", what())),
    }
}

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

fn validate_skill_md(path: &str, name: &str, bytes: &[u8]) -> Result<Metadata> {
    let meta = skill_md_metadata(path, bytes)?;
    if meta.name != name || !valid_skill_name(name) || !valid_description(&meta.description) {
        bail!("{path}: invalid skill name or description");
    }
    Ok(meta)
}

fn skill_md_metadata(path: &str, bytes: &[u8]) -> Result<Metadata> {
    let text = std::str::from_utf8(bytes)
        .context("skill is not UTF-8")?
        .replace("\r\n", "\n");
    let front = text
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .with_context(|| format!("{path}: missing YAML frontmatter"))?;
    serde_yaml::from_str(front).with_context(|| format!("{path}: invalid metadata"))
}

fn valid_skill_name(name: &str) -> bool {
    name.split('-').all(|part| {
        !part.is_empty() && part.bytes().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9'))
    })
}

fn valid_description(description: &str) -> bool {
    !description.trim().is_empty() && description.len() <= 1024
}

#[cfg(test)]
fn tree_digest(files: &[(&str, &[u8])], executable: &[&str]) -> Digest {
    use sha2::{Digest as _, Sha256};
    let mut lines: Vec<(&str, String, bool)> = files
        .iter()
        .map(|(rel, bytes)| {
            (
                *rel,
                crate::digest::sha256_bytes(bytes).hex(),
                executable.contains(rel),
            )
        })
        .collect();
    lines.sort();
    let mut tree = Sha256::new();
    for (rel, hash, exec) in &lines {
        tree.update(rel.as_bytes());
        tree.update([0u8]);
        tree.update(hash.as_bytes());
        if *exec {
            tree.update(b"\0x");
        }
        tree.update(b"\n");
    }
    Digest(tree.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_time_digests_equal_the_run_time_digest() {
        let catalog = SkillCatalog::bundled().unwrap();
        assert_eq!(catalog.entries.len(), BUNDLED_SKILL_DIGESTS.len());
        for (name, built) in BUNDLED_SKILL_DIGESTS {
            let prefix = format!("{name}/");
            let files: Vec<(&str, &[u8])> = BUNDLED_SKILL_FILES
                .iter()
                .filter_map(|(p, b)| p.strip_prefix(&prefix).map(|rel| (rel, *b)))
                .collect();
            assert!(!files.is_empty(), "{name}");
            let executable = &catalog.entries[*name].executable;
            assert_eq!(tree_digest(&files, executable), Digest(*built), "{name}");
            assert_eq!(catalog.entries[*name].digest, Digest(*built), "{name}");
        }
    }

    #[test]
    fn skl_11_digest_covers_the_execute_bit() {
        let files: [(&str, &[u8]); 2] = [
            ("SKILL.md", b"---\nname: demo\n---\n"),
            ("scripts/run.sh", b"#!/bin/sh\necho ok\n"),
        ];
        let plain = tree_digest(&files, &[]);
        let exec = tree_digest(&files, &["scripts/run.sh"]);
        assert_ne!(plain, exec);
        assert_ne!(exec, tree_digest(&files, &["SKILL.md"]));

        let dir = tempfile::tempdir().unwrap();
        for (rel, bytes) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let on_disk = horch_marketplace::integrity::tree_digest(dir.path()).unwrap();
        assert_eq!(plain.to_string(), on_disk);
    }

    #[cfg(unix)]
    #[test]
    fn skl_11_bundled_entry_lists_its_executable_files() {
        let catalog = SkillCatalog::bundled().unwrap();
        let entry = &catalog.entries["godot-build-verify"];
        assert!(entry.executable.contains(&"scripts/godot-run.sh"));
        assert!(!entry.executable.contains(&"SKILL.md"));
    }

    #[cfg(unix)]
    #[test]
    fn skl_11_marketplace_install_keeps_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let entry = installer(tmp.path(), Path::new("git"))
            .unwrap()
            .install(
                &horch_marketplace::SkillSource::parse("bundled:godot-build-verify").unwrap(),
                &horch_marketplace::InstallOptions::default(),
                None,
            )
            .unwrap();
        let dir = version_dir(tmp.path(), &entry.id, &entry.version).unwrap();
        let mode = |rel: &str| {
            std::fs::metadata(dir.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("scripts/godot-run.sh") & 0o111, 0o111);
        assert_eq!(mode("SKILL.md") & 0o111, 0);
        assert_eq!(
            horch_marketplace::integrity::tree_digest(&dir).unwrap(),
            entry.digest
        );
        let states = check_store(tmp.path()).unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].2, StoreState::Ok);
    }

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\nBody\n")
    }

    fn invalid(name: &str, md_name: &str, description: &str) -> bool {
        match validate_skill_md(
            "p/SKILL.md",
            name,
            skill_md(md_name, description).as_bytes(),
        ) {
            Ok(_) => false,
            Err(e) => {
                assert_eq!(
                    e.to_string(),
                    "p/SKILL.md: invalid skill name or description"
                );
                true
            }
        }
    }

    #[test]
    fn validate_skill_md_rejects_each_rule_alone() {
        let meta = validate_skill_md(
            "p/SKILL.md",
            "a-b2",
            skill_md("a-b2", "Does a thing.").as_bytes(),
        )
        .unwrap();
        assert_eq!(meta.description, "Does a thing.");
        assert!(invalid("a-b2", "other", "Does a thing."), "name differs");
        assert!(invalid("-ab", "-ab", "Does a thing."), "leading -");
        assert!(invalid("ab-", "ab-", "Does a thing."), "trailing -");
        assert!(invalid("a--b", "a--b", "Does a thing."), "doubled -");
        assert!(invalid("Ab", "Ab", "Does a thing."), "uppercase");
        assert!(invalid("a_b", "a_b", "Does a thing."), "underscore");
        assert!(invalid("a-b", "a-b", "''"), "blank description");
        assert!(invalid("a-b", "a-b", &"x".repeat(1025)), "long description");
        assert!(validate_skill_md("p", "a", skill_md("a", &"x".repeat(1024)).as_bytes()).is_ok());
    }

    #[test]
    fn validate_skill_md_names_a_missing_frontmatter() {
        let e = validate_skill_md("p/SKILL.md", "a", b"no frontmatter").unwrap_err();
        assert_eq!(e.to_string(), "p/SKILL.md: missing YAML frontmatter");
        let e = validate_skill_md("p/SKILL.md", "a", &[0xff, 0xfe]).unwrap_err();
        assert_eq!(e.to_string(), "skill is not UTF-8");
    }

    fn copied_error(json: &str) -> String {
        parse_copied(json).unwrap_err().to_string()
    }

    #[test]
    fn parse_copied_rejects_each_bad_entry() {
        let one = |extra: &str, files: &str| {
            format!(r#"{{"skills":[{{"name":"s","copied_files":[{files}]{extra}}}]}}"#)
        };
        for (json, shown) in [
            (r#""""#, ""),
            (r#""/abs""#, "/abs"),
            (r#""a/../b""#, "a/../b"),
        ] {
            assert_eq!(
                copied_error(&one("", json)),
                format!("copied 's': '{shown}' is not a path inside the skill directory")
            );
        }
        assert_eq!(
            copied_error(&one(r#","budget_exempt":" ""#, "")),
            "copied 's': budget_exempt needs a reason"
        );
        assert_eq!(
            copied_error(&one(r#","budget_exempt":"why","verbatim":true"#, "")),
            "copied 's': a verbatim skill is already exempt; drop budget_exempt"
        );
        let dup = r#"{"skills":[{"name":"s","copied_files":[]},{"name":"s","copied_files":[]}]}"#;
        assert_eq!(copied_error(dup), "copied 's': duplicate entry");
        let ok = parse_copied(&one(r#","budget_exempt":"why""#, r#""a/b.md""#)).unwrap();
        assert_eq!(ok["s"].copied_files, ["a/b.md"]);
        assert_eq!(ok["s"].budget_exempt.as_deref(), Some("why"));
    }

    fn lock_for(entries: Vec<LockEntry>, root: &Path) {
        let lock = Lockfile {
            skills: entries,
            ..Lockfile::default()
        };
        lock.write(&Store::new(root).lock_path()).unwrap();
    }

    fn lock_entry(id: &str, version: &str, digest: &str) -> LockEntry {
        LockEntry {
            id: id.into(),
            source: "https://example.com/r.git".into(),
            requested_revision: None,
            resolved_commit: Some("ab".repeat(20)),
            version: version.into(),
            digest: digest.into(),
            installed_at: "2026-10-02T12:00:00Z".into(),
        }
    }

    #[test]
    fn check_store_reports_invalid_missing_and_tampered_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let want = format!("sha256:{}", "cd".repeat(32));
        let tampered = tmp.path().join("skills/changed/git+1");
        std::fs::create_dir_all(&tampered).unwrap();
        std::fs::write(tampered.join("SKILL.md"), "x").unwrap();
        lock_for(
            vec![
                lock_entry("bad", "../up", &want),
                lock_entry("gone", "git+1", &want),
                lock_entry("changed", "git+1", &want),
            ],
            tmp.path(),
        );
        let mut got = check_store(tmp.path()).unwrap();
        assert_eq!(got.len(), 3);
        got.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        got.swap(1, 2);
        assert!(
            matches!(&got[0].2, StoreState::Invalid(m) if m.contains("version '../up' cannot name a directory")),
            "{:?}",
            got[0].2
        );
        assert_eq!(got[0].1, PathBuf::new());
        assert_eq!(got[1].2, StoreState::Missing);
        assert_eq!(got[1].1, tmp.path().join("skills/gone/git+1"));
        let actual = horch_marketplace::integrity::tree_digest(&tampered).unwrap();
        assert_eq!(got[2].2, StoreState::Tampered { actual });
    }

    #[test]
    fn check_store_accepts_a_matching_digest_and_reports_a_bad_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = tmp.path().join("skills/fine/git+1");
        std::fs::create_dir_all(&ok).unwrap();
        std::fs::write(ok.join("SKILL.md"), "x").unwrap();
        let digest = horch_marketplace::integrity::tree_digest(&ok).unwrap();
        lock_for(vec![lock_entry("fine", "git+1", &digest)], tmp.path());
        assert_eq!(check_store(tmp.path()).unwrap()[0].2, StoreState::Ok);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("SKILL.md", ok.join("link")).unwrap();
            let got = check_store(tmp.path()).unwrap();
            assert!(
                matches!(&got[0].2, StoreState::Tampered { actual } if !actual.starts_with("sha256:")),
                "{:?}",
                got[0].2
            );
        }
    }

    fn operator_teammate(dir: &str, names: &[&str]) -> Teammate {
        Teammate {
            name: "probe".into(),
            operator_skills: Some(crate::roster::OperatorSkills {
                dir: dir.into(),
                names: names.iter().map(|n| (*n).to_owned()).collect(),
            }),
            ..Teammate::default()
        }
    }

    fn operator_error(t: &Teammate) -> String {
        SkillCatalog::bundled()
            .unwrap()
            .with_operator_dir(t, None)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn with_operator_dir_refuses_bad_names_in_order() {
        assert_eq!(
            operator_error(&operator_teammate("/x", &[])),
            "probe: operator_skills names no skill"
        );
        assert_eq!(
            operator_error(&operator_teammate("/x", &["a", "a"])),
            "probe: operator skill 'a' is named twice"
        );
        assert_eq!(
            operator_error(&operator_teammate("/x", &["tdd"])),
            "probe: operator skill 'tdd' has the name of a catalog skill; \
             rename one, or name the catalog skill in skills:"
        );
        assert_eq!(
            operator_error(&operator_teammate("/x", &["device-interaction"])),
            "probe: operator skill 'device-interaction' is a subagent skill \
             (device-interaction runs as a subagent); fleet panes start no subagents"
        );
        assert_eq!(
            operator_error(&operator_teammate("/x", &["bad\u{1}name"])),
            "probe: operator skill 'bad\u{1}name'"
        );
    }

    #[test]
    fn with_operator_dir_refuses_a_file_for_a_directory_and_skips_a_missing_one() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("afile");
        std::fs::write(&file, "x").unwrap();
        let shown = file.display().to_string();
        assert_eq!(
            operator_error(&operator_teammate(&shown, &["mine"])),
            format!("probe: operator_skills dir '{shown}' is not a directory")
        );
        let gone = tmp.path().join("gone").display().to_string();
        let catalog = SkillCatalog::bundled()
            .unwrap()
            .with_operator_dir(&operator_teammate(&gone, &["mine"]), None)
            .unwrap();
        assert_eq!(
            catalog.skipped_operator(),
            [SkippedOperatorSkill {
                name: "mine".into(),
                path: format!("{gone}/mine"),
            }]
        );
    }

    #[test]
    fn with_operator_dir_refuses_a_subagent_skill_md_and_a_bad_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().display().to_string();
        let write = |name: &str, text: &str| {
            std::fs::create_dir_all(tmp.path().join(name)).unwrap();
            std::fs::write(tmp.path().join(name).join("SKILL.md"), text).unwrap();
        };
        write("agentic", &skill_md("agentic", "Use the Agent tool."));
        assert_eq!(
            operator_error(&operator_teammate(&dir, &["agentic"])),
            "probe: operator skill 'agentic' is a subagent skill \
             (its SKILL.md uses the \"Agent tool\"); fleet panes start no subagents"
        );
        write("wrong", &skill_md("other", "Fine."));
        assert!(operator_error(&operator_teammate(&dir, &["wrong"]))
            .ends_with("/wrong/SKILL.md: invalid skill name or description"));
        write("fine", &skill_md("fine", "Fine."));
        let catalog = SkillCatalog::bundled()
            .unwrap()
            .with_operator_dir(&operator_teammate(&dir, &["fine"]), None)
            .unwrap();
        let entry = catalog.lookup("fine").unwrap();
        assert!(entry.version.0.starts_with("operator+"));
        assert_eq!(entry.description, "Fine.");
        assert!(catalog.skipped_operator().is_empty());
    }

    #[test]
    fn with_operator_dir_digests_the_directory_of_the_skill_it_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().display().to_string();
        for name in ["fine", "other"] {
            std::fs::create_dir_all(tmp.path().join(name)).unwrap();
            std::fs::write(
                tmp.path().join(name).join("SKILL.md"),
                skill_md(name, "Fine."),
            )
            .unwrap();
        }
        std::fs::write(tmp.path().join("fine").join("notes.txt"), "more\n").unwrap();
        let catalog = SkillCatalog::bundled()
            .unwrap()
            .with_operator_dir(&operator_teammate(&dir, &["fine"]), None)
            .unwrap();
        let entry = catalog.lookup("fine").unwrap();
        let own = horch_marketplace::integrity::tree_digest(&tmp.path().join("fine")).unwrap();
        let sibling = horch_marketplace::integrity::tree_digest(&tmp.path().join("other")).unwrap();
        let above = horch_marketplace::integrity::tree_digest(tmp.path()).unwrap();
        assert_eq!(entry.digest.to_string(), own);
        assert_ne!(own, sibling);
        assert_ne!(own, above);
        assert_eq!(
            entry.version.0,
            format!("operator+{}", entry.digest.short12())
        );
    }

    #[cfg(unix)]
    #[test]
    fn with_operator_dir_refuses_a_skill_tree_the_digest_rules_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().display().to_string();
        std::fs::create_dir_all(tmp.path().join("linked")).unwrap();
        std::fs::write(
            tmp.path().join("linked").join("SKILL.md"),
            skill_md("linked", "Fine."),
        )
        .unwrap();
        std::os::unix::fs::symlink("SKILL.md", tmp.path().join("linked").join("alias")).unwrap();
        assert_eq!(
            operator_error(&operator_teammate(&dir, &["linked"])),
            "probe: operator skill 'linked': 'alias' is a symlink"
        );
    }
}
