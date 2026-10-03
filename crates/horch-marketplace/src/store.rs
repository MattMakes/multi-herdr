//! The on-disk store. The caller passes the root
//! (`${XDG_DATA_HOME:-~/.local/share}/horch/`). Layout:
//!
//! ```text
//! <root>/staging/<random hex>/      one per install; removed when it ends
//! <root>/skills/<id>/<version>/     materialized skills
//! <root>/marketplace.lock           the lock (see `lockfile`)
//! ```
//!
//! A version directory without a lock entry is ignored by readers and
//! replaced by the next install of that version.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::model::{SkillId, SkillVersion};

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
