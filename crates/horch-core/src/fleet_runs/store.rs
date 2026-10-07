//! The `fleet/` files on disk (fleet-dataset §3).
//!
//! `starts.jsonl`, `runs.jsonl` and `verdicts.jsonl`: one JSON value per
//! `\n`-terminated line, files 0600 in a 0700 directory. Every append holds
//! the `fleet.lock` `DirLock` and is `sync_data`'d. A crash can leave a torn
//! last line; [`read_rows`] skips and counts it, and the next append ends it
//! first, as the event log does (`measure::store`).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::fsx::{self, DirLock, DirLockGuard};
use crate::measure::paths::DatasetPaths;

/// The `DirLock` name in the fleet dir: `fleet/fleet.lock/`.
const FLEET_LOCK: &str = "fleet";
/// A lock this old belongs to a hung writer and is broken.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60);
/// How long an append waits for the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// One of the 3 fleet files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FleetFile {
    Starts,
    Runs,
    Verdicts,
}

impl FleetFile {
    pub fn name(self) -> &'static str {
        match self {
            FleetFile::Starts => "starts.jsonl",
            FleetFile::Runs => "runs.jsonl",
            FleetFile::Verdicts => "verdicts.jsonl",
        }
    }

    pub fn path(self, paths: &DatasetPaths) -> PathBuf {
        paths.fleet_dir().join(self.name())
    }
}

/// Every readable row of one file, in line order.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadRows<T> {
    pub rows: Vec<T>,
    /// Lines skipped: a last line without `\n`, or a line that is not a row.
    pub torn_lines: u32,
}

impl<T> Default for ReadRows<T> {
    fn default() -> Self {
        ReadRows {
            rows: Vec::new(),
            torn_lines: 0,
        }
    }
}

/// Create every missing level of `fleet/`, each 0700. A directory that
/// exists keeps its mode.
pub fn ensure_fleet_dir(paths: &DatasetPaths) -> Result<()> {
    let dir = paths.fleet_dir();
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = paths.root().parent().filter(|p| !p.exists()) {
        fsx::ensure_private_dir(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    crate::competition::observe::ensure_dirs(paths, &dir)
}

/// Take the fleet lock, creating `fleet/` first. Hold it across a check and
/// its append.
pub fn lock(paths: &DatasetPaths) -> Result<DirLockGuard> {
    ensure_fleet_dir(paths)?;
    DirLock::acquire(
        &paths.fleet_dir(),
        FLEET_LOCK,
        LOCK_STALE_AFTER,
        LOCK_TIMEOUT,
    )
    .context("taking the fleet lock")
}

/// Read every row of `file`. A missing file has no rows.
pub fn read_rows<T: DeserializeOwned>(
    paths: &DatasetPaths,
    file: FleetFile,
) -> Result<ReadRows<T>> {
    let path = file.path(paths);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ReadRows::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut out = ReadRows::default();
    let mut rest: &[u8] = &bytes;
    while !rest.is_empty() {
        let (line, terminated) = match rest.iter().position(|&b| b == b'\n') {
            Some(n) => {
                let line = &rest[..n];
                rest = &rest[n + 1..];
                (line, true)
            }
            None => {
                let line = rest;
                rest = &[];
                (line, false)
            }
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<T>(line)
            .ok()
            .filter(|_| terminated)
        {
            Some(row) => out.rows.push(row),
            None => out.torn_lines += 1,
        }
    }
    Ok(out)
}

/// Append `row` to `file` under the fleet lock.
pub fn append<T: Serialize>(paths: &DatasetPaths, file: FleetFile, row: &T) -> Result<()> {
    let _guard = lock(paths)?;
    append_locked(paths, file, row)
}

/// Append `row` to `file`. The caller holds [`lock`].
pub fn append_locked<T: Serialize>(paths: &DatasetPaths, file: FleetFile, row: &T) -> Result<()> {
    let path = file.path(paths);
    let mut line = serde_json::to_string(row).context("serializing a fleet row")?;
    line.push('\n');
    append_line(&path, line)
}

fn append_line(path: &Path, mut line: String) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(fsx::PRIVATE_FILE);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;

    // A torn last line from a crashed writer: end it, so this row starts on
    // a line of its own and the torn one stays a single skipped line.
    let len = f.metadata()?.len();
    if len > 0 {
        let mut last = [0u8; 1];
        f.seek(SeekFrom::Start(len - 1))?;
        f.read_exact(&mut last)?;
        if last[0] != b'\n' {
            line.insert(0, '\n');
        }
    }
    f.write_all(line.as_bytes())
        .and_then(|()| f.sync_data())
        .with_context(|| format!("appending to {}", path.display()))
}
