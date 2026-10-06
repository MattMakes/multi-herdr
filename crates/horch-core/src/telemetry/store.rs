//! The event store: `events-YYYY-MM-DD.jsonl`, append-only (section 8.2).
//!
//! Crash safety comes from order, not transactions: events are appended and
//! synced first, and only then is the cursor file replaced. A crash between
//! the two re-reads some input on restart, and the dedupe index drops every
//! event whose key `(agent, session_id, event_id)` is already stored.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::readers::Cursors;
use super::{Event, TokenClasses};

pub type Key = (String, String, String);

/// The events on disk, with an in-memory copy for rollups and the index of
/// keys already stored.
#[derive(Debug, Default)]
pub struct Store {
    dir: PathBuf,
    keys: HashSet<Key>,
    pub events: Vec<Event>,
}

/// Create (or truncate) a file readable by its owner only (section 15).
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Replace `path` atomically: write a private temp file, then rename.
pub(crate) fn replace_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    write_private(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn private_options() -> std::fs::OpenOptions {
    #[allow(unused_mut)]
    let mut o = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// The event file an event belongs in: the UTC date of its timestamp.
fn file_for(dir: &Path, ts: &str) -> PathBuf {
    let date = ts
        .get(..10)
        .filter(|d| d.as_bytes().get(4) == Some(&b'-'))
        .unwrap_or("undated");
    dir.join(format!("events-{date}.jsonl"))
}

/// Event files in `dir`, oldest first, with their date.
fn event_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let date = name
                .strip_prefix("events-")?
                .strip_suffix(".jsonl")?
                .to_string();
            Some((date, e.path()))
        })
        .collect();
    out.sort();
    out
}

impl Store {
    /// Open the store in `dir`, deleting files older than the retention
    /// window and loading the rest (keys and events).
    pub fn open(dir: &Path, now: DateTime<Utc>, retention_days: i64) -> Result<Store> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let oldest = (now - Duration::days(retention_days))
            .format("%Y-%m-%d")
            .to_string();
        let mut store = Store {
            dir: dir.to_path_buf(),
            ..Store::default()
        };
        for (date, path) in event_files(dir) {
            if date.as_str() < oldest.as_str() && date != "undated" {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            for event in read_file(&path) {
                if store.keys.insert(event.key()) {
                    store.events.push(event);
                }
            }
        }
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.keys.contains(key)
    }

    /// Append the events whose keys are new, and sync. Returns how many were
    /// appended.
    pub fn append(&mut self, events: Vec<Event>) -> Result<usize> {
        let mut by_file: BTreeMap<PathBuf, Vec<Event>> = BTreeMap::new();
        for event in events {
            if self.keys.insert(event.key()) {
                by_file
                    .entry(file_for(&self.dir, &event.ts))
                    .or_default()
                    .push(event);
            }
        }
        let mut n = 0;
        for (path, batch) in by_file {
            let mut text = String::new();
            for e in &batch {
                text.push_str(&serde_json::to_string(e)?);
                text.push('\n');
            }
            let mut f = private_options()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("opening {}", path.display()))?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            n += batch.len();
            self.events.extend(batch);
        }
        Ok(n)
    }

    /// Every stored event with `ts >= since` (all when `None`).
    pub fn since<'a>(&'a self, since: Option<&'a str>) -> impl Iterator<Item = &'a Event> + 'a {
        self.events
            .iter()
            .filter(move |e| since.is_none_or(|s| e.ts.as_str() >= s))
    }
}

/// Every event in one file. A torn last line (a crash mid-append) is skipped.
pub(crate) fn read_file(path: &Path) -> Vec<Event> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Every event in `dir`, deduplicated, without opening a [`Store`] (the
/// readers of the files: `horch usage` with a live collector).
pub fn read_all(dir: &Path) -> Vec<Event> {
    let mut keys = HashSet::new();
    let mut out = Vec::new();
    for (_, path) in event_files(dir) {
        for e in read_file(&path) {
            if keys.insert(e.key()) {
                out.push(e);
            }
        }
    }
    out
}

// ─── cursors ────────────────────────────────────────────────────────────────

pub(crate) fn cursors_path(dir: &Path) -> PathBuf {
    dir.join("cursors.json")
}

pub fn load_cursors(dir: &Path) -> Cursors {
    std::fs::read_to_string(cursors_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_cursors(dir: &Path, cursors: &Cursors) -> Result<()> {
    let json = serde_json::to_vec(cursors)?;
    replace_private(&cursors_path(dir), &json)
        .with_context(|| format!("saving {}", cursors_path(dir).display()))
}

// ─── rollups ────────────────────────────────────────────────────────────────

/// One row of a rollup (section 9).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RollupRow {
    pub key: String,
    pub tokens: TokenClasses,
    pub cost_usd: f64,
    /// Distinct records with events in the window.
    pub records: u64,
    /// Of those, how many are `done` now.
    pub done: u64,
    pub cache_hit: Option<f64>,
    /// Cost of the done records divided by their count.
    pub cost_per_done: Option<f64>,
}

/// The six groupings of one window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rollup {
    pub by_teammate: Vec<RollupRow>,
    pub by_phase: Vec<RollupRow>,
    pub by_agent: Vec<RollupRow>,
    pub by_project: Vec<RollupRow>,
    pub by_plan: Vec<RollupRow>,
    pub by_kind: Vec<RollupRow>,
}

impl Rollup {
    pub fn group(&self, name: &str) -> Option<&[RollupRow]> {
        Some(match name {
            "teammate" => &self.by_teammate,
            "phase" => &self.by_phase,
            "agent" => &self.by_agent,
            "project" => &self.by_project,
            "plan" => &self.by_plan,
            "kind" => &self.by_kind,
            _ => return None,
        })
    }
}

/// The groupings `horch usage --by` and the screen's `g` key cycle through.
pub const GROUPS: [&str; 6] = ["teammate", "phase", "agent", "project", "plan", "kind"];

/// The group key of an event.
pub(crate) fn group_key<'a>(e: &'a Event, group: &str) -> &'a str {
    match group {
        "teammate" => &e.teammate,
        "phase" => e.phase.as_deref().unwrap_or("-"),
        "agent" => &e.agent,
        "project" => e.project.as_deref().unwrap_or("-"),
        "plan" => e.plan.as_deref().unwrap_or("-"),
        "kind" => &e.kind,
        _ => "-",
    }
}

/// One group's sums, before it becomes a row.
#[derive(Debug, Default)]
pub(crate) struct GroupSums {
    pub tokens: TokenClasses,
    pub cost: f64,
    /// Distinct records with events in the window.
    pub records: u64,
    /// Of those, how many are done now.
    pub done: u64,
    /// The cost of the done records' events.
    pub done_cost: f64,
}

/// The rows of one grouping, sorted by cost, highest first.
pub(crate) fn rows_from(groups: impl IntoIterator<Item = (String, GroupSums)>) -> Vec<RollupRow> {
    let mut rows: Vec<RollupRow> = groups
        .into_iter()
        .map(|(key, g)| RollupRow {
            key,
            cache_hit: g.tokens.cache_hit(),
            tokens: g.tokens,
            cost_usd: g.cost,
            records: g.records,
            done: g.done,
            cost_per_done: (g.done > 0).then(|| g.done_cost / g.done as f64),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
    rows
}

/// Group `events` by `group`, rows sorted by cost, highest first.
/// `done` names the records whose status is `done`.
pub fn rollup_rows<'a>(
    events: impl Iterator<Item = &'a Event>,
    group: &str,
    done: &BTreeSet<String>,
) -> Vec<RollupRow> {
    #[derive(Default)]
    struct Acc<'e> {
        tokens: TokenClasses,
        cost: f64,
        records: BTreeSet<&'e str>,
        done_cost: f64,
    }
    let mut acc: BTreeMap<&str, Acc> = BTreeMap::new();
    for e in events {
        let a = acc.entry(group_key(e, group)).or_default();
        a.tokens.add(&e.tokens);
        let cost = e.cost_usd.unwrap_or(0.0);
        a.cost += cost;
        a.records.insert(&e.record_id);
        if done.contains(&e.record_id) {
            a.done_cost += cost;
        }
    }
    rows_from(acc.into_iter().map(|(key, a)| {
        let sums = GroupSums {
            records: a.records.len() as u64,
            done: a.records.iter().filter(|r| done.contains(**r)).count() as u64,
            tokens: a.tokens,
            cost: a.cost,
            done_cost: a.done_cost,
        };
        (key.to_string(), sums)
    }))
}

/// All six groupings of the events at or after `since`.
pub fn rollup(events: &[Event], since: Option<&str>, done: &BTreeSet<String>) -> Rollup {
    let pick = || {
        events
            .iter()
            .filter(move |e| since.is_none_or(|s| e.ts.as_str() >= s))
    };
    Rollup {
        by_teammate: rollup_rows(pick(), "teammate", done),
        by_phase: rollup_rows(pick(), "phase", done),
        by_agent: rollup_rows(pick(), "agent", done),
        by_project: rollup_rows(pick(), "project", done),
        by_plan: rollup_rows(pick(), "plan", done),
        by_kind: rollup_rows(pick(), "kind", done),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, ts: &str, record: &str, out: u64, cost: f64) -> Event {
        Event {
            ts: ts.into(),
            project: Some("/p".into()),
            record_id: record.into(),
            session_id: format!("s-{record}"),
            event_id: id.into(),
            role: "r".into(),
            teammate: "sonnet".into(),
            via: None,
            kind: "worker".into(),
            agent: "claude".into(),
            model: "claude-sonnet-5".into(),
            effort: None,
            phase: Some("implementation".into()),
            plan: None,
            subagent: false,
            delta: false,
            tool_nested: false,
            idle: false,
            tokens: TokenClasses {
                output: out,
                cache_read: 90,
                input: 10,
                ..TokenClasses::default()
            },
            cost_usd: Some(cost),
            harness_cost: None,
        }
    }

    fn now() -> DateTime<Utc> {
        crate::clock::parse("2026-09-28T18:00:00Z").unwrap()
    }

    #[test]
    fn tel_07_duplicate_keys_are_dropped_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        let a = ev("m1", "2026-09-28T17:00:00Z", "r1", 5, 0.1);
        assert_eq!(s.append(vec![a.clone(), a.clone()]).unwrap(), 1);
        drop(s);
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        assert_eq!(s.append(vec![a]).unwrap(), 0, "already stored");
        assert_eq!(s.events.len(), 1);
        assert!(tmp.path().join("events-2026-09-28.jsonl").is_file());
    }

    #[test]
    fn retention_deletes_old_files_at_open() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        s.append(vec![
            ev("old", "2026-08-01T00:00:00Z", "r1", 1, 0.0),
            ev("new", "2026-09-27T00:00:00Z", "r1", 1, 0.0),
        ])
        .unwrap();
        let s = Store::open(tmp.path(), now(), 35).unwrap();
        assert_eq!(s.events.len(), 1);
        assert!(!tmp.path().join("events-2026-08-01.jsonl").exists());
    }

    #[test]
    fn a_torn_last_line_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let line = serde_json::to_string(&ev("m1", "2026-09-28T17:00:00Z", "r1", 5, 0.1)).unwrap();
        std::fs::write(
            tmp.path().join("events-2026-09-28.jsonl"),
            format!("{line}\n{}", &line[..20]),
        )
        .unwrap();
        assert_eq!(Store::open(tmp.path(), now(), 35).unwrap().events.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn tel_11_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        s.append(vec![ev("m1", "2026-09-28T17:00:00Z", "r1", 5, 0.1)])
            .unwrap();
        save_cursors(tmp.path(), &Cursors::new()).unwrap();
        for f in ["events-2026-09-28.jsonl", "cursors.json"] {
            let mode = std::fs::metadata(tmp.path().join(f))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{f}");
        }
    }

    #[test]
    fn rollups_count_records_done_and_cost_per_done() {
        let events = vec![
            ev("a", "2026-09-28T17:00:00Z", "r1", 10, 1.0),
            ev("b", "2026-09-28T17:01:00Z", "r1", 10, 2.0),
            ev("c", "2026-09-28T17:02:00Z", "r2", 10, 4.0),
            ev("d", "2026-09-20T17:02:00Z", "r3", 10, 8.0),
        ];
        let done: BTreeSet<String> = ["r1".to_string()].into();
        let r = rollup(&events, Some("2026-09-21T18:00:00Z"), &done);
        let row = &r.by_teammate[0];
        assert_eq!((row.records, row.done), (2, 1));
        assert!((row.cost_usd - 7.0).abs() < 1e-12);
        assert_eq!(row.cost_per_done, Some(3.0));
        assert_eq!(row.tokens.output, 30);
        assert!((row.cache_hit.unwrap() - 0.9).abs() < 1e-12);
        assert_eq!(r.group("plan").unwrap()[0].key, "-");
    }
}
