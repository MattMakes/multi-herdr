//! `marketplace.lock`: `{ "version": 1, "skills": [ ... ] }`, one entry
//! per skill id, sorted by id, replaced atomically.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{MarketplaceError, Result};
use crate::fsx;

pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockEntry {
    pub id: String,
    /// The source spec without `@rev`, as `parse_source` reads it:
    /// `bundled:<name>`, `/abs/path`, or `<url>[#<subdir>]`.
    pub source: String,
    /// The `@rev` text the user asked for (`HEAD` when none); `null` for
    /// local and bundled sources.
    pub requested_revision: Option<String>,
    /// The full 40-hex commit for a git source; `null` otherwise.
    pub resolved_commit: Option<String>,
    /// `git+<commit12>`, `local+<digest12>` or `bundled+<digest12>`.
    pub version: String,
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
        match fs::read(path) {
            Ok(bytes) => Self::parse(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(MarketplaceError::io(path.display().to_string(), e)),
        }
    }

    fn parse(bytes: &[u8]) -> Result<Self> {
        let lock: Self =
            serde_json::from_slice(bytes).map_err(|e| MarketplaceError::Lock(e.to_string()))?;
        if lock.version == LOCK_VERSION {
            Ok(lock)
        } else {
            Err(MarketplaceError::Lock(format!(
                "version {} is not supported (expected {LOCK_VERSION})",
                lock.version
            )))
        }
    }

    /// Write the lock atomically, entries sorted by id.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut lock = self.clone();
        lock.skills.sort_by(|a, b| a.id.cmp(&b.id));
        let mut json =
            serde_json::to_vec_pretty(&lock).map_err(|e| MarketplaceError::Lock(e.to_string()))?;
        json.push(b'\n');
        fsx::atomic_write(path, &json)
    }

    pub fn get(&self, id: &str) -> Option<&LockEntry> {
        self.skills.iter().find(|e| e.id == id)
    }

    /// Insert `entry`, replacing an entry with the same id.
    pub fn upsert(&mut self, entry: LockEntry) {
        self.skills.retain(|e| e.id != entry.id);
        self.skills.push(entry);
        self.skills.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_text(text: &str) -> Result<Lockfile> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marketplace.lock");
        fs::write(&path, text).unwrap();
        Lockfile::read(&path)
    }

    #[test]
    fn missing_file_is_an_empty_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = Lockfile::read(&dir.path().join("marketplace.lock")).unwrap();
        assert_eq!(
            lock,
            Lockfile {
                version: 1,
                skills: vec![]
            }
        );
    }

    #[test]
    fn unreadable_path_is_an_io_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let err = Lockfile::read(dir.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::Io(_)), "{err:?}");
        assert!(
            err.to_string().contains(&dir.path().display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn unsupported_version_is_refused() {
        let err = read_text(r#"{"version":2,"skills":[]}"#).unwrap_err();
        assert!(matches!(err, MarketplaceError::Lock(_)), "{err:?}");
        assert_eq!(
            err.to_string(),
            "marketplace.lock: version 2 is not supported (expected 1)"
        );
    }

    #[test]
    fn invalid_json_and_unknown_fields_are_lock_errors() {
        for text in [
            "not json",
            r#"{"version":1}"#,
            r#"{"version":1,"skills":[],"x":1}"#,
        ] {
            let err = read_text(text).unwrap_err();
            assert!(matches!(err, MarketplaceError::Lock(_)), "{text}: {err:?}");
        }
    }

    #[test]
    fn written_lock_reads_back_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marketplace.lock");
        let entry = |id: &str| LockEntry {
            id: id.into(),
            source: format!("bundled:{id}"),
            requested_revision: None,
            resolved_commit: None,
            version: "bundled+abc".into(),
            digest: "sha256:abc".into(),
            installed_at: "2026-01-01T00:00:00Z".into(),
        };
        let mut lock = Lockfile::default();
        lock.upsert(entry("b"));
        lock.upsert(entry("a"));
        lock.write(&path).unwrap();
        let back = Lockfile::read(&path).unwrap();
        let ids: Vec<_> = back.skills.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(back.get("a"), Some(&entry("a")));
    }
}
