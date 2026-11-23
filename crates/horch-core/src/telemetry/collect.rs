use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::cursor::Cursor;
use super::readers::{self, Cursors, Located, Polled, Unreadable};
use super::store::{self, GroupSums, Kept, Rollup, Store, Table, UnpricedRow};
use super::timing::TickTiming;
use super::{Event, Observation, QuotaSignal, TokenClasses, Unread};
use crate::clock;
use crate::execution::legacy::{Record, KIND_ORCHESTRATOR};
use crate::execution::model::ExecutionStatus;
use crate::execution::store as execution_store;
use crate::routing::policy::Policy;
use crate::routing::quota::{self, Assessment, QuotaFile, QuotaView};
use crate::routing::quota_probe::{self, ProbeBins};
use crate::routing::snapshot::QuotaEnv;
use crate::usage::{self, Locations, Price};

pub fn read_ledgers(state_root: &Path) -> Vec<Record> {
    let (db, task_ledger) = execution_store::session_files(state_root);
    execution_store::read_all_sessions(state_root, &db, task_ledger.as_deref())
        .into_iter()
        .flat_map(|(_, records)| records)
        .collect()
}

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

pub const IDLE_TASK: &str = "(idle - awaiting assignment)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probing {
    Never,
    Scheduled,
    OnDemand,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectorInfo {
    pub pid: u32,
    pub version: String,
    pub started_at: String,
}

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
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unpriced_events: u64,
    pub rate_tokens_per_min: f64,
    pub rate_usd_per_hour: f64,
    pub last_event_at: Option<String>,
    pub idle: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub generated_at: String,
    pub collector: CollectorInfo,
    pub pools: BTreeMap<String, quota::PoolReading>,
    pub live: Vec<LiveRow>,
    pub rollups: BTreeMap<String, Rollup>,
    pub insights: Insights,
    pub unread: Vec<Unread>,
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

pub fn window_start(window: &str, now: DateTime<Utc>) -> Option<String> {
    match window {
        "5h" => Some(clock::stamp(now - Duration::hours(5))),
        "today" => Some(format!("{}T00:00:00Z", now.format("%Y-%m-%d"))),
        "7d" => Some(clock::stamp(now - Duration::days(7))),
        _ => None,
    }
}

pub const WINDOWS: [&str; 3] = ["5h", "today", "7d"];

pub struct Collector {
    pub state_root: PathBuf,
    pub loc: Locations,
    pub prices: BTreeMap<String, Price>,
    pub policy: Policy,
    pub store: Store,
    pub cursors: Cursors,
    pub probing: Probing,
    pub bins: ProbeBins,
    pub quota_env: QuotaEnv,
    pub info: CollectorInfo,
    last_good: BTreeMap<PathBuf, LedgerSnapshot>,
    task_ledger: Option<PathBuf>,
    session_db: PathBuf,
    places: HashMap<(String, String), Place>,
    polled: HashMap<(String, String), Vec<Option<Stamp>>>,
    pub searches: u64,
    pub batch: usize,
    pub peak_pending: usize,
    pub fail_after_append: bool,
    pub abort_after_append: bool,
    pub timing: TickTiming,
    pub print_timing: bool,
    snapshot_hash: Option<u64>,
    tidied_at: Option<DateTime<Utc>>,
}

impl Collector {
    pub fn open(state_root: &Path, loc: Locations, probing: Probing) -> Result<Collector> {
        Self::open_at(state_root, loc, probing, clock::now())
    }

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
        let mut c = c.with_faults(&ctx.settings.faults);
        c.task_ledger = Some(ctx.paths.ledger_db.clone());
        c.session_db = execution_store::session_db(&ctx.paths);
        use crate::runtime::EnvSource;
        c.print_timing = crate::runtime::ProcessEnv
            .var("HORCH_TELEMETRY_TIMING")
            .as_deref()
            == Some("1");
        Ok(c)
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
            task_ledger: None,
            session_db: state_root.join(execution_store::STATE_DB),
            places: HashMap::new(),
            polled: HashMap::new(),
            searches: 0,
            batch: BATCH,
            peak_pending: 0,
            fail_after_append: false,
            abort_after_append: false,
            timing: TickTiming::default(),
            print_timing: false,
            snapshot_hash: None,
            tidied_at: None,
        })
    }

    pub(crate) fn with_faults(mut self, faults: &crate::runtime::Faults) -> Collector {
        self.fail_after_append = faults.has("after-append");
        self.abort_after_append = faults.has("abort-after-append");
        self
    }

    pub fn ledgers(&mut self) -> Vec<Record> {
        self.refresh_ledgers();
        self.merged()
    }

    fn merged(&self) -> Vec<Record> {
        let legacy = (self.last_good.iter())
            .map(|(p, l)| (p.clone(), l.records.clone()))
            .collect();
        let task_ledger = self.task_ledger.as_deref().filter(|db| db.is_file());
        execution_store::merge_projects(&self.state_root, legacy, &self.session_db, task_ledger)
            .into_iter()
            .flat_map(|p| p.records)
            .collect()
    }

    pub fn project_rollups(&self, project: &str, now: DateTime<Utc>) -> BTreeMap<String, Rollup> {
        let index = &self.store.table;
        let records = self.merged();
        let mut fold = RollupFold::new(index, &done_ids(&records), now);
        if let Some(id) = index.names.get(project) {
            let mine: Vec<bool> = index.rows.iter().map(|r| r.groups[3] == id).collect();
            for e in index.kept.iter().filter(|e| mine[e.row as usize]) {
                fold.add(e);
            }
        }
        fold.finish()
    }

    fn refresh_ledgers(&mut self) {
        let paths = execution_store::ledger_paths(&self.state_root);
        self.last_good.retain(|p, _| paths.contains(p));
        for path in paths {
            let stamp = stamp(&path);
            if stamp.is_some() && self.last_good.get(&path).map(|l| l.stamp) == Some(stamp) {
                continue;
            }
            if let Some(records) = execution_store::read_ledger_file(&path) {
                self.last_good
                    .insert(path, LedgerSnapshot { stamp, records });
            }
        }
    }

    pub fn tick(&mut self, now: DateTime<Utc>) -> Result<Snapshot> {
        self.timing = TickTiming::default();
        let start = Instant::now();
        self.refresh_ledgers();
        let merged = self.merged();
        let records: Vec<&Record> = merged.iter().collect();
        self.timing.lap("ledgers", start);
        let snapshot = self.tick_on(&records, now);
        self.timing.total = start.elapsed();
        if self.print_timing {
            eprintln!("{}", self.timing.line());
        }
        snapshot
    }

    fn tick_on(&mut self, records: &[&Record], now: DateTime<Utc>) -> Result<Snapshot> {
        let oldest = clock::stamp(now - Duration::days(self.policy.retention_days));
        self.store.advance(now);
        let batch = self.batch.max(1);
        let mut pass = TickPass::default();
        for r in records
            .iter()
            .filter(|r| r.updated_at.as_str() >= oldest.as_str())
        {
            if let Some(sid) = r.session_id.as_deref().filter(|s| !s.is_empty()) {
                self.drain_record(r, sid, now, batch, &mut pass)?;
            } else {
                pass.unread.push(unread_row(r, Unreadable::NoSessionId));
            }
        }
        pass.moved |= self.tidy_if_due(records, &oldest, now);
        self.store_batch(pass.pending, pass.moved.then_some(true))?;

        let at = Instant::now();
        let quota = self.update_quota(now, &pass.signals)?;
        let at = self.timing.lap("quota", at);
        let snapshot = build_indexed(
            records,
            &self.store.table,
            quota,
            pass.unread,
            now,
            self.info.clone(),
        );
        let at = self.timing.lap("build", at);
        self.write_snapshot_if_changed(&snapshot, at)?;
        Ok(snapshot)
    }

    fn drain_record(
        &mut self,
        r: &Record,
        sid: &str,
        now: DateTime<Utc>,
        batch: usize,
        pass: &mut TickPass,
    ) -> Result<()> {
        loop {
            let polled = match self.poll(r, sid, now, batch - pass.pending.len()) {
                Ok(None) => break,
                Ok(Some(polled)) => polled,
                Err(why) => {
                    pass.unread.push(unread_row(r, why));
                    break;
                }
            };
            pass.moved = true;
            pass.absorb(r, polled.observations, &self.prices);
            self.peak_pending = self.peak_pending.max(pass.pending.len());
            if pass.pending.len() >= batch {
                self.store_batch(std::mem::take(&mut pass.pending), Some(false))?;
                pass.moved = false;
            }
            if !polled.more {
                break;
            }
        }
        Ok(())
    }

    fn tidy_if_due(&mut self, records: &[&Record], oldest: &str, now: DateTime<Utc>) -> bool {
        let due = (self.tidied_at).is_none_or(|t| now - t >= Duration::minutes(TIDY_EVERY_MIN));
        if !due {
            return false;
        }
        self.tidied_at = Some(now);
        let at = Instant::now();
        let changed = self.tidy_cursors(records, oldest, now);
        self.timing.lap("cursor_save", at);
        changed
    }

    fn write_snapshot_if_changed(&mut self, snapshot: &Snapshot, at: Instant) -> Result<()> {
        let path = snapshot_path(&self.state_root);
        let hash = content_hash(snapshot)?;
        let known = *self
            .snapshot_hash
            .get_or_insert_with(|| file_hash(&path).unwrap_or(!hash));
        let at = self.timing.lap("serialize", at);
        if known != hash {
            let json = serde_json::to_vec(snapshot)?;
            let at = self.timing.lap("serialize", at);
            store::replace_private(&path, &json)?;
            self.snapshot_hash = Some(hash);
            self.timing.lap("write", at);
            self.timing.wrote("snapshot", json.len() as u64);
        }
        Ok(())
    }

    fn store_batch(&mut self, events: Vec<Event>, save: Option<bool>) -> Result<()> {
        let at = Instant::now();
        let bytes = self.store.bytes_written;
        self.store.append(events)?;
        let at = self.timing.lap("append", at);
        self.timing
            .wrote("events", self.store.bytes_written - bytes);
        if self.abort_after_append {
            eprintln!("HORCH_FAULT=abort-after-append: dying before the cursor save");
            std::process::abort();
        }
        if self.fail_after_append {
            anyhow::bail!("HORCH_FAULT=after-append: stopping before the cursor save");
        }
        if let Some(sync) = save {
            let bytes = store::save_cursors(self.store.dir(), &self.cursors, sync)?;
            self.timing.lap("cursor_save", at);
            self.timing.wrote("cursors", bytes as u64);
        }
        Ok(())
    }

    fn poll(
        &mut self,
        r: &Record,
        sid: &str,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Option<Polled>, Unreadable> {
        let at = Instant::now();
        let located = self.locate(r, sid, now);
        let located = located.inspect_err(|_| {
            self.timing.lap("resolve", at);
        })?;
        let stamps: Vec<Option<Stamp>> = match &located {
            Located::Files(files) => files.iter().map(|f| stamp(f)).collect(),
            Located::OpenCode(db) => vec![stamp(db), wal_stamp(db)],
        };
        let key = (r.agent.clone(), sid.to_string());
        let at = self.timing.lap("resolve", at);
        if self.polled.get(&key) == Some(&stamps) {
            return Ok(None);
        }
        self.polled.remove(&key);
        let polled =
            readers::poll_located(&self.loc, &r.agent, sid, located, &mut self.cursors, limit);
        self.timing.lap("read", at);
        let polled = polled?;
        if !polled.more {
            self.polled.insert(key, stamps);
        }
        Ok(Some(polled))
    }

    fn locate(&mut self, r: &Record, sid: &str, now: DateTime<Utc>) -> Result<Located, Unreadable> {
        if !matches!(r.agent.as_str(), "claude" | "codex" | "pi" | "prime") {
            return readers::locate(&self.loc, &r.agent, sid);
        }
        let key = (r.agent.clone(), sid.to_string());
        let cached = self.cached(&key, r, now);
        cached.unwrap_or_else(|| self.search(key, r, sid, now))
    }

    fn cached(
        &mut self,
        key: &(String, String),
        r: &Record,
        now: DateTime<Utc>,
    ) -> Option<Result<Located, Unreadable>> {
        match self.places.get_mut(key) {
            Some(Place::File { main, subagents }) if main.is_file() => {
                Some(Ok(Located::Files(cached_files(main, subagents, &r.agent))))
            }
            Some(Place::Missing { why, again }) => {
                let fresh = clock::stamp(now - Duration::minutes(FRESH_MIN));
                (now < *again && r.updated_at.as_str() < fresh.as_str()).then(|| Err(why.clone()))
            }
            _ => None,
        }
    }

    fn search(
        &mut self,
        key: (String, String),
        r: &Record,
        sid: &str,
        now: DateTime<Utc>,
    ) -> Result<Located, Unreadable> {
        self.searches += 1;
        let found = readers::locate(&self.loc, &r.agent, sid);
        self.remember(key, &r.agent, &found, now);
        found
    }

    fn remember(
        &mut self,
        key: (String, String),
        agent: &str,
        found: &Result<Located, Unreadable>,
        now: DateTime<Utc>,
    ) {
        match found {
            Ok(Located::Files(files)) if !files.is_empty() => {
                let main = files[0].clone();
                let subagents = (agent == "claude").then(|| Listing::of(&main));
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
    }

    fn tidy_cursors(&mut self, records: &[&Record], oldest: &str, now: DateTime<Utc>) -> bool {
        let owners = session_owners(records, oldest);
        let quiet_since = std::time::SystemTime::from(now - Duration::hours(QUIET_H));
        let mut changed = false;
        self.cursors.retain(|_, c| {
            let tidy = tidy_cursor(c, &owners, quiet_since);
            changed |= tidy != Tidy::Keep;
            tidy != Tidy::Drop
        });
        changed
    }

    fn update_quota(&mut self, now: DateTime<Utc>, signals: &[QuotaSignal]) -> Result<QuotaView> {
        match &self.quota_env.quota_file {
            Some(p) => {
                let file = QuotaFile::load(&self.state_root, Some(p))?;
                Ok(QuotaView::new(file, now, self.policy.clone(), true))
            }
            None => self.merged_quota(now, signals),
        }
    }

    fn merged_quota(&mut self, now: DateTime<Utc>, signals: &[QuotaSignal]) -> Result<QuotaView> {
        let path = quota::quota_path(&self.state_root);
        let on_disk = path
            .is_file()
            .then(|| QuotaFile::load(&self.state_root, None).ok())
            .flatten();
        let mut file = on_disk.clone().unwrap_or_default();
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
        let next = refreshed(&file, now, &self.policy);
        if on_disk.as_ref() != Some(&next) {
            file.write(&self.state_root, now, &self.policy)?;
            let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
            self.timing.wrote("quota", bytes);
        }
        Ok(QuotaView::new(file, now, self.policy.clone(), false))
    }
}

fn cached_files(main: &Path, subagents: &mut Option<Listing>, agent: &str) -> Vec<PathBuf> {
    let mut files = vec![main.to_path_buf()];
    if agent == "claude" {
        if !subagents.as_ref().is_some_and(Listing::unchanged) {
            *subagents = Some(Listing::of(main));
        }
        files.extend(subagents.iter().flat_map(|l| l.files.iter().cloned()));
    }
    files
}

#[derive(Default)]
struct TickPass {
    pending: Vec<Event>,
    signals: Vec<QuotaSignal>,
    unread: Vec<Unread>,
    moved: bool,
}

impl TickPass {
    fn absorb(
        &mut self,
        r: &Record,
        observations: Vec<Observation>,
        prices: &BTreeMap<String, Price>,
    ) {
        for o in observations {
            match o {
                Observation::Usage(u) => self.pending.push(event_for(r, u, prices)),
                Observation::Quota(s) => self.signals.push(s),
            }
        }
    }
}

#[derive(PartialEq)]
enum Tidy {
    Keep,
    Trimmed,
    Drop,
}

fn session_owners<'r>(records: &[&'r Record], oldest: &str) -> HashMap<(&'r str, &'r str), bool> {
    let mut owners = HashMap::new();
    let owns = |r: &&&Record| r.updated_at.as_str() >= oldest || r.execution_status().is_live();
    for r in records.iter().filter(owns) {
        let Some(sid) = r.session_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        let done = r.execution_status().is_terminal();
        *owners.entry((&*r.agent, sid)).or_insert(true) &= done;
    }
    owners
}

fn tidy_cursor(
    c: &mut Cursor,
    owners: &HashMap<(&str, &str), bool>,
    quiet_since: std::time::SystemTime,
) -> Tidy {
    if c.path.is_empty() || c.agent.is_empty() {
        return Tidy::Keep;
    }
    match std::fs::metadata(&c.path).and_then(|m| m.modified()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Tidy::Drop,
        Ok(modified) if modified < quiet_since => tidy_quiet(c, owners),
        _ => Tidy::Keep,
    }
}

fn tidy_quiet(c: &mut Cursor, owners: &HashMap<(&str, &str), bool>) -> Tidy {
    match owners.get(&(c.agent.as_str(), c.session_id.as_str())) {
        None => Tidy::Drop,
        Some(true) if c.recent.len() > QUIET_RECENT_IDS => {
            let extra = c.recent.len() - QUIET_RECENT_IDS;
            c.recent.drain(..extra);
            Tidy::Trimmed
        }
        Some(_) => Tidy::Keep,
    }
}

fn refreshed(file: &QuotaFile, now: DateTime<Utc>, policy: &Policy) -> QuotaFile {
    let view = QuotaView::new(file.clone(), now, policy.clone(), false);
    QuotaFile {
        pools: shown_pools(&view),
        ..file.clone()
    }
}

fn content_hash(snapshot: &Snapshot) -> Result<u64> {
    let content = Snapshot {
        generated_at: String::new(),
        ..snapshot.clone()
    };
    Ok(bytes_hash(&serde_json::to_vec(&content)?))
}

fn bytes_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn file_hash(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let head = b",\"generated_at\":\"";
    let at = bytes.windows(head.len()).position(|w| w == head)? + head.len();
    if !bytes.starts_with(b"{\"schema\":") || at > 40 {
        return None;
    }
    let end = at + bytes[at..].iter().position(|b| *b == b'"')?;
    Some(bytes_hash(&[&bytes[..at], &bytes[end..]].concat()))
}

struct LedgerSnapshot {
    stamp: Option<Stamp>,
    records: Vec<Record>,
}

const FRESH_MIN: i64 = 10;

pub const QUIET_H: i64 = 24;

pub const TIDY_EVERY_MIN: i64 = 60;

pub const QUIET_RECENT_IDS: usize = 4;

pub const SEARCH_AGAIN_S: i64 = 30;

enum Place {
    File {
        main: PathBuf,
        subagents: Option<Listing>,
    },
    Missing {
        why: Unreadable,
        again: DateTime<Utc>,
    },
}

type Stamp = (u64, u64, Option<std::time::SystemTime>);

pub const BATCH: usize = 10_000;

fn wal_stamp(db: &Path) -> Option<Stamp> {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    stamp(Path::new(&wal)).filter(|s| s.0 > 0)
}

fn unread_row(r: &Record, why: Unreadable) -> Unread {
    Unread {
        record_id: r.record_id.clone(),
        role: r.role.clone(),
        agent: r.agent.clone(),
        reason: why.to_string(),
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    let identity = horch_platform::fs::file_identity_of_target(path).ok()?;
    Some((m.len(), identity.1, m.modified().ok()))
}

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

struct RollupFold<'t> {
    index: &'t Table,
    is_done: Vec<bool>,
    since: [String; WINDOWS.len()],
    row_tokens: Vec<TokenClasses>,
    row_seen: Vec<bool>,
    costs: Vec<(f64, f64)>,
    unpriced: Vec<u64>,
    unpriced_models: [BTreeMap<&'t str, UnpricedRow>; WINDOWS.len()],
}

impl<'t> RollupFold<'t> {
    fn new(index: &'t Table, done: &BTreeSet<String>, now: DateTime<Utc>) -> Self {
        let (names, rows) = (index.names.len(), index.rows.len());
        let keys = WINDOWS.len() * store::GROUPS.len() * names;
        RollupFold {
            index,
            is_done: (index.names.names.iter())
                .map(|n| done.contains(&**n))
                .collect(),
            since: WINDOWS.map(|w| window_start(w, now).unwrap_or_default()),
            row_tokens: vec![TokenClasses::default(); WINDOWS.len() * rows],
            row_seen: vec![false; WINDOWS.len() * rows],
            costs: vec![(0.0, 0.0); keys],
            unpriced: vec![0; keys],
            unpriced_models: Default::default(),
        }
    }

    fn add(&mut self, e: &'t Kept) {
        let (names, rows) = (self.index.names.len(), self.index.rows.len());
        let row = e.row as usize;
        let r = self.index.rows[row];
        let ts = &*e.ts;
        let cost = e.cost.unwrap_or(0.0);
        let no_price = e.cost.is_none();
        for (w, start) in self.since.iter().enumerate() {
            if ts < start.as_str() {
                continue;
            }
            self.row_tokens[w * rows + row].add(&e.tokens);
            self.row_seen[w * rows + row] = true;
            for (g, key) in r.groups.iter().enumerate() {
                let at = (w * store::GROUPS.len() + g) * names + *key as usize;
                let c = &mut self.costs[at];
                c.0 += cost;
                if self.is_done[r.record as usize] {
                    c.1 += cost;
                }
                self.unpriced[at] += u64::from(no_price);
            }
            if no_price {
                add_unpriced(&mut self.unpriced_models[w], self.index, e);
            }
        }
    }

    fn finish(mut self) -> BTreeMap<String, Rollup> {
        let index = self.index;
        let (names, rows) = (index.names.len(), index.rows.len());
        let mut rollups = BTreeMap::new();
        for (w, window) in WINDOWS.iter().enumerate() {
            let mut groups: [BTreeMap<u32, (TokenClasses, BTreeSet<u32>)>; 6] = Default::default();
            for (row, r) in index.rows.iter().enumerate() {
                if !self.row_seen[w * rows + row] {
                    continue;
                }
                for (g, key) in r.groups.iter().enumerate() {
                    let (tokens, recs) = groups[g].entry(*key).or_default();
                    tokens.add(&self.row_tokens[w * rows + row]);
                    recs.insert(r.record);
                }
            }
            let [by_teammate, by_phase, by_agent, by_project, by_plan, by_kind] =
                std::array::from_fn(|g| {
                    store::rows_from(std::mem::take(&mut groups[g]).into_iter().map(
                        |(key, (tokens, recs))| {
                            let at = (w * store::GROUPS.len() + g) * names + key as usize;
                            let (cost, done_cost) = self.costs[at];
                            let done = recs.iter().filter(|r| self.is_done[**r as usize]);
                            let sums = GroupSums {
                                tokens,
                                cost,
                                records: recs.len() as u64,
                                done: done.count() as u64,
                                done_cost,
                                unpriced_events: self.unpriced[at],
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
                    unpriced: std::mem::take(&mut self.unpriced_models[w])
                        .into_values()
                        .collect(),
                },
            );
        }
        rollups
    }
}

fn build_indexed(
    records: &[&Record],
    index: &Table,
    quota: QuotaView,
    unread: Vec<Unread>,
    now: DateTime<Utc>,
    collector: CollectorInfo,
) -> Snapshot {
    let mut fold = RollupFold::new(index, &done_ids(records.iter().copied()), now);
    let recent = clock::stamp(now - Duration::minutes(5));
    let (per, week) = fold_events(index, &mut fold, &recent);
    let rollups = fold.finish();
    let live = live_rows(records, index, &per, &quota, now);
    let top_plan = rollups
        .get("7d")
        .and_then(|r| r.by_plan.iter().find(|row| row.key != "-"))
        .map(|row| row.key.clone());
    let projects = live
        .iter()
        .filter_map(|l| l.project.as_deref())
        .collect::<BTreeSet<_>>()
        .len();
    Snapshot {
        schema: 1,
        generated_at: clock::stamp(now),
        collector,
        pools: shown_pools(&quota),
        live,
        rollups,
        insights: Insights {
            orchestrator_share: share(week.orch, week.total),
            cache_hit: week.tokens.cache_hit(),
            idle_spend_share: share(week.idle, week.total),
            top_plan,
        },
        unread,
        projects,
    }
}

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

impl<'e> Acc<'e> {
    fn add(&mut self, e: &'e Kept, recent: &str) {
        let ts = &*e.ts;
        let cost = e.cost.unwrap_or(0.0);
        self.events += 1;
        self.tokens.add(&e.tokens);
        self.cost += cost;
        self.unpriced += u64::from(e.cost.is_none());
        if self.last.is_none_or(|l| ts > l) {
            self.last = Some(ts);
        }
        if ts >= recent {
            self.recent_tokens += e.tokens.total();
            self.recent_cost += cost;
        }
    }
}

#[derive(Default)]
struct WeekTotals {
    total: f64,
    orch: f64,
    idle: f64,
    tokens: TokenClasses,
}

fn fold_events<'e>(
    index: &'e Table,
    fold: &mut RollupFold<'e>,
    recent: &str,
) -> (Vec<Acc<'e>>, WeekTotals) {
    let mut per: Vec<Acc> = (0..index.names.len()).map(|_| Acc::default()).collect();
    let mut week = WeekTotals::default();
    let is_orch: Vec<bool> = index
        .rows
        .iter()
        .map(|r| &*index.names.names[r.groups[5] as usize] == KIND_ORCHESTRATOR)
        .collect();
    for e in &index.kept {
        let row = e.row as usize;
        let record = index.rows[row].record as usize;
        let ts = &*e.ts;
        let cost = e.cost.unwrap_or(0.0);
        per[record].add(e, recent);
        fold.add(e);
        if ts >= fold.since[2].as_str() {
            week.total += cost;
            if is_orch[row] {
                week.orch += cost;
            }
            if e.idle {
                week.idle += cost;
            }
            week.tokens.add(&e.tokens);
        }
    }
    (per, week)
}

fn live_rows(
    records: &[&Record],
    index: &Table,
    per: &[Acc],
    quota: &QuotaView,
    now: DateTime<Utc>,
) -> Vec<LiveRow> {
    let ten_min_ago = clock::stamp(now - Duration::minutes(10));
    let mut live = Vec::new();
    for r in records {
        let a = record_acc(index, per, &r.record_id);
        let recent_event = a
            .and_then(|a| a.last)
            .is_some_and(|l| l >= ten_min_ago.as_str());
        let status = r.execution_status();
        if !shows_live(&status, recent_event) {
            continue;
        }
        let idle = r.task == IDLE_TASK || (status.is_live() && !recent_event);
        live.push(live_row(r, a, idle, quota.assess(&r.agent, &r.model)));
    }
    live.sort_by(|a, b| {
        (a.kind != KIND_ORCHESTRATOR, &a.project, &a.role).cmp(&(
            b.kind != KIND_ORCHESTRATOR,
            &b.project,
            &b.role,
        ))
    });
    live
}

fn record_acc<'a>(index: &Table, per: &'a [Acc], record_id: &str) -> Option<&'a Acc<'a>> {
    let id = index.names.get(record_id)?;
    Some(&per[id as usize]).filter(|a| a.events > 0)
}

fn shows_live(status: &ExecutionStatus, recent_event: bool) -> bool {
    let failed = matches!(
        status,
        ExecutionStatus::Failed { .. } | ExecutionStatus::LaunchFailed { .. }
    );
    !failed && (status.is_live() || recent_event)
}

fn live_row(r: &Record, a: Option<&Acc>, idle: bool, assessment: Assessment) -> LiveRow {
    let none = Acc::default();
    let a = a.unwrap_or(&none);
    LiveRow {
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
        tokens: a.tokens,
        cost_usd: a.cost,
        unpriced_events: a.unpriced,
        rate_tokens_per_min: a.recent_tokens as f64 / 5.0,
        rate_usd_per_hour: a.recent_cost * 12.0,
        last_event_at: a.last.map(str::to_string),
        idle,
        pool: assessment.pool,
        pool_state: assessment.state.as_str().to_string(),
    }
}

fn shown_pools(quota: &QuotaView) -> BTreeMap<String, quota::PoolReading> {
    let mut pools = quota.file.pools.clone();
    for (name, reading) in pools.iter_mut() {
        let a = quota.assess_shown(name);
        reading.state = a.state.as_str().to_string();
        reading.reason = Some(a.reason);
    }
    pools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{property, SplitMix64};

    fn now() -> DateTime<Utc> {
        clock::parse("2026-09-28T18:00:00Z").unwrap()
    }

    fn pick<'a>(rng: &mut SplitMix64, from: &[&'a str]) -> &'a str {
        from[rng.below(from.len() as u64) as usize]
    }

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

    #[test]
    fn nfr_02_a_same_length_ledger_rewrite_by_rename_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let home = tmp.path().join("home");
        let loc = Locations {
            home: home.clone(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join(".local/share/opencode/opencode.db"),
            sqlite3: PathBuf::from("sqlite3"),
        };
        let ledger = |role: &str| {
            let mut rows = serde_json::to_value(records()).unwrap();
            rows[0]["role"] = role.into();
            serde_json::to_vec(&rows).unwrap()
        };
        let path = state.join("-work.json");
        std::fs::write(&path, ledger("aaaa")).unwrap();
        let mut c = Collector::open_at(&state, loc, Probing::Never, now()).unwrap();
        let roles = |c: &mut Collector| -> Vec<String> {
            c.ledgers().into_iter().map(|r| r.role).collect()
        };
        assert!(roles(&mut c).contains(&"aaaa".to_string()));

        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let next = state.join("-work.json.tmp");
        std::fs::write(&next, ledger("bbbb")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&next)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        std::fs::rename(&next, &path).unwrap();
        let (old, new) = (ledger("aaaa").len(), std::fs::metadata(&path).unwrap());
        assert_eq!((new.len() as usize, new.modified().unwrap()), (old, mtime));
        let roles = roles(&mut c);
        assert!(roles.contains(&"bbbb".to_string()), "{roles:?}");
        assert!(!roles.contains(&"aaaa".to_string()), "{roles:?}");
    }

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

    #[test]
    fn nfr_02_indexed_fold_matches_the_event_sums() {
        let records = records();
        property(0x4e46_5230_3220_0001, 40, |rng| {
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

    fn collector_on(state: &Path) -> Collector {
        let home = state.join("home");
        let loc = Locations {
            home: home.clone(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join(".local/share/opencode/opencode.db"),
            sqlite3: PathBuf::from("sqlite3"),
        };
        let ledger = serde_json::to_vec(&records()).unwrap();
        std::fs::write(state.join("-work.json"), ledger).unwrap();
        let mut c = Collector::open_at(state, loc, Probing::Never, now()).unwrap();
        c.refresh_ledgers();
        c
    }

    fn rollups_from_files(state: &Path, project: &str) -> BTreeMap<String, Rollup> {
        let events: Vec<Event> = store::read_all(&super::super::dir(state))
            .into_iter()
            .filter(|e| e.project.as_deref() == Some(project))
            .collect();
        let done = done_record_ids(&read_ledgers(state));
        (WINDOWS.iter())
            .map(|w| {
                let since = window_start(w, now());
                (
                    w.to_string(),
                    store::rollup(&events, since.as_deref(), &done),
                )
            })
            .collect()
    }

    fn near(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        use serde_json::Value;
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() <= 1e-9
            }
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(x, y)| near(x, y))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| near(v, w)))
            }
            _ => a == b,
        }
    }

    #[test]
    fn project_rollups_match_the_rollups_from_the_event_files() {
        property(0x5435_6300_0000_0001, 40, |rng| {
            let tmp = tempfile::tempdir().unwrap();
            let mut c = collector_on(tmp.path());
            c.store.append(events(rng)).unwrap();
            let reopened = collector_on(tmp.path());
            for project in ["/a", "/b"] {
                let want = rollups_from_files(tmp.path(), project);
                assert_eq!(reopened.project_rollups(project, now()), want, "{project}");
                let got = serde_json::to_value(c.project_rollups(project, now())).unwrap();
                let want = serde_json::to_value(&want).unwrap();
                assert!(near(&got, &want), "{project}: {got:#}\n{want:#}");
            }
        });
    }

    #[test]
    fn project_rollups_of_a_project_with_no_events_are_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = collector_on(tmp.path());
        c.store.append(events(&mut SplitMix64::new(7))).unwrap();
        assert!(!c.store.is_empty());
        let got = c.project_rollups("/none", now());
        let windows: BTreeSet<&str> = got.keys().map(String::as_str).collect();
        assert_eq!(windows, BTreeSet::from(WINDOWS));
        assert!(got.values().all(|r| *r == Rollup::default()), "{got:?}");
        assert_eq!(got, rollups_from_files(tmp.path(), "/none"));
    }
}

#[cfg(test)]
mod trf_45_tests {
    use super::*;
    use crate::execution::records::{NewRecord, SessionLedger};

    #[test]
    fn trf_45_read_ledgers_and_the_collector_read_the_session_records() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let _fleetd =
            crate::execution::store::test_serve(&state.join(crate::execution::store::STATE_DB));
        let ledger = SessionLedger::for_project(&state, "/repo");
        ledger
            .add(NewRecord {
                record_id: "r-1",
                agent: "claude",
                tier: "opus",
                model: "opus",
                role: "opus-1",
                session_id: None,
                task: "t",
                phase: None,
            })
            .unwrap();
        ledger.done("r-1", "finished").unwrap();
        assert!(!ledger.path().exists(), "no JSON ledger");

        let records = read_ledgers(&state);
        assert_eq!(records.len(), 1);
        assert_eq!(done_record_ids(&records).len(), 1);

        let home = tmp.path().join("home");
        let loc = Locations {
            home: home.clone(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join(".local/share/opencode/opencode.db"),
            sqlite3: PathBuf::from("sqlite3"),
        };
        let mut c = Collector::open_at(&state, loc, Probing::Never, clock::now()).unwrap();
        let ids: Vec<String> = c.ledgers().into_iter().map(|r| r.record_id).collect();
        assert_eq!(ids, ["r-1"]);
    }
}
