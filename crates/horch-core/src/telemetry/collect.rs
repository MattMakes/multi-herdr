//! One collector tick (section 8.3), and the snapshot it writes (section 9).
//!
//! 1. Read every ledger under the state root (a ledger that does not parse
//!    keeps its last good copy: a write may be in flight).
//! 2. Keep the records updated inside the retention window.
//! 3. Poll each record's reader.
//! 4. Append the events, sync, then save the cursors.
//! 5. Fold limit signals into the quota readings; probe when due.
//! 6. Write `snapshot.json` (temp file + rename).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::readers::{self, Cursors, Located, Polled, Unreadable};
use super::store::{self, GroupSums, Kept, Rollup, Store, Table, UnpricedRow};
use super::{Event, Observation, QuotaSignal, TokenClasses, Unread};
use crate::clock;
use crate::execution::legacy::{Record, KIND_ORCHESTRATOR};
use crate::execution::model::ExecutionStatus;
use crate::execution::store as execution_store;
use crate::routing::policy::Policy;
use crate::routing::quota::{self, QuotaFile, QuotaView};
use crate::routing::quota_probe::{self, ProbeBins};
use crate::routing::snapshot::QuotaEnv;
use crate::usage::{self, Locations, Price};

/// Every record in every ledger under `state_root`, skipping a ledger that
/// does not parse right now. For readers outside the collector.
pub fn read_ledgers(state_root: &Path) -> Vec<Record> {
    execution_store::read_all_ledgers(state_root)
        .into_iter()
        .flat_map(|(_, records)| records)
        .collect()
}

/// The ids of the records whose typed status is terminal: their spend counts
/// as finished work in the rollups.
pub fn done_record_ids(records: &[Record]) -> BTreeSet<String> {
    done_ids(records)
}

fn done_ids<'r>(records: impl IntoIterator<Item = &'r Record>) -> BTreeSet<String> {
    records
        .into_iter()
        .filter(|r| r.execution_status().is_terminal())
        .map(|r| r.record_id.clone())
        .collect()
}

/// The idle marker a worker spawned without a task carries.
pub const IDLE_TASK: &str = "(idle - awaiting assignment)";

/// When the probes run in a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probing {
    /// Never (a viewer, or tests without fakes).
    Never,
    /// When the last probe is older than `probe_interval_min` (the collector).
    Scheduled,
    /// When the last probe is older than `probe_on_demand_age_min` (a CLI
    /// with no live collector, QUO-07).
    OnDemand,
}

/// The collector's own identity in the snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectorInfo {
    pub pid: u32,
    pub version: String,
    pub started_at: String,
}

/// One live pane (section 9).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveRow {
    pub project: Option<String>,
    pub record_id: String,
    pub role: String,
    pub teammate: String,
    pub via: Option<String>,
    pub kind: String,
    pub agent: String,
    pub model: String,
    pub effort: Option<String>,
    pub phase: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub task_head: String,
    pub tokens: TokenClasses,
    /// The priced events' cost; `unpriced_events` are not in it.
    pub cost_usd: f64,
    /// Events with no price (an unknown model), never counted as $0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unpriced_events: u64,
    pub rate_tokens_per_min: f64,
    pub rate_usd_per_hour: f64,
    pub last_event_at: Option<String>,
    pub idle: bool,
    /// The state of the pool this pane draws from.
    #[serde(default)]
    pub pool: String,
    #[serde(default)]
    pub pool_state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Insights {
    pub orchestrator_share: Option<f64>,
    pub cache_hit: Option<f64>,
    pub idle_spend_share: Option<f64>,
    pub top_plan: Option<String>,
}

/// `telemetry/snapshot.json`, schema 1.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub generated_at: String,
    pub collector: CollectorInfo,
    pub pools: BTreeMap<String, quota::PoolReading>,
    pub live: Vec<LiveRow>,
    /// `5h`, `today`, `7d`.
    pub rollups: BTreeMap<String, Rollup>,
    pub insights: Insights,
    pub unread: Vec<Unread>,
    /// How many distinct projects the live rows span.
    #[serde(default)]
    pub projects: usize,
}

pub fn snapshot_path(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("snapshot.json")
}

impl Snapshot {
    pub fn read(path: &Path) -> Result<Snapshot> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// The start of each rollup window at `now`.
pub fn window_start(window: &str, now: DateTime<Utc>) -> Option<String> {
    match window {
        "5h" => Some(clock::stamp(now - Duration::hours(5))),
        "today" => Some(format!("{}T00:00:00Z", now.format("%Y-%m-%d"))),
        "7d" => Some(clock::stamp(now - Duration::days(7))),
        _ => None,
    }
}

pub const WINDOWS: [&str; 3] = ["5h", "today", "7d"];

/// Collector state that lives across ticks.
pub struct Collector {
    pub state_root: PathBuf,
    pub loc: Locations,
    pub prices: BTreeMap<String, Price>,
    pub policy: Policy,
    pub store: Store,
    pub cursors: Cursors,
    pub probing: Probing,
    pub bins: ProbeBins,
    /// The quota file override, probe timeout and temp root.
    pub quota_env: QuotaEnv,
    pub info: CollectorInfo,
    /// Each ledger's last good records, read again only when it changes.
    last_good: BTreeMap<PathBuf, Ledger>,
    /// Where each session's inputs are, kept across ticks.
    places: HashMap<(String, String), Place>,
    /// Each session's inputs as they were when last polled.
    polled: HashMap<(String, String), Vec<Option<Stamp>>>,
    /// How many times a tick searched the transcript trees.
    pub searches: u64,
    /// Test hook: fail after the append, before the cursor save.
    pub fail_after_append: bool,
    /// Test hook: die there instead, leaving a stale lock (a `kill -9`).
    pub abort_after_append: bool,
}

impl Collector {
    pub fn open(state_root: &Path, loc: Locations, probing: Probing) -> Result<Collector> {
        Self::open_at(state_root, loc, probing, clock::now())
    }

    /// As [`Collector::open`], at an explicit time (retention is relative).
    ///
    /// These two read no environment: no `HORCH_BALANCE`, no quota file, the
    /// default probe timeout, and the plain program names. The binary opens
    /// its collector with [`Collector::open_in`].
    pub fn open_at(
        state_root: &Path,
        loc: Locations,
        probing: Probing,
        now: DateTime<Utc>,
    ) -> Result<Collector> {
        let quota_env = QuotaEnv {
            quota_file: None,
            probe_timeout: None,
            temp_root: state_root.to_path_buf(),
            bins: ProbeBins::new(
                &crate::runtime::HarnessBins::resolve(
                    &crate::runtime::BinOverrides::default(),
                    None,
                    None,
                ),
                loc.codex_sessions.clone(),
            ),
        };
        Self::open_with(state_root, loc, probing, now, None, quota_env)
    }

    /// The collector the binary runs: `HORCH_BALANCE`, the quota settings,
    /// the programs and the fault points all come from `ctx`.
    pub fn open_in(
        ctx: &crate::runtime::RuntimeContext,
        loc: Locations,
        probing: Probing,
        now: DateTime<Utc>,
    ) -> Result<Collector> {
        let c = Self::open_with(
            &ctx.paths.state_root,
            loc,
            probing,
            now,
            ctx.settings.balance_override.as_deref(),
            QuotaEnv::from_context(ctx),
        )?;
        Ok(c.with_faults(&ctx.settings.faults))
    }

    fn open_with(
        state_root: &Path,
        loc: Locations,
        probing: Probing,
        now: DateTime<Utc>,
        balance: Option<&str>,
        quota_env: QuotaEnv,
    ) -> Result<Collector> {
        let policy = Policy::load(state_root, balance)?;
        let dir = super::dir(state_root);
        let store = Store::open(&dir, now, policy.retention_days)?;
        let cursors = store::load_cursors(&dir);
        Ok(Collector {
            state_root: state_root.to_path_buf(),
            bins: ProbeBins {
                codex_sessions: loc.codex_sessions.clone(),
                ..quota_env.bins.clone()
            },
            quota_env,
            loc,
            prices: usage::builtin_prices(),
            policy,
            store,
            cursors,
            probing,
            info: CollectorInfo {
                pid: std::process::id(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                started_at: clock::stamp(now),
            },
            last_good: BTreeMap::new(),
            places: HashMap::new(),
            polled: HashMap::new(),
            searches: 0,
            fail_after_append: false,
            abort_after_append: false,
        })
    }

    /// Arm the test hooks that `HORCH_FAULT` names (`RuntimeContext::settings`).
    pub(crate) fn with_faults(mut self, faults: &crate::runtime::Faults) -> Collector {
        self.fail_after_append = faults.has("after-append");
        self.abort_after_append = faults.has("abort-after-append");
        self
    }

    /// Every ledger's records, keeping the last good copy of one that does
    /// not parse right now.
    pub fn ledgers(&mut self) -> Vec<Record> {
        self.refresh_ledgers();
        (self.last_good.values())
            .flat_map(|l| l.records.iter().cloned())
            .collect()
    }

    /// Read the ledgers whose length or modification time changed since
    /// the last tick (NFR-02: parsing every ledger took most of a tick). A
    /// ledger that does not parse keeps its last good copy and is read again
    /// next tick; a deleted one is dropped.
    fn refresh_ledgers(&mut self) {
        let paths = execution_store::ledger_paths(&self.state_root);
        self.last_good.retain(|p, _| paths.contains(p));
        for path in paths {
            let stamp = std::fs::metadata(&path)
                .ok()
                .map(|m| (m.len(), m.modified().ok()));
            if stamp.is_some() && self.last_good.get(&path).map(|l| l.stamp) == Some(stamp) {
                continue;
            }
            if let Some(records) = execution_store::read_ledger_file(&path) {
                self.last_good.insert(path, Ledger { stamp, records });
            }
        }
    }

    /// Run one tick at `now` and write the snapshot.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Result<Snapshot> {
        self.refresh_ledgers();
        let ledgers = std::mem::take(&mut self.last_good);
        let records: Vec<&Record> = ledgers.values().flat_map(|l| &l.records).collect();
        let snapshot = self.tick_on(&records, now);
        self.last_good = ledgers;
        snapshot
    }

    fn tick_on(&mut self, records: &[&Record], now: DateTime<Utc>) -> Result<Snapshot> {
        let oldest = clock::stamp(now - Duration::days(self.policy.retention_days));
        let mut new_events = Vec::new();
        let mut signals: Vec<QuotaSignal> = Vec::new();
        let mut unread = Vec::new();
        let mut moved = false;
        for r in records
            .iter()
            .filter(|r| r.updated_at.as_str() >= oldest.as_str())
        {
            let polled = match r.session_id.as_deref().filter(|s| !s.is_empty()) {
                None => Err(Unreadable::NoSessionId),
                Some(sid) => self.poll(r, sid, now),
            };
            match polled {
                Ok(None) => {}
                Ok(Some(polled)) => {
                    moved = true;
                    for o in polled.observations {
                        match o {
                            Observation::Usage(u) => new_events.push(event_for(r, u, &self.prices)),
                            Observation::Quota(s) => signals.push(s),
                        }
                    }
                }
                Err(why) => unread.push(Unread {
                    record_id: r.record_id.clone(),
                    role: r.role.clone(),
                    agent: r.agent.clone(),
                    reason: why.to_string(),
                }),
            }
        }
        // Append and sync first, cursors second (section 8.2).
        self.store.append(new_events)?;
        if self.abort_after_append {
            eprintln!("HORCH_FAULT=abort-after-append: dying before the cursor save");
            std::process::abort();
        }
        if self.fail_after_append {
            anyhow::bail!("HORCH_FAULT=after-append: stopping before the cursor save");
        }
        // A cursor moves only when its input is polled.
        if moved {
            store::save_cursors(self.store.dir(), &self.cursors)?;
        }

        let quota = self.update_quota(now, &signals)?;
        let snapshot = build_indexed(
            records,
            &self.store.table,
            quota,
            unread,
            now,
            self.info.clone(),
        );
        let json = serde_json::to_vec_pretty(&snapshot)?;
        store::replace_private(&snapshot_path(&self.state_root), &json)?;
        Ok(snapshot)
    }

    /// Poll the inputs of `r` when they changed since its last poll
    /// (NFR-02: opening every transcript of every record took most of a
    /// tick). `None`: nothing changed, so there is nothing new to read. The
    /// stamps are taken before the poll, so a write during it shows next tick.
    fn poll(
        &mut self,
        r: &Record,
        sid: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Polled>, Unreadable> {
        let located = self.locate(r, sid, now)?;
        let stamps: Vec<Option<Stamp>> = match &located {
            Located::Files(files) => files.iter().map(|f| stamp(f)).collect(),
            // SQLite writes to the database or to its write-ahead log.
            Located::OpenCode(db) => vec![stamp(db), wal_stamp(db)],
        };
        let key = (r.agent.clone(), sid.to_string());
        if self.polled.get(&key) == Some(&stamps) {
            return Ok(None);
        }
        self.polled.remove(&key);
        let polled = readers::poll_located(&self.loc, &r.agent, sid, located, &mut self.cursors)?;
        self.polled.insert(key, stamps);
        Ok(Some(polled))
    }

    /// The inputs of session `sid`, from the places found on earlier ticks
    /// (NFR-02: a search walks the transcript trees). A found file is kept
    /// while it is a file. A session with no transcript is searched again
    /// on every tick while its record is fresh (a new spawn), else after
    /// [`SEARCH_AGAIN_S`]. Claude's subagent files are listed again when a
    /// directory of the last listing changed.
    fn locate(&mut self, r: &Record, sid: &str, now: DateTime<Utc>) -> Result<Located, Unreadable> {
        if !matches!(r.agent.as_str(), "claude" | "codex" | "pi" | "prime") {
            return readers::locate(&self.loc, &r.agent, sid);
        }
        let key = (r.agent.clone(), sid.to_string());
        match self.places.get_mut(&key) {
            Some(Place::File { main, subagents }) if main.is_file() => {
                let mut files = vec![main.clone()];
                if r.agent == "claude" {
                    if !subagents.as_ref().is_some_and(Listing::unchanged) {
                        *subagents = Some(Listing::of(main));
                    }
                    files.extend(subagents.iter().flat_map(|l| l.files.iter().cloned()));
                }
                return Ok(Located::Files(files));
            }
            Some(Place::Missing { why, again }) => {
                let fresh = clock::stamp(now - Duration::minutes(FRESH_MIN));
                if now < *again && r.updated_at.as_str() < fresh.as_str() {
                    return Err(why.clone());
                }
            }
            _ => {}
        }
        self.searches += 1;
        let found = readers::locate(&self.loc, &r.agent, sid);
        match &found {
            Ok(Located::Files(files)) if !files.is_empty() => {
                let main = files[0].clone();
                let subagents = (r.agent == "claude").then(|| Listing::of(&main));
                self.places.insert(key, Place::File { main, subagents });
            }
            Err(why @ Unreadable::NoTranscript(_)) => {
                let again = now + Duration::seconds(SEARCH_AGAIN_S);
                let why = why.clone();
                self.places.insert(key, Place::Missing { why, again });
            }
            _ => {
                self.places.remove(&key);
            }
        }
        found
    }

    /// Fold signals into `quota.json`, probing when due. A file override
    /// is read, never written.
    fn update_quota(&mut self, now: DateTime<Utc>, signals: &[QuotaSignal]) -> Result<QuotaView> {
        if let Some(p) = &self.quota_env.quota_file {
            let file = QuotaFile::load(&self.state_root, Some(p))?;
            return Ok(QuotaView::new(file, now, self.policy.clone(), true));
        }
        let mut file = QuotaFile::load(&self.state_root, None).unwrap_or_default();
        file.apply_signals(signals, &self.policy);
        let due = match self.probing {
            Probing::Never => false,
            Probing::Scheduled => quota::probe_due(&file, now, self.policy.probe_interval_min),
            Probing::OnDemand => quota::probe_due(&file, now, self.policy.probe_on_demand_age_min),
        };
        if due {
            quota_probe::probe_all(
                &mut file,
                &self.bins,
                now,
                self.quota_env.probe_timeout,
                &self.quota_env.temp_root,
            );
        }
        file.write(&self.state_root, now, &self.policy)?;
        Ok(QuotaView::new(file, now, self.policy.clone(), false))
    }
}

/// One ledger file's last good records, and the file's `(length, mtime)`
/// when they were read.
struct Ledger {
    stamp: Option<(u64, Option<std::time::SystemTime>)>,
    records: Vec<Record>,
}

/// A record updated this many minutes ago or less is searched for its
/// transcript on every tick: a new spawn's transcript appears within seconds.
const FRESH_MIN: i64 = 10;

/// How long an older record with no transcript waits between 2 searches.
pub const SEARCH_AGAIN_S: i64 = 30;

/// Where one session's input is, as the collector last found it.
enum Place {
    /// The main transcript (Claude, Codex, pi, Prime), and Claude's
    /// subagent files as last listed.
    File {
        main: PathBuf,
        subagents: Option<Listing>,
    },
    /// None found; search again at `again`.
    Missing {
        why: Unreadable,
        again: DateTime<Utc>,
    },
}

/// A file or directory as it was: `(length, inode, modification time)`.
type Stamp = (u64, u64, Option<std::time::SystemTime>);

/// The stamp of `db`'s write-ahead log, an empty one stamped as none. The
/// first `sqlite3 -readonly` read of a WAL database creates an empty
/// `-wal` on Linux (sqlite 3.40), so the stamp a tick takes before that
/// read differs from the next tick's although nothing was written. A write
/// fills the log; a checkpoint that empties it writes the database.
fn wal_stamp(db: &Path) -> Option<Stamp> {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    stamp(Path::new(&wal)).filter(|s| s.0 > 0)
}

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((
        m.len(),
        super::cursor::file_identity(&m).1,
        m.modified().ok(),
    ))
}

/// The `*.jsonl` files under a Claude `<sid>/subagents/` directory, nested
/// ones included ([`readers::claude_subagent_files`]), with the stamp of
/// every directory walked: a file added anywhere below changes one of them.
struct Listing {
    dirs: Vec<(PathBuf, Option<Stamp>)>,
    files: Vec<PathBuf>,
}

impl Listing {
    fn of(main: &Path) -> Listing {
        let mut l = Listing {
            dirs: Vec::new(),
            files: Vec::new(),
        };
        l.walk(&main.with_extension("").join("subagents"));
        l.files.sort();
        l
    }

    fn walk(&mut self, dir: &Path) {
        // The stamp first: a file added during the walk shows next tick.
        self.dirs.push((dir.to_path_buf(), stamp(dir)));
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                self.files.push(path);
            }
        }
    }

    fn unchanged(&self) -> bool {
        self.dirs.iter().all(|(d, s)| stamp(d) == *s)
    }
}

/// A reader's usage, stamped with the record it belongs to and priced.
pub(crate) fn event_for(r: &Record, u: super::RawUsage, prices: &BTreeMap<String, Price>) -> Event {
    let model = if u.model.is_empty() {
        r.model.clone()
    } else {
        u.model.clone()
    };
    let cost_usd = usage::cost_of(prices, &model, &u.tokens.priced());
    Event {
        ts: u.ts,
        project: r.project.clone(),
        record_id: r.record_id.clone(),
        session_id: r.session_id.clone().unwrap_or_default(),
        event_id: u.event_id,
        role: r.role.clone(),
        teammate: r.tier.clone(),
        via: r.via.clone(),
        kind: r.kind.clone(),
        agent: r.agent.clone(),
        model,
        effort: u.effort.or_else(|| r.effort.clone()),
        phase: r.phase.map(|p| p.to_string()),
        // Ledgers written before `plan` existed still name the plan in the task.
        plan: r.plan.clone().or_else(|| super::plan_slug(&r.task)),
        subagent: u.subagent,
        delta: u.delta,
        tool_nested: u.tool_nested,
        idle: r.task == IDLE_TASK,
        tokens: u.tokens,
        cost_usd,
        harness_cost: u.harness_cost,
    }
}

/// [`store::add_unpriced`] on the table: per model, sorted by model.
fn add_unpriced<'t>(by_model: &mut BTreeMap<&'t str, UnpricedRow>, index: &'t Table, e: &Kept) {
    let model = &*index.names.names[e.model as usize];
    let row = by_model.entry(model).or_insert_with(|| UnpricedRow {
        model: model.to_string(),
        ..UnpricedRow::default()
    });
    row.events += 1;
    row.tokens.add(&e.tokens);
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn share(part: f64, whole: f64) -> Option<f64> {
    (whole > 0.0).then(|| part / whole)
}

/// Build the snapshot from the ledgers and the stored events. Pure.
pub fn build_snapshot(
    records: &[Record],
    events: &[Event],
    quota: QuotaView,
    unread: Vec<Unread>,
    now: DateTime<Utc>,
    policy: &Policy,
    collector: CollectorInfo,
) -> Snapshot {
    let _ = policy;
    let table = Table::from_events(events);
    let records: Vec<&Record> = records.iter().collect();
    build_indexed(&records, &table, quota, unread, now, collector)
}

/// [`build_snapshot`] from the store's [`Table`]. Each sum adds its events
/// in store order, as [`store::rollup`] does, so the costs are the same to
/// the last bit.
fn build_indexed(
    records: &[&Record],
    index: &Table,
    quota: QuotaView,
    unread: Vec<Unread>,
    now: DateTime<Utc>,
    collector: CollectorInfo,
) -> Snapshot {
    let done = done_ids(records.iter().copied());
    let names = index.names.len();
    let rows = index.rows.len();
    let is_done: Vec<bool> = (index.names.names.iter())
        .map(|n| done.contains(&**n))
        .collect();
    let since = WINDOWS.map(|w| window_start(w, now).unwrap_or_default());
    let recent = clock::stamp(now - Duration::minutes(5));

    // Per record: totals, last event, and the last 5 minutes for the rate.
    #[derive(Default)]
    struct Acc<'e> {
        events: u64,
        tokens: TokenClasses,
        cost: f64,
        unpriced: u64,
        last: Option<&'e str>,
        recent_tokens: u64,
        recent_cost: f64,
    }
    let mut per: Vec<Acc> = (0..names).map(|_| Acc::default()).collect();
    // Per window and row: tokens, and whether it has an event. Per window,
    // group and key: cost and done cost.
    let mut row_tokens = vec![TokenClasses::default(); WINDOWS.len() * rows];
    let mut row_seen = vec![false; WINDOWS.len() * rows];
    let mut costs = vec![(0.0f64, 0.0f64); WINDOWS.len() * store::GROUPS.len() * names];
    // Per window, group and key: the unpriced events. Per window: per model.
    let mut unpriced = vec![0u64; WINDOWS.len() * store::GROUPS.len() * names];
    let mut unpriced_models: [BTreeMap<&str, UnpricedRow>; WINDOWS.len()] = Default::default();
    // Insights over the 7-day window.
    let (mut total, mut orch, mut idle) = (0.0f64, 0.0f64, 0.0f64);
    let mut week_tokens = TokenClasses::default();
    let is_orch: Vec<bool> = index
        .rows
        .iter()
        .map(|r| &*index.names.names[r.groups[5] as usize] == KIND_ORCHESTRATOR)
        .collect();

    for e in &index.kept {
        let row = e.row as usize;
        let r = index.rows[row];
        let record = r.record as usize;
        let ts = &*e.ts;
        let cost = e.cost.unwrap_or(0.0);
        let no_price = e.cost.is_none();
        let a = &mut per[record];
        a.events += 1;
        a.tokens.add(&e.tokens);
        a.cost += cost;
        a.unpriced += u64::from(no_price);
        if a.last.is_none_or(|l| ts > l) {
            a.last = Some(ts);
        }
        if ts >= recent.as_str() {
            a.recent_tokens += e.tokens.total();
            a.recent_cost += cost;
        }
        for (w, start) in since.iter().enumerate() {
            if ts < start.as_str() {
                continue;
            }
            row_tokens[w * rows + row].add(&e.tokens);
            row_seen[w * rows + row] = true;
            for (g, key) in r.groups.iter().enumerate() {
                let at = (w * store::GROUPS.len() + g) * names + *key as usize;
                let c = &mut costs[at];
                c.0 += cost;
                if is_done[record] {
                    c.1 += cost;
                }
                unpriced[at] += u64::from(no_price);
            }
            if no_price {
                add_unpriced(&mut unpriced_models[w], index, e);
            }
        }
        if ts >= since[2].as_str() {
            total += cost;
            if is_orch[row] {
                orch += cost;
            }
            if e.idle {
                idle += cost;
            }
            week_tokens.add(&e.tokens);
        }
    }

    let mut rollups = BTreeMap::new();
    for (w, window) in WINDOWS.iter().enumerate() {
        // Per group: key -> (tokens, records), from the rows in the window.
        let mut groups: [BTreeMap<u32, (TokenClasses, BTreeSet<u32>)>; 6] = Default::default();
        for (row, r) in index.rows.iter().enumerate() {
            if !row_seen[w * rows + row] {
                continue;
            }
            for (g, key) in r.groups.iter().enumerate() {
                let (tokens, recs) = groups[g].entry(*key).or_default();
                tokens.add(&row_tokens[w * rows + row]);
                recs.insert(r.record);
            }
        }
        let [by_teammate, by_phase, by_agent, by_project, by_plan, by_kind] =
            std::array::from_fn(|g| {
                store::rows_from(std::mem::take(&mut groups[g]).into_iter().map(
                    |(key, (tokens, recs))| {
                        let at = (w * store::GROUPS.len() + g) * names + key as usize;
                        let (cost, done_cost) = costs[at];
                        let sums = GroupSums {
                            tokens,
                            cost,
                            records: recs.len() as u64,
                            done: recs.iter().filter(|r| is_done[**r as usize]).count() as u64,
                            done_cost,
                            unpriced_events: unpriced[at],
                        };
                        (index.names.names[key as usize].to_string(), sums)
                    },
                ))
            });
        rollups.insert(
            window.to_string(),
            Rollup {
                by_teammate,
                by_phase,
                by_agent,
                by_project,
                by_plan,
                by_kind,
                unpriced: std::mem::take(&mut unpriced_models[w])
                    .into_values()
                    .collect(),
            },
        );
    }

    let ten_min_ago = clock::stamp(now - Duration::minutes(10));
    let mut live = Vec::new();
    for r in records {
        let a = index
            .names
            .get(&r.record_id)
            .map(|id| &per[id as usize])
            .filter(|a| a.events > 0);
        let last = a.and_then(|a| a.last.map(str::to_string));
        let recent_event = last.as_deref().is_some_and(|l| l >= ten_min_ago.as_str());
        // A live execution always shows; a finished one while its last
        // event is recent. A failed one never does: no agent runs for it.
        let status = r.execution_status();
        let failed = matches!(
            status,
            ExecutionStatus::Failed { .. } | ExecutionStatus::LaunchFailed { .. }
        );
        if failed || (!status.is_live() && !recent_event) {
            continue;
        }
        let idle = r.task == IDLE_TASK || (status.is_live() && !recent_event);
        let assessment = quota.assess(&r.agent, &r.model);
        live.push(LiveRow {
            project: r.project.clone(),
            record_id: r.record_id.clone(),
            role: r.role.clone(),
            teammate: r.tier.clone(),
            via: r.via.clone(),
            kind: r.kind.clone(),
            agent: r.agent.clone(),
            model: r.model.clone(),
            effort: r.effort.clone(),
            phase: r.phase.map(|p| p.to_string()),
            plan: r.plan.clone(),
            status: r.status.clone(),
            task_head: r.task.chars().take(60).collect(),
            tokens: a.map(|a| a.tokens).unwrap_or_default(),
            cost_usd: a.map(|a| a.cost).unwrap_or(0.0),
            unpriced_events: a.map(|a| a.unpriced).unwrap_or(0),
            rate_tokens_per_min: a.map(|a| a.recent_tokens as f64 / 5.0).unwrap_or(0.0),
            rate_usd_per_hour: a.map(|a| a.recent_cost * 12.0).unwrap_or(0.0),
            last_event_at: last,
            idle,
            pool: assessment.pool,
            pool_state: assessment.state.as_str().to_string(),
        });
    }
    // Orchestrators first, then by project and role.
    live.sort_by(|a, b| {
        (a.kind != KIND_ORCHESTRATOR, &a.project, &a.role).cmp(&(
            b.kind != KIND_ORCHESTRATOR,
            &b.project,
            &b.role,
        ))
    });

    let top_plan = rollups
        .get("7d")
        .and_then(|r| r.by_plan.iter().find(|row| row.key != "-"))
        .map(|row| row.key.clone());

    let mut pools = quota.file.pools.clone();
    for (name, reading) in pools.iter_mut() {
        let a = quota.assess_pool(name, None, None);
        reading.state = a.state.as_str().to_string();
        reading.reason = Some(a.reason);
    }
    let projects = live
        .iter()
        .filter_map(|l| l.project.as_deref())
        .collect::<BTreeSet<_>>()
        .len();
    Snapshot {
        schema: 1,
        generated_at: clock::stamp(now),
        collector,
        pools,
        live,
        rollups,
        insights: Insights {
            orchestrator_share: share(orch, total),
            cache_hit: week_tokens.cache_hit(),
            idle_spend_share: share(idle, total),
            top_plan,
        },
        unread,
        projects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::testkit::{property, SplitMix64};

    fn now() -> DateTime<Utc> {
        clock::parse("2026-09-28T18:00:00Z").unwrap()
    }

    fn pick<'a>(rng: &mut SplitMix64, from: &[&'a str]) -> &'a str {
        from[rng.below(from.len() as u64) as usize]
    }

    /// Events over 8 days around every window start, some keys missing.
    fn events(rng: &mut SplitMix64) -> Vec<Event> {
        let records = ["r0", "r1", "r2", "r3"];
        let maybe = |rng: &mut SplitMix64, from: &[&str]| {
            let v = pick(rng, from);
            (rng.below(3) > 0).then(|| v.to_string())
        };
        (0..rng.below(80))
            .map(|i| {
                let ts = now() - Duration::seconds(rng.below(8 * 86_400) as i64);
                let frac = if rng.below(4) == 0 { ".5Z" } else { "Z" };
                Event {
                    ts: format!("{}{frac}", &clock::stamp(ts)[..19]),
                    project: maybe(rng, &["/a", "/b"]),
                    record_id: pick(rng, &records).into(),
                    session_id: "s".into(),
                    event_id: format!("e{i}"),
                    role: "r".into(),
                    teammate: pick(rng, &["sonnet", "opus"]).into(),
                    via: None,
                    kind: pick(rng, &["worker", KIND_ORCHESTRATOR]).into(),
                    agent: pick(rng, &["claude", "codex"]).into(),
                    model: "m".into(),
                    effort: None,
                    phase: maybe(rng, &["implementation"]),
                    plan: maybe(rng, &["p1", "p2"]),
                    subagent: false,
                    delta: false,
                    tool_nested: false,
                    idle: rng.below(5) == 0,
                    tokens: TokenClasses {
                        input: rng.below(1000),
                        cache_read: rng.below(1000),
                        output: rng.below(100),
                        ..TokenClasses::default()
                    },
                    cost_usd: (rng.below(6) > 0).then(|| rng.below(1_000_000) as f64 / 7e6),
                    harness_cost: None,
                }
            })
            .collect()
    }

    fn records() -> Vec<Record> {
        let ledger = ["r0", "r1", "r2", "r3"].map(|id| {
            serde_json::json!({
                "record_id": id, "session_id": "s", "agent": "claude", "tier": "sonnet",
                "model": "m", "role": id, "status": if id == "r1" { "done" } else { "working" },
                "task": "t", "history": [], "created_at": "2026-09-20T00:00:00Z",
                "updated_at": "2026-09-28T17:00:00Z"
            })
        });
        serde_json::from_value(serde_json::Value::from(ledger.to_vec())).unwrap()
    }

    fn snapshot(records: &[Record], index: &Table) -> Snapshot {
        let records: Vec<&Record> = records.iter().collect();
        let policy = Policy::default();
        let quota = QuotaView::new(QuotaFile::default(), now(), policy, false);
        let info = CollectorInfo::default();
        build_indexed(&records, index, quota, Vec::new(), now(), info)
    }

    /// NFR-02: an empty write-ahead log stamps as no log. On Linux the
    /// first `sqlite3 -readonly` read creates an empty `-wal`, and a quiet
    /// tick then saw a change and saved the cursors again.
    #[test]
    fn nfr_02_an_empty_wal_stamps_as_no_wal() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("opencode.db");
        std::fs::write(&db, "db").unwrap();
        assert_eq!(wal_stamp(&db), None);
        let wal = tmp.path().join("opencode.db-wal");
        std::fs::write(&wal, "").unwrap();
        assert_eq!(wal_stamp(&db), None, "an empty log is no log");
        std::fs::write(&wal, "frame").unwrap();
        assert_eq!(wal_stamp(&db), stamp(&wal));
        assert!(wal_stamp(&db).is_some());
    }

    /// NFR-02: the fold over the store's table gives what the per-event sums
    /// give, and a table grown tick by tick gives what a fresh one gives.
    #[test]
    fn nfr_02_indexed_fold_matches_the_event_sums() {
        let records = records();
        property(0x4e46_5230_3220_0001, 200, |rng| {
            let events = events(rng);
            let snap = snapshot(&records, &Table::from_events(&events));

            let done = done_record_ids(&records);
            for w in WINDOWS {
                let since = window_start(w, now());
                let want = store::rollup(&events, since.as_deref(), &done);
                assert_eq!(snap.rollups[w], want, "{w}");
            }
            let week: Vec<&Event> = (events.iter())
                .filter(|e| e.ts >= window_start("7d", now()).unwrap())
                .collect();
            let cost = |f: &dyn Fn(&Event) -> bool| -> f64 {
                week.iter()
                    .filter(|e| f(e))
                    .map(|e| e.cost_usd.unwrap_or(0.0))
                    .sum()
            };
            let total = cost(&|_| true);
            assert_eq!(
                snap.insights.orchestrator_share,
                share(cost(&|e| e.kind == KIND_ORCHESTRATOR), total)
            );
            assert_eq!(
                snap.insights.idle_spend_share,
                share(cost(&|e| e.idle), total)
            );
            for row in &snap.live {
                let mine = events.iter().filter(|e| e.record_id == row.record_id);
                let cost: f64 = mine.clone().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
                assert_eq!(row.cost_usd, cost, "{}", row.record_id);
                let unpriced = mine.clone().filter(|e| e.cost_usd.is_none()).count();
                assert_eq!(row.unpriced_events, unpriced as u64, "{}", row.record_id);
                assert_eq!(row.last_event_at, mine.map(|e| e.ts.clone()).max());
            }

            // The store's table, appended in 2 ticks and reopened from the
            // files, folds as a fresh table over the same events in the same
            // order. An append files its events by day, oldest file first.
            let tmp = tempfile::tempdir().unwrap();
            let mut store = Store::open(tmp.path(), now(), 35).unwrap();
            let cut = events.len() / 2;
            let mut order = Vec::new();
            for batch in [&events[..cut], &events[cut..]] {
                store.append(batch.to_vec()).unwrap();
                let mut by_day = batch.to_vec();
                by_day.sort_by(|a, b| a.ts[..10].cmp(&b.ts[..10]));
                order.extend(by_day);
            }
            let fresh = |events: &[Event]| snapshot(&records, &Table::from_events(events));
            assert_eq!(snapshot(&records, &store.table), fresh(&order));
            let reopened = Store::open(tmp.path(), now(), 35).unwrap();
            assert_eq!(reopened.len(), events.len());
            let on_disk = store::read_all(tmp.path());
            assert_eq!(snapshot(&records, &reopened.table), fresh(&on_disk));
        });
    }
}
