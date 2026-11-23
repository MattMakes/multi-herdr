use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::model::{SkillId, SkillVersion};

const LOCK_WAIT: Duration = Duration::from_secs(60);
const STALE_AFTER: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    contentions: Arc<AtomicUsize>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            contentions: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn lock_contentions(&self) -> usize {
        self.contentions.load(Ordering::SeqCst)
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

    pub fn lock(&self) -> Result<StoreLock> {
        self.lock_within(LOCK_WAIT, STALE_AFTER)
    }

    pub fn lock_within(&self, wait: Duration, stale_after: Duration) -> Result<StoreLock> {
        self.make_root()?;
        let dir = self.guard_path();
        let deadline = Instant::now() + wait;
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => break Ok(StoreLock { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    self.contentions.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => break Err(MarketplaceError::io(dir.display().to_string(), e)),
            }
            if self.clear_stale(&dir, stale_after) {
                continue;
            }
            if Instant::now() >= deadline {
                break Err(timed_out(&dir));
            }
            thread::sleep(LOCK_POLL);
        }
    }

    fn make_root(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .map_err(|e| MarketplaceError::io(self.root.display().to_string(), e))
    }

    fn clear_stale(&self, dir: &Path, stale_after: Duration) -> bool {
        let stale = guard_age(dir).is_some_and(|age| age > stale_after);
        if stale {
            let aside = self.root.join(format!(".lock-stale-{}", fsx::random_hex()));
            if fs::rename(dir, &aside).is_ok() {
                let _ = fsx::remove_dir_if_exists(&aside);
            }
        }
        stale
    }

    pub fn version_dir(&self, id: &SkillId, version: &SkillVersion) -> PathBuf {
        self.skills_dir().join(id.as_str()).join(&version.0)
    }

    pub fn new_staging(&self) -> Result<Staging> {
        let parent = self.staging_dir();
        fs::create_dir_all(&parent)
            .map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
        let dir = parent.join(fsx::random_hex());
        fs::create_dir(&dir).map_err(|e| MarketplaceError::io(dir.display().to_string(), e))?;
        Ok(Staging { dir })
    }

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

fn timed_out(dir: &Path) -> MarketplaceError {
    let timed_out = std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "another install holds the store lock; remove the directory if no install runs",
    );
    MarketplaceError::io(dir.display().to_string(), timed_out)
}

fn guard_age(dir: &Path) -> Option<Duration> {
    let modified = fs::metadata(dir).and_then(|meta| meta.modified()).ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    )
}

#[derive(Debug)]
pub struct StoreLock {
    dir: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

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
