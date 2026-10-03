//! The install pipeline: catalog, resolve, fetch, validate, verify,
//! materialize, lock. Every step before the lock works in a staging
//! directory that is removed when the install ends, so a failed install
//! leaves no lock entry and no staging behind.

use std::path::PathBuf;

use crate::catalog::Catalog;
use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::git::GitRunner;
use crate::integrity::tree_digest;
use crate::lockfile::{LockEntry, Lockfile};
use crate::manifest::SkillManifest;
use crate::model::{SkillId, SkillSource};
use crate::resolver::{ResolvedSource, Resolver};
use crate::source::fetch;
use crate::store::{Staging, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Catalog,
    Resolve,
    Fetch,
    Validate,
    Verify,
    Materialize,
    Lock,
}

/// A crash point for tests and drills. The caller maps `HORCH_FAULT` to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// `abort-after-materialize-before-lock`
    AbortAfterMaterializeBeforeLock,
}

impl FaultPoint {
    pub fn parse(s: &str) -> Option<Self> {
        (s == Self::AbortAfterMaterializeBeforeLock.as_str())
            .then_some(Self::AbortAfterMaterializeBeforeLock)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AbortAfterMaterializeBeforeLock => "abort-after-materialize-before-lock",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    pub fault: Option<FaultPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReinstallAction {
    /// The version directory existed and its digest matched the lock.
    Verified,
    /// The version directory was missing and was rebuilt from the pinned
    /// source.
    Rebuilt,
}

#[derive(Debug, Clone)]
pub struct InstalledSkill {
    pub entry: LockEntry,
    pub path: PathBuf,
}

pub struct Installer {
    store: Store,
    git: GitRunner,
    catalog: Catalog,
}

/// A fetched, validated and verified tree, ready to materialize.
struct Prepared {
    resolved: ResolvedSource,
    staging: Staging,
    tree: PathBuf,
    id: SkillId,
    digest: String,
}

fn record(observer: &mut Option<&mut Vec<Step>>, step: Step) {
    if let Some(steps) = observer {
        steps.push(step);
    }
}

impl Installer {
    pub fn new(store: Store, git: GitRunner, catalog: Catalog) -> Self {
        Self {
            store,
            git,
            catalog,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn install(&self, source: &SkillSource, opts: &InstallOptions) -> Result<LockEntry> {
        self.install_observed(source, opts, None)
    }

    /// `install`, recording each step as it starts.
    pub fn install_observed(
        &self,
        source: &SkillSource,
        opts: &InstallOptions,
        mut observer: Option<&mut Vec<Step>>,
    ) -> Result<LockEntry> {
        record(&mut observer, Step::Catalog);
        // Read the lock first, so a corrupt lock fails before any fetch.
        Lockfile::read(&self.store.lock_path())?;
        let prepared = self.prepare(source, &mut observer)?;
        let version = prepared.resolved.version(&prepared.digest);

        record(&mut observer, Step::Materialize);
        self.store
            .materialize(&prepared.tree, &prepared.staging, &prepared.id, &version)?;
        if opts.fault == Some(FaultPoint::AbortAfterMaterializeBeforeLock) {
            return Err(MarketplaceError::Fault(
                FaultPoint::AbortAfterMaterializeBeforeLock.as_str(),
            ));
        }

        record(&mut observer, Step::Lock);
        let lock_path = self.store.lock_path();
        let mut lock = Lockfile::read(&lock_path)?;
        let entry = LockEntry {
            id: prepared.id,
            source: prepared.resolved.source,
            requested_revision: prepared.resolved.requested_revision,
            resolved_commit: prepared.resolved.resolved_commit,
            version,
            digest: prepared.digest,
            installed_at: fsx::now_rfc3339(),
        };
        lock.upsert(entry.clone());
        lock.write(&lock_path)?;
        Ok(entry)
    }

    /// Resolve, fetch, validate and verify into a new staging directory.
    fn prepare(
        &self,
        source: &SkillSource,
        observer: &mut Option<&mut Vec<Step>>,
    ) -> Result<Prepared> {
        let staging = self.store.new_staging()?;

        record(observer, Step::Resolve);
        let resolved = Resolver {
            git: &self.git,
            catalog: &self.catalog,
            scratch: staging.path(),
        }
        .resolve(source)?;

        record(observer, Step::Fetch);
        let tree = fetch(&self.git, &resolved, staging.path())?;

        record(observer, Step::Validate);
        let manifest = SkillManifest::read(&tree, resolved.expected_name.as_deref())?;
        let id = SkillId::parse(&manifest.name)?;

        record(observer, Step::Verify);
        let digest = tree_digest(&tree)?;

        Ok(Prepared {
            resolved,
            staging,
            tree,
            id,
            digest,
        })
    }

    /// The installed skills: lock entries whose version directory exists.
    /// A version directory without a lock entry is not listed.
    pub fn installed(&self) -> Result<Vec<InstalledSkill>> {
        let lock = Lockfile::read(&self.store.lock_path())?;
        Ok(lock
            .skills
            .into_iter()
            .filter_map(|entry| {
                let path = self.store.version_dir(&entry.id, &entry.version);
                path.is_dir().then_some(InstalledSkill { entry, path })
            })
            .collect())
    }

    /// Make every locked skill present and verified. An existing version
    /// directory is only digest-checked, with no network. A missing one is
    /// rebuilt from the pinned source and must match the locked digest. The
    /// lock is not changed.
    pub fn reinstall_from_lock(&self) -> Result<Vec<(SkillId, ReinstallAction)>> {
        let lock = Lockfile::read(&self.store.lock_path())?;
        let mut out = Vec::new();
        for entry in &lock.skills {
            let dir = self.store.version_dir(&entry.id, &entry.version);
            if dir.is_dir() {
                self.check_digest(entry, &tree_digest(&dir)?)?;
                out.push((entry.id.clone(), ReinstallAction::Verified));
                continue;
            }
            let source = entry
                .source
                .clone()
                .pinned(entry.resolved_commit.as_deref());
            let prepared = self.prepare(&source, &mut None)?;
            if prepared.id != entry.id {
                return Err(MarketplaceError::InvalidManifest {
                    path: prepared.tree.join("SKILL.md"),
                    reason: format!("name '{}' does not match the lock", prepared.id),
                });
            }
            self.check_digest(entry, &prepared.digest)?;
            self.store
                .materialize(&prepared.tree, &prepared.staging, &entry.id, &entry.version)?;
            out.push((entry.id.clone(), ReinstallAction::Rebuilt));
        }
        Ok(out)
    }

    fn check_digest(&self, entry: &LockEntry, actual: &str) -> Result<()> {
        if actual == entry.digest {
            Ok(())
        } else {
            Err(MarketplaceError::DigestMismatch {
                id: entry.id.to_string(),
                expected: entry.digest.clone(),
                actual: actual.to_owned(),
            })
        }
    }
}
