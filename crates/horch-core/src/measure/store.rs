//! The event log on disk (dataset design §4.2, MEA-03, MEA-04).
//!
//! `events/YYYY-MM-DD.jsonl`, one [`EventEnvelope`] per `\n`-terminated
//! line, files 0600. Appends happen under the `events.lock` [`DirLock`]
//! and are `sync_data`'d one by one. A crash can leave a torn last line;
//! readers skip and count it, and the next append terminates it first so it
//! never swallows a whole event.
//!
//! This is an adapter: it does file I/O, so the domain modules never do.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::fsx::{self, DirLock, DirLockGuard};
use crate::measure::event::EventEnvelope;
use crate::measure::paths::DatasetPaths;
use crate::telemetry::cursor::{poll_lines, Cursor};

/// The `DirLock` name under the dataset root: `<root>/events.lock/`.
pub const EVENTS_LOCK: &str = "events";
/// A lock this old belongs to a hung writer and is broken.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60);
/// How long an append waits for the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Fault injection for crash tests. The B3 coordinator maps
/// `HORCH_FAULT=abort-after-event-append` onto `abort_after_append`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreOptions {
    /// Abort the process right after an event line is durable.
    pub abort_after_append: bool,
}

/// Every readable event, in file (day) order then line order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadEvents {
    pub events: Vec<EventEnvelope>,
    /// Lines skipped: a last line without `\n`, or a line that is not an
    /// envelope.
    pub torn_lines: u32,
}

/// One line as an envelope, or `None` for a torn or foreign line.
pub fn parse_line(line: &[u8]) -> Option<EventEnvelope> {
    serde_json::from_slice(line).ok()
}

/// The `*.jsonl` files of the events dir, sorted by name (= by UTC day).
pub fn event_files(paths: &DatasetPaths) -> Result<Vec<PathBuf>> {
    let dir = paths.events_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    Ok(files)
}

/// Read every event file in full.
pub fn read_all(paths: &DatasetPaths) -> Result<ReadEvents> {
    let mut out = ReadEvents::default();
    for file in event_files(paths)? {
        let bytes = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
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
            match parse_line(line).filter(|_| terminated) {
                Some(env) => out.events.push(env),
                None => out.torn_lines += 1,
            }
        }
    }
    Ok(out)
}

/// The idempotency index: every key seen, with the first envelope that
/// carried it. Refreshed from the bytes appended since the last refresh, by
/// this process or another.
#[derive(Debug, Default)]
pub struct EventIndex {
    by_key: HashMap<String, EventEnvelope>,
    cursors: BTreeMap<PathBuf, Cursor>,
}

impl EventIndex {
    /// Read whatever every event file gained since the last call.
    pub fn refresh(&mut self, paths: &DatasetPaths) -> Result<()> {
        for file in event_files(paths)? {
            let cursor = self.cursors.entry(file.clone()).or_default();
            let by_key = &mut self.by_key;
            poll_lines(&file, cursor, |_, _, line| {
                if let Some(env) = parse_line(line.as_bytes()) {
                    by_key.entry(env.idempotency_key.clone()).or_insert(env);
                }
            })
            .with_context(|| format!("reading {}", file.display()))?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&EventEnvelope> {
        self.by_key.get(key)
    }

    pub fn insert(&mut self, env: EventEnvelope) {
        self.by_key
            .entry(env.idempotency_key.clone())
            .or_insert(env);
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

/// Take the events lock. Hold it across refresh, check and append.
pub fn lock(paths: &DatasetPaths) -> Result<DirLockGuard> {
    DirLock::acquire(paths.root(), EVENTS_LOCK, LOCK_STALE_AFTER, LOCK_TIMEOUT)
        .context("taking the dataset events lock")
}

/// Append one envelope to its day file and make it durable. The caller
/// holds the events lock.
pub fn append(paths: &DatasetPaths, file: &Path, env: &EventEnvelope) -> Result<()> {
    fsx::ensure_private_dir(&paths.events_dir())?;
    let mut line = serde_json::to_string(env).context("serializing an event")?;
    line.push('\n');

    let existed = file.exists();
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(fsx::PRIVATE_FILE);
    }
    let mut f = opts
        .open(file)
        .with_context(|| format!("opening {}", file.display()))?;

    // A torn last line from a crashed writer: end it, so this event starts
    // on a line of its own and the torn one stays a single skipped line.
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
        .with_context(|| format!("appending to {}", file.display()))?;
    if !existed {
        sync_dir(&paths.events_dir())?;
    }
    Ok(())
}

/// Make a new directory entry durable.
fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("syncing {}", dir.display()))?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
