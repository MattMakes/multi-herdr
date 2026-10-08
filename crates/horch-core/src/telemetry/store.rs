//! The event store: `events-YYYY-MM-DD.jsonl`, append-only (section 8.2).
//!
//! Crash safety comes from order, not transactions: events are appended and
//! synced first, and only then is the cursor file replaced. A crash between
//! the two re-reads some input on restart, and the dedupe index drops every
//! event whose key `(agent, session_id, event_id)` is already stored.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::readers::Cursors;
use super::{Event, TokenClasses};
use crate::usage::{self, Price};

pub type Key = (String, String, String);

/// The events on disk, with what the snapshot folds of each one in memory
/// (`Table`) and the index of keys already stored.
///
/// The collector keeps every event of the retention window for as long as it
/// runs (NFR-02 RSS), so it keeps no [`Event`]: a few interned ids, the time,
/// the tokens and the cost. The full events stay on disk.
#[derive(Debug, Default)]
pub struct Store {
    dir: PathBuf,
    /// `(agent, session, event id)`, the agent and session interned.
    keys: HashSet<(u32, u32, Box<str>)>,
    sessions: Interner,
    pub(crate) table: Table,
    /// The bytes appended to the event files since open (NFR-02 timing).
    pub bytes_written: u64,
    /// The retention window in days, and the oldest event file date it
    /// keeps (`YYYY-MM-DD`). [`Store::append`] drops older events.
    retention_days: i64,
    oldest: String,
    /// The events [`Store::append`] dropped as older than the retention
    /// window: a transcript read again from the start (section 8.2).
    pub expired_dropped: u64,
}

/// Strings stored once, each with a dense id.
#[derive(Debug, Default)]
pub(crate) struct Interner {
    pub names: Vec<Box<str>>,
    ids: HashMap<Box<str>, u32>,
}

impl Interner {
    pub(crate) fn id(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.into());
        self.ids.insert(name.into(), id);
        id
    }

    pub(crate) fn get(&self, name: &str) -> Option<u32> {
        self.ids.get(name).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }
}

/// A record and its keys in [`GROUPS`] order, as name ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Row {
    pub record: u32,
    pub groups: [u32; 6],
}

/// What the snapshot folds of one stored event.
#[derive(Debug, Clone)]
pub(crate) struct Kept {
    pub ts: Box<str>,
    pub tokens: TokenClasses,
    pub cost: Option<f64>,
    /// Into [`Table::rows`].
    pub row: u32,
    /// The model, a name id: the unpriced events are listed per model.
    pub model: u32,
    pub idle: bool,
}

/// The stored events in store order, interned for the snapshot fold
/// (NFR-02): each event's record and 6 group keys become 1 shared [`Row`].
#[derive(Debug, Default)]
pub(crate) struct Table {
    pub kept: Vec<Kept>,
    pub rows: Vec<Row>,
    row_ids: HashMap<Row, u32>,
    /// The record ids, group keys and models.
    pub names: Interner,
}

impl Table {
    pub(crate) fn from_events<'a>(events: impl IntoIterator<Item = &'a Event>) -> Table {
        let mut t = Table::default();
        for e in events {
            t.push(e);
        }
        t
    }

    pub(crate) fn push(&mut self, e: &Event) {
        let row = Row {
            record: self.names.id(&e.record_id),
            groups: GROUPS.map(|g| self.names.id(group_key(e, g))),
        };
        let next = self.rows.len() as u32;
        let id = *self.row_ids.entry(row).or_insert(next);
        if id == next {
            self.rows.push(row);
        }
        self.kept.push(Kept {
            ts: e.ts.as_str().into(),
            tokens: e.tokens,
            cost: e.cost_usd,
            row: id,
            model: self.names.id(&e.model),
            idle: e.idle,
        });
    }
}

/// Create (or truncate) a file readable by its owner only (section 15), and
/// sync it when `sync`.
fn write_private(path: &Path, bytes: &[u8], sync: bool) -> std::io::Result<()> {
    let mut f = private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes)?;
    if sync {
        f.sync_all()?;
    }
    Ok(())
}

/// Replace `path` atomically: write a private temp file, then rename.
pub(crate) fn replace_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace_private_with(path, bytes, true)
}

fn replace_private_with(path: &Path, bytes: &[u8], sync: bool) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    write_private(&tmp, bytes, sync)?;
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
    dir.join(format!("events-{}.jsonl", file_date(ts)))
}

/// The date part of an event file name: the date of `ts`, or `undated`.
fn file_date(ts: &str) -> &str {
    ts.get(..10)
        .filter(|d| d.as_bytes().get(4) == Some(&b'-'))
        .unwrap_or("undated")
}

/// The oldest event file date the retention window keeps at `now`.
fn oldest_date(now: DateTime<Utc>, retention_days: i64) -> String {
    (now - Duration::days(retention_days))
        .format("%Y-%m-%d")
        .to_string()
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
        let oldest = oldest_date(now, retention_days);
        let mut store = Store {
            dir: dir.to_path_buf(),
            retention_days,
            oldest: oldest.clone(),
            ..Store::default()
        };
        let prices = usage::builtin_prices();
        let mut files = event_files(dir);
        files.retain(|(date, path)| {
            let keep = date.as_str() >= oldest.as_str() || date == "undated";
            if !keep {
                let _ = std::fs::remove_file(path);
            }
            keep
        });
        // NFR-02 peak RSS: size the table and the key index once, from the
        // line count, with room for the events of the next hours. Grown by
        // doubling, they left each old block in the allocator's cache of
        // freed large blocks, still resident.
        let lines: usize = files.iter().map(|(_, path)| count_lines(path)).sum();
        store.reserve(lines + lines / 8);
        for (_, path) in &files {
            for_each_event(path, &prices, |event| {
                if store.insert_key(&event) {
                    store.table.push(&event);
                }
            });
        }
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Move the retention window to `now`: [`Store::append`] drops the
    /// events older than it. The files already stored stay until the next
    /// open.
    pub fn advance(&mut self, now: DateTime<Utc>) {
        self.oldest = oldest_date(now, self.retention_days);
    }

    /// Room for `n` events in the key index and the table.
    fn reserve(&mut self, n: usize) {
        self.keys.reserve(n);
        self.table.kept.reserve_exact(n);
    }

    /// How many events the store holds.
    pub fn len(&self) -> usize {
        self.table.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.kept.is_empty()
    }

    pub fn contains(&self, key: &Key) -> bool {
        let (Some(agent), Some(session)) =
            (self.table.names.get(&key.0), self.sessions.get(&key.1))
        else {
            return false;
        };
        self.keys.contains(&(agent, session, key.2.as_str().into()))
    }

    /// Whether an event at `ts` is older than the retention window: its
    /// file date, as [`Store::open`] keeps files. An undated event is kept.
    fn expired(&self, ts: &str) -> bool {
        let date = file_date(ts);
        date != "undated" && date < self.oldest.as_str()
    }

    /// Record `e`'s key; false when it is already stored.
    fn insert_key(&mut self, e: &Event) -> bool {
        let agent = self.table.names.id(&e.agent);
        let session = self.sessions.id(&e.session_id);
        self.keys
            .insert((agent, session, e.event_id.as_str().into()))
    }

    /// Append the events whose keys are new, and sync. Returns how many were
    /// appended. An event older than the retention window is dropped: its
    /// key may be gone with its file, and the store never takes it back.
    pub fn append(&mut self, events: Vec<Event>) -> Result<usize> {
        let mut by_file: BTreeMap<PathBuf, Vec<Event>> = BTreeMap::new();
        for event in events {
            if self.expired(&event.ts) {
                self.expired_dropped += 1;
                continue;
            }
            if self.insert_key(&event) {
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
            self.bytes_written += text.len() as u64;
            n += batch.len();
            for e in &batch {
                self.table.push(e);
            }
        }
        Ok(n)
    }
}

/// Every event in one file, an event stored without a cost priced from
/// `prices` now ([`Event::price_if_unset`]). A line that does not parse (a
/// torn last line after a crash mid-append) is skipped.
pub(crate) fn read_file(path: &Path, prices: &BTreeMap<String, Price>) -> Vec<Event> {
    let mut out = Vec::new();
    for_each_event(path, prices, |e| out.push(e));
    out
}

/// How many lines `path` holds (its `\n` bytes), 0 when it does not open.
fn count_lines(path: &Path) -> usize {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return 0;
    };
    let mut buf = vec![0u8; 64 << 10];
    let mut n = 0;
    loop {
        match file.read(&mut buf) {
            Ok(0) | Err(_) => return n,
            Ok(got) => n += buf[..got].iter().filter(|b| **b == b'\n').count(),
        }
    }
}

/// [`read_file`], one event at a time: the file is never in memory whole.
fn for_each_event(path: &Path, prices: &BTreeMap<String, Price>, mut f: impl FnMut(Event)) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    for line in std::io::BufReader::new(file).split(b'\n') {
        let Ok(line) = line else {
            return;
        };
        if let Ok(mut e) = serde_json::from_slice::<Event>(&line) {
            e.price_if_unset(prices);
            f(e);
        }
    }
}

/// Every event in `dir`, deduplicated, without opening a [`Store`] (the
/// readers of the files: `horch usage` with a live collector). Priced at
/// read time from the built-in table, as [`Store::open`] does.
pub fn read_all(dir: &Path) -> Vec<Event> {
    let prices = usage::builtin_prices();
    let mut keys = HashSet::new();
    let mut out = Vec::new();
    for (_, path) in event_files(dir) {
        for e in read_file(&path, &prices) {
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

/// The saved cursors. A cursor saved before it named its agent and session
/// gets them from its key ([`Cursor::fill_from_key`]).
///
/// [`Cursor::fill_from_key`]: super::cursor::Cursor::fill_from_key
pub fn load_cursors(dir: &Path) -> Cursors {
    let mut cursors: Cursors = std::fs::read_to_string(cursors_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for (key, c) in cursors.iter_mut() {
        if c.agent.is_empty() || c.session_id.is_empty() || c.path.is_empty() {
            c.fill_from_key(key);
        }
    }
    cursors
}

/// Replace the cursor file. `sync: false` leaves the new file in the page
/// cache. That is safe after the events are synced: a power loss can only
/// keep the old cursors, or a torn file that loads as none, and both re-read
/// input the store already holds, whose repeats it drops (section 8.2).
/// Returns the bytes written.
pub(crate) fn save_cursors(dir: &Path, cursors: &Cursors, sync: bool) -> Result<usize> {
    let json = serde_json::to_vec(cursors)?;
    replace_private_with(&cursors_path(dir), &json, sync)
        .with_context(|| format!("saving {}", cursors_path(dir).display()))?;
    Ok(json.len())
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
    /// Events with no price (an unknown model): their tokens are in
    /// `tokens`, their cost is not in `cost_usd`.
    #[serde(default)]
    pub unpriced_events: u64,
}

/// The events of one model the price table does not know (TEL-10: listed,
/// never counted as $0).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnpricedRow {
    pub model: String,
    pub events: u64,
    pub tokens: TokenClasses,
}

/// The unpriced events of `events`, one row per model, sorted by model.
pub fn unpriced<'a>(events: impl Iterator<Item = &'a Event>) -> Vec<UnpricedRow> {
    let mut by_model: BTreeMap<&str, UnpricedRow> = BTreeMap::new();
    for e in events.filter(|e| e.cost_usd.is_none()) {
        add_unpriced(&mut by_model, e);
    }
    by_model.into_values().collect()
}

pub(crate) fn add_unpriced<'a>(by_model: &mut BTreeMap<&'a str, UnpricedRow>, e: &'a Event) {
    let row = by_model
        .entry(e.model.as_str())
        .or_insert_with(|| UnpricedRow {
            model: e.model.clone(),
            ..UnpricedRow::default()
        });
    row.events += 1;
    row.tokens.add(&e.tokens);
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
    /// The window's unpriced events, per model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unpriced: Vec<UnpricedRow>,
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
    /// Events with no price.
    pub unpriced_events: u64,
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
            unpriced_events: g.unpriced_events,
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
        unpriced: u64,
    }
    let mut acc: BTreeMap<&str, Acc> = BTreeMap::new();
    for e in events {
        let a = acc.entry(group_key(e, group)).or_default();
        a.tokens.add(&e.tokens);
        // An unpriced event adds its tokens and is counted; it adds no $0.
        a.unpriced += u64::from(e.cost_usd.is_none());
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
            unpriced_events: a.unpriced,
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
        unpriced: unpriced(pick()),
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
        assert_eq!(s.append(vec![a.clone()]).unwrap(), 0, "already stored");
        assert_eq!(s.len(), 1);
        assert!(s.contains(&a.key()));
        assert!(tmp.path().join("events-2026-09-28.jsonl").is_file());
    }

    /// NFR-02 peak RSS: the open sizes the table and the key index once,
    /// from the line count, with room for an eighth more: the next events
    /// go in without a new block. The table and the keys are what the files
    /// hold, a repeat and a torn line dropped.
    #[test]
    fn nfr_02_open_sizes_the_table_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        let events: Vec<Event> = (0..80)
            .map(|i| {
                let day = if i % 3 == 0 { 27 } else { 28 };
                let ts = format!("2026-09-{day}T17:{:02}:00Z", i % 60);
                ev(&format!("m{i}"), &ts, &format!("r{}", i % 7), i, 0.1)
            })
            .collect();
        s.append(events.clone()).unwrap();
        // A repeat in another file, and a torn last line.
        let line = serde_json::to_string(&events[1]).unwrap();
        let path = tmp.path().join("events-2026-09-27.jsonl");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(f, "{line}\n{}", &line[..20]).unwrap();
        drop((s, f));

        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        let all = read_all(tmp.path());
        assert_eq!(all.len(), 80);
        // Each kept event with its names resolved (the store interns the
        // agent first, so the ids differ).
        let resolved = |t: &Table| -> Vec<String> {
            let name = |id: u32| t.names.names[id as usize].to_string();
            (t.kept.iter())
                .map(|k| {
                    let row = t.rows[k.row as usize];
                    let groups = row.groups.map(name);
                    let (record, model) = (name(row.record), name(k.model));
                    format!(
                        "{} {:?} {:?} {record} {groups:?} {model} {}",
                        k.ts, k.tokens, k.cost, k.idle
                    )
                })
                .collect()
        };
        assert_eq!(resolved(&s.table), resolved(&Table::from_events(&all)));
        assert!(all.iter().all(|e| s.contains(&e.key())));
        assert_eq!(s.keys.len(), 80);

        // 80 events and the repeat; the torn line has no `\n`.
        let lines = 81;
        let (cap, at) = (s.table.kept.capacity(), s.table.kept.as_ptr());
        assert_eq!(cap, lines + lines / 8);
        let more: Vec<Event> = (80..80 + lines / 8)
            .map(|i| {
                ev(
                    &format!("m{i}"),
                    "2026-09-28T17:59:00Z",
                    "r1",
                    i as u64,
                    0.1,
                )
            })
            .collect();
        assert_eq!(s.append(more).unwrap(), lines / 8);
        assert_eq!(s.table.kept.as_ptr(), at, "the table moved to a new block");
    }

    #[test]
    fn retention_deletes_old_files_at_open() {
        let tmp = tempfile::tempdir().unwrap();
        let earlier = now() - Duration::days(60);
        let mut s = Store::open(tmp.path(), earlier, 35).unwrap();
        s.append(vec![
            ev("old", "2026-08-01T00:00:00Z", "r1", 1, 0.0),
            ev("new", "2026-09-27T00:00:00Z", "r1", 1, 0.0),
        ])
        .unwrap();
        let s = Store::open(tmp.path(), now(), 35).unwrap();
        assert_eq!(s.len(), 1);
        assert!(!tmp.path().join("events-2026-08-01.jsonl").exists());
    }

    /// R2 review: the store never takes an event older than its retention
    /// window back (a transcript read again from the start, after its old
    /// keys left with their file). The window moves with [`Store::advance`];
    /// an undated event is kept, as its file is.
    #[test]
    fn append_drops_events_older_than_the_retention_window() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        let n = s
            .append(vec![
                ev("old", "2026-08-23T23:59:59Z", "r1", 1, 0.0),
                ev("edge", "2026-08-24T00:00:00Z", "r1", 1, 0.0),
                ev("undated", "soon", "r1", 1, 0.0),
            ])
            .unwrap();
        assert_eq!((n, s.expired_dropped), (2, 1));
        assert!(!tmp.path().join("events-2026-08-23.jsonl").exists());
        s.advance(now() + Duration::days(1));
        let n = s
            .append(vec![ev("edge2", "2026-08-24T12:00:00Z", "r1", 1, 0.0)])
            .unwrap();
        assert_eq!((n, s.expired_dropped), (0, 2));
    }

    /// R2 review: a `cursors.json` saved before the cursors named their
    /// agent and session loads, each cursor's owner taken from its key, a
    /// session id with a `|` included; a save then writes the fields.
    #[test]
    fn an_old_cursors_file_loads_with_its_owners() {
        let tmp = tempfile::tempdir().unwrap();
        let prime = "/s/has|pipe/prime/sessions/p.jsonl";
        let old = serde_json::json!({
            "claude|c1|/h/.claude/projects/-w/c1.jsonl":
                {"path": "/h/.claude/projects/-w/c1.jsonl", "offset": 5},
            format!("prime|{prime}|{prime}"): {"path": prime, "offset": 7},
            "opencode|ses_1|/h/opencode.db": {"since_ms": 9},
            "no key parts": {"offset": 1},
        });
        std::fs::write(cursors_path(tmp.path()), old.to_string()).unwrap();
        let cursors = load_cursors(tmp.path());
        let owner = |key: &str| {
            let c = &cursors[key];
            (c.agent.as_str(), c.session_id.as_str(), c.path.as_str())
        };
        assert_eq!(
            owner("claude|c1|/h/.claude/projects/-w/c1.jsonl"),
            ("claude", "c1", "/h/.claude/projects/-w/c1.jsonl")
        );
        assert_eq!(
            owner(&format!("prime|{prime}|{prime}")),
            ("prime", prime, prime)
        );
        assert_eq!(
            owner("opencode|ses_1|/h/opencode.db"),
            ("opencode", "ses_1", "/h/opencode.db")
        );
        assert_eq!(owner("no key parts"), ("", "", ""));
        assert_eq!(cursors[&format!("prime|{prime}|{prime}")].offset, 7);

        save_cursors(tmp.path(), &cursors, false).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(cursors_path(tmp.path())).unwrap())
                .unwrap();
        assert_eq!(saved[format!("prime|{prime}|{prime}")]["session_id"], prime);
        assert_eq!(load_cursors(tmp.path()), cursors);
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
        assert_eq!(Store::open(tmp.path(), now(), 35).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn tel_11_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Store::open(tmp.path(), now(), 35).unwrap();
        s.append(vec![ev("m1", "2026-09-28T17:00:00Z", "r1", 5, 0.1)])
            .unwrap();
        save_cursors(tmp.path(), &Cursors::new(), true).unwrap();
        for f in ["events-2026-09-28.jsonl", "cursors.json"] {
            let mode = std::fs::metadata(tmp.path().join(f))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{f}");
        }
    }

    /// Read-time pricing (W9): an event stored with no cost, on a model the
    /// table prices now, is priced on read with the one pricing function. A
    /// stored cost is the price at the time and is kept. A model with no
    /// price stays unpriced.
    #[test]
    fn tel_10_a_stored_event_without_a_cost_is_priced_at_read_time() {
        let tmp = tempfile::tempdir().unwrap();
        let mut late = ev("late", "2026-09-28T17:00:00Z", "r1", 1000, 0.0);
        late.model = "claude-sonnet-5-5".into();
        late.cost_usd = None;
        let mut kept = late.clone();
        kept.event_id = "kept".into();
        kept.cost_usd = Some(0.5);
        let mut unknown = late.clone();
        unknown.event_id = "unknown".into();
        unknown.model = "codex-auto-review".into();
        let lines: Vec<String> = [&late, &kept, &unknown]
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        std::fs::write(
            tmp.path().join("events-2026-09-28.jsonl"),
            lines.join("\n") + "\n",
        )
        .unwrap();
        let prices = usage::builtin_prices();
        let want = usage::cost_of(&prices, "claude-sonnet-5-5", &late.tokens.priced());
        assert!(want.is_some_and(|c| c > 0.0));
        let events = read_all(tmp.path());
        let cost = |id: &str| events.iter().find(|e| e.event_id == id).unwrap().cost_usd;
        assert_eq!(cost("late"), want);
        assert_eq!(cost("kept"), Some(0.5), "the stored price is kept");
        assert_eq!(cost("unknown"), None, "unpriced, not $0");
        // The collector's store, in file order: late, kept, unknown.
        let opened = Store::open(tmp.path(), now(), 35).unwrap();
        let costs: Vec<Option<f64>> = opened.table.kept.iter().map(|k| k.cost).collect();
        assert_eq!(costs, vec![want, Some(0.5), None]);
    }

    /// Never zero the unpriced (W9): an unpriced event adds its tokens and a
    /// count, and no cost; the window lists it per model.
    #[test]
    fn tel_10_rollups_list_unpriced_events_and_never_count_them_as_zero() {
        let mut free = ev("b", "2026-09-28T17:01:00Z", "r1", 10, 0.0);
        free.cost_usd = None;
        free.model = "mystery-9".into();
        let events = vec![ev("a", "2026-09-28T17:00:00Z", "r1", 10, 1.0), free];
        let r = rollup(&events, None, &BTreeSet::new());
        let row = &r.by_teammate[0];
        assert_eq!(row.unpriced_events, 1);
        assert!((row.cost_usd - 1.0).abs() < 1e-12);
        assert_eq!(row.tokens.output, 20, "the unpriced tokens still count");
        assert_eq!(
            r.unpriced,
            vec![UnpricedRow {
                model: "mystery-9".into(),
                events: 1,
                tokens: events[1].tokens,
            }]
        );
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
