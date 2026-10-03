//! `marketplace.lock`: `{ "version": 1, "skills": [ ... ] }`, one entry
//! per skill id, sorted by id, replaced atomically.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::model::{SkillId, SkillSource, SkillVersion};

pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockEntry {
    pub id: SkillId,
    pub source: SkillSource,
    /// The revision text the user asked for (`HEAD` when none); `null` for
    /// local and bundled sources.
    pub requested_revision: Option<String>,
    /// The full 40-hex commit for a git source; `null` otherwise.
    pub resolved_commit: Option<String>,
    pub version: SkillVersion,
    /// The tree digest, `sha256:<hex>`.
    pub digest: String,
    /// RFC 3339 UTC.
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    pub skills: Vec<LockEntry>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: LOCK_VERSION,
            skills: Vec::new(),
        }
    }
}

impl Lockfile {
    /// Read the lock. A missing file is an empty lock.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(MarketplaceError::io(path.display().to_string(), e)),
        };
        let lock: Self =
            serde_json::from_slice(&bytes).map_err(|e| MarketplaceError::Lockfile {
                path: path.to_owned(),
                reason: e.to_string(),
            })?;
        if lock.version != LOCK_VERSION {
            return Err(MarketplaceError::Lockfile {
                path: path.to_owned(),
                reason: format!(
                    "lock version {} is not supported (expected {LOCK_VERSION})",
                    lock.version
                ),
            });
        }
        Ok(lock)
    }

    /// Write the lock atomically, entries sorted by id.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut lock = self.clone();
        lock.skills.sort_by(|a, b| a.id.cmp(&b.id));
        let mut json =
            serde_json::to_vec_pretty(&lock).map_err(|e| MarketplaceError::Lockfile {
                path: path.to_owned(),
                reason: e.to_string(),
            })?;
        json.push(b'\n');
        fsx::atomic_write(path, &json)
    }

    pub fn get(&self, id: &SkillId) -> Option<&LockEntry> {
        self.skills.iter().find(|e| &e.id == id)
    }

    /// Insert `entry`, replacing an entry with the same id.
    pub fn upsert(&mut self, entry: LockEntry) {
        self.skills.retain(|e| e.id != entry.id);
        self.skills.push(entry);
        self.skills.sort_by(|a, b| a.id.cmp(&b.id));
    }
}
