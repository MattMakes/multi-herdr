//! The event log on disk (dataset design §4.2, MEA-03, MEA-04).
//!
//! `events/YYYY-MM-DD.jsonl`, one [`EventEnvelope`] per `\n`-terminated
//! line, files 0600. Appends happen under the `events.lock` `DirLock`
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
use crate::runtime::fault::Faults;
use crate::telemetry::cursor::{poll_lines, Cursor};

/// The `DirLock` name under the dataset root: `<root>/events.lock/`.
pub(crate) const EVENTS_LOCK: &str = "events";
/// A lock this old belongs to a hung writer and is broken.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60);
/// How long an append waits for the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Fault injection for crash tests: `HORCH_FAULT=abort-after-event-append`
/// sets `abort_after_append` (see [`StoreOptions::from_faults`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreOptions {
    /// Abort the process right after an event line is durable.
    pub abort_after_append: bool,
}

/// The `HORCH_FAULT` point that aborts right after an event append.
pub(crate) const ABORT_AFTER_EVENT_APPEND: &str = "abort-after-event-append";

impl StoreOptions {
    pub fn from_faults(faults: &Faults) -> StoreOptions {
        StoreOptions {
            abort_after_append: faults.has(ABORT_AFTER_EVENT_APPEND),
        }
    }
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
pub(crate) fn parse_line(line: &[u8]) -> Option<EventEnvelope> {
    serde_json::from_slice(line).ok()
}

/// The `*.jsonl` files of the events dir, sorted by name (= by UTC day).
pub(crate) fn event_files(paths: &DatasetPaths) -> Result<Vec<PathBuf>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn envelope(key: &str) -> EventEnvelope {
        serde_json::from_value(serde_json::json!({
            "schema_version": "1.0.0",
            "event_id": "0199a5b0-0000-7000-8000-000000000003",
            "kind": "judge.started",
            "occurred_at": "2026-10-02T12:00:00.000Z",
            "actor": "coordinator",
            "experiment_id": "0199a5b0-0000-7000-8000-000000000001",
            "idempotency_key": key,
            "payload": {},
        }))
        .unwrap()
    }

    fn line(key: &str) -> String {
        serde_json::to_string(&envelope(key)).unwrap()
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn keys(read: &ReadEvents) -> Vec<&str> {
        read.events
            .iter()
            .map(|e| e.idempotency_key.as_str())
            .collect()
    }

    /// Only regular `*.jsonl` files count, in name order; a missing events
    /// dir is empty and any other read error is an error.
    #[test]
    fn event_files_lists_sorted_jsonl_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "proj");
        assert!(event_files(&paths).unwrap().is_empty(), "no dir yet");

        let dir = paths.events_dir();
        std::fs::create_dir_all(dir.join("sub.jsonl")).unwrap();
        for name in ["2026-10-03.jsonl", "2026-10-01.jsonl", "notes.txt", "x"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let names: Vec<_> = event_files(&paths)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["2026-10-01.jsonl", "2026-10-03.jsonl"]);

        let other = DatasetPaths::from_slug(tmp.path(), "other");
        std::fs::create_dir_all(other.root()).unwrap();
        std::fs::write(other.events_dir(), "a file, not a dir").unwrap();
        let err = event_files(&other).unwrap_err();
        assert!(format!("{err:#}").contains("reading"), "{err:#}");
    }

    /// Blank and whitespace-only lines are skipped without a count; a line
    /// that does not parse and a last line with no newline each count as
    /// torn, even when that last line is a whole envelope.
    #[test]
    fn read_all_skips_blank_lines_and_counts_torn_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "proj");
        fsx::ensure_private_dir(&paths.events_dir()).unwrap();
        let first = format!("{}\n\n  \t\nnot json\r\n{}\r\n", line("a"), line("b"));
        std::fs::write(paths.events_file(day(1)), first).unwrap();
        std::fs::write(
            paths.events_file(day(2)),
            format!("{}\n{}", line("c"), line("d")),
        )
        .unwrap();
        std::fs::write(paths.events_file(day(3)), "\n\n").unwrap();
        std::fs::write(paths.events_dir().join("skip.txt"), "garbage").unwrap();

        let read = read_all(&paths).unwrap();
        assert_eq!(keys(&read), ["a", "b", "c"]);
        assert_eq!(read.torn_lines, 2, "bad line + unterminated last line");
    }

    /// An append creates the day file, is the file's only content, and ends
    /// a torn last line first so the new event is a line of its own.
    #[test]
    fn append_creates_the_file_and_terminates_a_torn_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "proj");
        let file = paths.events_file(day(1));
        append(&paths, &file, &envelope("a")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("{}\n", line("a"))
        );

        append(&paths, &file, &envelope("b")).unwrap();
        let mut text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, format!("{}\n{}\n", line("a"), line("b")));

        text.push_str("{\"torn");
        std::fs::write(&file, text).unwrap();
        append(&paths, &file, &envelope("c")).unwrap();
        let read = read_all(&paths).unwrap();
        assert_eq!(keys(&read), ["a", "b", "c"]);
        assert_eq!(read.torn_lines, 1);

        let empty = paths.events_file(day(2));
        std::fs::write(&empty, "").unwrap();
        append(&paths, &empty, &envelope("d")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&empty).unwrap(),
            format!("{}\n", line("d"))
        );
    }
}
