//! The install pipeline: catalog, resolve, fetch, validate, verify,
//! materialize, lock. Every step before the lock works in a staging
//! directory that is removed when the install ends, so a failed install
//! leaves no lock entry and no staging behind.

use std::path::PathBuf;

use crate::catalog::Catalog;
use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::git::GitRunner;
use crate::integrity::{check_relative_path, tree_digest};
use crate::lockfile::{LockEntry, Lockfile};
use crate::manifest::SkillManifest;
use crate::model::{parse_source, GitRevision, SkillId, SkillSource, SkillVersion};
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
    /// `store_root` is `${XDG_DATA_HOME:-~/.local/share}/horch/`; the caller
    /// resolves it.
    pub fn new(store_root: PathBuf, git: GitRunner) -> Self {
        Self {
            store: Store::new(store_root),
            git,
            catalog: Catalog::new(),
        }
    }

    /// The bundled skills a `bundled:<name>` source can install.
    pub fn with_catalog(mut self, catalog: Catalog) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Install `source` and pin it in the lock. `observer` records each
    /// step as it starts.
    pub fn install(
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

        // One install at a time changes the store: the same version directory
        // and the lock read-modify-write must not interleave.
        let _guard = self.store.lock()?;
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
            id: prepared.id.to_string(),
            source: prepared.resolved.source.to_spec(),
            requested_revision: prepared.resolved.requested_revision,
            resolved_commit: prepared.resolved.resolved_commit,
            version: version.0,
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
                let path = self.entry_dir(&entry).ok()?;
                path.is_dir().then_some(InstalledSkill { entry, path })
            })
            .collect())
    }

    /// Install the locked skills again at their requested revision: every
    /// skill, or only `id`. A moved branch or tag gets its new commit; a
    /// local or bundled source gets its current files.
    pub fn update(&self, id: Option<&str>) -> Result<Vec<LockEntry>> {
        let lock = Lockfile::read(&self.store.lock_path())?;
        if let Some(id) = id {
            if lock.get(id).is_none() {
                return Err(MarketplaceError::BadSource(format!(
                    "skill '{id}' is not in the lock"
                )));
            }
        }
        let mut out = Vec::new();
        for entry in lock
            .skills
            .iter()
            .filter(|e| id.is_none_or(|id| e.id == id))
        {
            let mut source = parse_source(&entry.source)?;
            if let Some(rev) = &entry.requested_revision {
                source = source.with_revision(GitRevision::from_requested(rev)?);
            }
            out.push(self.install(&source, &InstallOptions::default(), None)?);
        }
        Ok(out)
    }

    /// Make every locked skill present and verified, and return the lock
    /// entries. An existing version directory is only digest-checked, with
    /// no network. A missing one is rebuilt from the pinned source and must
    /// match the locked digest. The lock is not changed.
    pub fn reinstall_from_lock(&self) -> Result<Vec<LockEntry>> {
        let lock = Lockfile::read(&self.store.lock_path())?;
        for entry in &lock.skills {
            let dir = self.entry_dir(entry)?;
            if dir.is_dir() {
                check_digest(entry, &tree_digest(&dir)?)?;
                continue;
            }
            let mut source = parse_source(&entry.source)?;
            if let Some(commit) = &entry.resolved_commit {
                source = source.with_revision(GitRevision::Commit(commit.clone()));
            }
            let prepared = self.prepare(&source, &mut None)?;
            if prepared.id.as_str() != entry.id {
                return Err(MarketplaceError::SkillMd(format!(
                    "name '{}' does not match the locked id '{}'",
                    prepared.id, entry.id
                )));
            }
            check_digest(entry, &prepared.digest)?;
            self.store.materialize(
                &prepared.tree,
                &prepared.staging,
                &prepared.id,
                &SkillVersion(entry.version.clone()),
            )?;
        }
        Ok(lock.skills)
    }

    /// `skills/<id>/<version>/` of a lock entry. The id and version are
    /// checked, because the lock file is input.
    fn entry_dir(&self, entry: &LockEntry) -> Result<PathBuf> {
        let id = SkillId::parse(&entry.id)?;
        check_relative_path(&entry.version)?;
        if entry.version.contains('/') {
            return Err(MarketplaceError::Traversal(PathBuf::from(&entry.version)));
        }
        Ok(self
            .store
            .version_dir(&id, &SkillVersion(entry.version.clone())))
    }
}

fn check_digest(entry: &LockEntry, actual: &str) -> Result<()> {
    if actual == entry.digest {
        Ok(())
    } else {
        Err(MarketplaceError::DigestMismatch {
            expected: entry.digest.clone(),
            actual: actual.to_owned(),
        })
    }
}
