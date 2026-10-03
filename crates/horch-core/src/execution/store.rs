//! The execution store: a project's ledger file, read and written whole.
//!
//! One JSON array of [`LedgerRecordV1`] at `<state_root>/<slug>.json`, the
//! path, slug rule and pretty-printed format the bash implementation and
//! every earlier binary used. A read-modify-write holds [`DirLock`] on
//! `<slug>.json.lock/`, the directory the old mkdir spinlock created, so an
//! old binary and a new one still exclude each other. Writes go through
//! [`fsx::write_atomic`]: temp file, fsync, rename, directory fsync.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::execution::legacy::LedgerRecordV1;
use crate::execution::model::ExecutionStatus;
use crate::fsx::{self, DirLock};
use crate::skills::activation::ResolvedSkillRef;

/// A lock older than this is broken: panes can be killed mid-write, and the
/// old spinlock broke after about 15 seconds too.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(15);
/// How long a writer waits for a live lock before it gives up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(60);
/// The ledger file's mode: what `std::fs::write` gave it under the usual
/// umask before the store wrote it.
const LEDGER_MODE: u32 = 0o644;

/// Turn a project path into a filename, matching `tr -c 'A-Za-z0-9' '-'`.
///
/// Operates on bytes, exactly as `tr` does, so a multi-byte character becomes one
/// `-` per byte and slugs computed by the bash version still resolve.
pub fn slug(project: &str) -> String {
    project
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect()
}

/// A project's executions on disk.
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    path: PathBuf,
}

impl ExecutionStore {
    /// The store for an explicit state root and project path.
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        Self {
            path: state_root.as_ref().join(format!("{}.json", slug(project))),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All records, oldest write order preserved. An absent or empty file reads
    /// as an empty ledger, matching the bash `[]` default.
    pub fn read(&self) -> Result<Vec<LedgerRecordV1>> {
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return Ok(Vec::new());
        };
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing ledger {}", self.path.display()))
    }

    /// The bytes [`ExecutionStore::write`] puts on disk for `records`:
    /// `to_string_pretty`, no trailing newline, as every earlier version.
    pub fn render_json(records: &[LedgerRecordV1]) -> Result<String> {
        Ok(serde_json::to_string_pretty(records)?)
    }

    /// Replace the file with `records`, durably. A record's `status` is
    /// brought in line with its `state` first.
    fn write(&self, records: &mut [LedgerRecordV1]) -> Result<()> {
        for r in records.iter_mut() {
            r.sync_legacy_status();
        }
        let json = Self::render_json(records)?;
        fsx::write_atomic(&self.path, json.as_bytes(), LEDGER_MODE)
            .with_context(|| format!("replacing {}", self.path.display()))
    }

    /// Take the lock, mutate the records, write them back.
    pub fn update<T>(&self, f: impl FnOnce(&mut Vec<LedgerRecordV1>) -> Result<T>) -> Result<T> {
        let root = self
            .path
            .parent()
            .context("ledger path has no parent directory")?;
        let name = self
            .path
            .file_name()
            .context("ledger path has no file name")?
            .to_string_lossy();
        let _guard = DirLock::acquire(root, &name, LOCK_STALE_AFTER, LOCK_TIMEOUT)
            .with_context(|| format!("locking ledger {}", self.path.display()))?;
        let mut records = self.read()?;
        let out = f(&mut records)?;
        self.write(&mut records)?;
        Ok(out)
    }

    /// Mutate every record addressed by `key` (record id or session id).
    /// Fails, naming the file, when none matches.
    pub fn update_key(&self, key: &str, mut f: impl FnMut(&mut LedgerRecordV1)) -> Result<()> {
        self.update(|records| {
            if !records.iter().any(|r| r.matches(key)) {
                bail!("no record matches '{key}' in {}", self.path.display())
            }
            records
                .iter_mut()
                .filter(|r| r.matches(key))
                .for_each(&mut f);
            Ok(())
        })
    }

    /// Append a record as given. Lifecycle defaults are the caller's job.
    pub fn insert(&self, record: LedgerRecordV1) -> Result<()> {
        self.update(|records| {
            records.push(record);
            Ok(())
        })
    }

    /// The newest record addressed by `key`.
    pub fn get(&self, key: &str) -> Result<LedgerRecordV1> {
        self.read()?
            .into_iter()
            .filter(|r| r.matches(key))
            .next_back()
            .with_context(|| format!("no record matches {key}"))
    }

    /// Set the typed status of the records addressed by `key`; `status`
    /// follows it. A terminal state stamps `finished_at` once.
    pub fn set_state(&self, key: &str, state: ExecutionStatus) -> Result<()> {
        let at = crate::clock::now_stamp();
        self.update_key(key, |r| {
            if state.is_terminal() {
                r.finished_at.get_or_insert_with(|| at.clone());
            } else {
                r.finished_at = None;
            }
            r.set_state(state.clone());
            r.updated_at = at.clone();
        })
    }

    /// Record the exact skills the execution was briefed with (SKL-04).
    pub fn set_skills(&self, key: &str, skills: Vec<ResolvedSkillRef>) -> Result<()> {
        self.update_key(key, |r| r.skills = skills.clone())
    }

    /// Records whose typed status is live: starting or running.
    pub fn live(&self) -> Result<Vec<LedgerRecordV1>> {
        Ok(self
            .read()?
            .into_iter()
            .filter(|r| r.execution_status().is_live())
            .collect())
    }
}
