//! The on-disk store. The caller passes the root
//! (`${XDG_DATA_HOME:-~/.local/share}/horch/`). Layout:
//!
//! ```text
//! <root>/staging/<random hex>/      one per install; removed when it ends
//! <root>/skills/<id>/<version>/     materialized skills
//! <root>/marketplace.lock           the lock (see `lockfile`)
//! <root>/marketplace.lock.guard/    exists while an install changes the store
//! ```
//!
//! A version directory without a lock entry is ignored by readers and
//! replaced by the next install of that version.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::model::{SkillId, SkillVersion};

/// How long `Store::lock` waits for another install. It is longer than
/// `STALE_AFTER`, so a holder that died is broken before the wait ends.
const LOCK_WAIT: Duration = Duration::from_secs(60);
/// A guard directory older than this has no live holder: the guarded span is
/// a rename and one small file write.
const STALE_AFTER: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join("marketplace.lock")
    }

    fn guard_path(&self) -> PathBuf {
        self.root.join("marketplace.lock.guard")
    }

    /// Take the store lock: the span from the last read of `marketplace.lock`
    /// to its write, so two installs cannot each write from the same old
    /// lock. The lock is a directory made with an exclusive `create_dir`; it
    /// is removed when the guard drops. The crate cannot reuse the `DirLock`
    /// of the core crate, because that crate depends on this one, and it has
    /// no `libc` for `flock`. Waits up to 60 s, then fails with a
    /// `TimedOut` error that names the guard directory.
    pub fn lock(&self) -> Result<StoreLock> {
        self.lock_within(LOCK_WAIT, STALE_AFTER)
    }

    /// `lock` with explicit limits, for tests.
    pub fn lock_within(&self, wait: Duration, stale_after: Duration) -> Result<StoreLock> {
        fs::create_dir_all(&self.root)
            .map_err(|e| MarketplaceError::io(self.root.display().to_string(), e))?;
        let dir = self.guard_path();
        let deadline = Instant::now() + wait;
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(StoreLock { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(MarketplaceError::io(dir.display().to_string(), e)),
            }
            if guard_age(&dir).is_some_and(|age| age > stale_after) {
                // Rename first: of several waiters only one rename succeeds.
                let aside = self.root.join(format!(".lock-stale-{}", fsx::random_hex()));
                if fs::rename(&dir, &aside).is_ok() {
                    let _ = fsx::remove_dir_if_exists(&aside);
                }
                continue;
            }
            if Instant::now() >= deadline {
                let timed_out = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "another install holds the store lock; remove the directory if no install runs",
                );
                return Err(MarketplaceError::io(dir.display().to_string(), timed_out));
            }
            thread::sleep(LOCK_POLL);
        }
    }

    pub fn version_dir(&self, id: &SkillId, version: &SkillVersion) -> PathBuf {
        self.skills_dir().join(id.as_str()).join(&version.0)
    }

    /// Create a fresh `staging/<random hex>/`. It is removed when the
    /// returned guard drops, on success and on every error.
    pub fn new_staging(&self) -> Result<Staging> {
        let parent = self.staging_dir();
        fs::create_dir_all(&parent)
            .map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
        let dir = parent.join(fsx::random_hex());
        fs::create_dir(&dir).map_err(|e| MarketplaceError::io(dir.display().to_string(), e))?;
        Ok(Staging { dir })
    }

    /// Move a verified staged skill directory to `skills/<id>/<version>/`
    /// with a rename. An existing directory there (an earlier install that
    /// never reached the lock, or the same version) is moved into `staging`
    /// first, so the target is never half written.
    pub fn materialize(
        &self,
        staged: &Path,
        staging: &Staging,
        id: &SkillId,
        version: &SkillVersion,
    ) -> Result<PathBuf> {
        let target = self.version_dir(id, version);
        let parent = target.parent().expect("version dir has a parent");
        fs::create_dir_all(parent)
            .map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
        if fs::symlink_metadata(&target).is_ok() {
            let old = staging.path().join("replaced");
            fs::rename(&target, &old)
                .map_err(|e| MarketplaceError::io(format!("move aside {}", target.display()), e))?;
        }
        fs::rename(staged, &target)
            .map_err(|e| MarketplaceError::io(format!("materialize {}", target.display()), e))?;
        fsx::sync_dir(parent).map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
        Ok(target)
    }
}

/// How long ago the guard directory changed, or `None` when it is gone.
fn guard_age(dir: &Path) -> Option<Duration> {
    let modified = fs::metadata(dir).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    )
}

/// The store lock. It releases when it drops.
#[derive(Debug)]
pub struct StoreLock {
    dir: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

/// A staging directory that removes itself on drop.
#[derive(Debug)]
pub struct Staging {
    dir: PathBuf,
}

impl Staging {
    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fsx::remove_dir_if_exists(&self.dir);
    }
}
