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

use super::readers::{poll_record, Cursors};
use super::store::{self, GroupSums, Rollup, Store};
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
    records
        .iter()
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
    last_good: BTreeMap<PathBuf, Vec<Record>>,
    /// `store.events`, interned for the snapshot fold.
    index: EventIndex,
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
            index: EventIndex::default(),
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
        let mut out = Vec::new();
        for path in execution_store::ledger_paths(&self.state_root) {
            match execution_store::read_ledger_file(&path) {
                Some(records) => {
                    self.last_good.insert(path, records.clone());
                    out.extend(records);
                }
                None => {
                    if let Some(records) = self.last_good.get(&path) {
                        out.extend(records.clone());
                    }
                }
            }
        }
        out
    }

    /// Run one tick at `now` and write the snapshot.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Result<Snapshot> {
        let records = self.ledgers();
        let oldest = clock::stamp(now - Duration::days(self.policy.retention_days));
        let mut new_events = Vec::new();
        let mut signals: Vec<QuotaSignal> = Vec::new();
        let mut unread = Vec::new();
        for r in records
            .iter()
            .filter(|r| r.updated_at.as_str() >= oldest.as_str())
        {
            match poll_record(
                &self.loc,
                &r.agent,
                r.session_id.as_deref(),
                &mut self.cursors,
            ) {
                Ok(polled) => {
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
        store::save_cursors(self.store.dir(), &self.cursors)?;

        let quota = self.update_quota(now, &signals)?;
        self.index.extend(&self.store.events);
        let snapshot = build_indexed(
            &records,
            &self.store.events,
            &self.index,
            quota,
            unread,
            now,
            self.info.clone(),
        );
        let json = serde_json::to_vec_pretty(&snapshot)?;
        store::replace_private(&snapshot_path(&self.state_root), &json)?;
        Ok(snapshot)
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

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn share(part: f64, whole: f64) -> Option<f64> {
    (whole > 0.0).then(|| part / whole)
}

/// The stored events, interned for the snapshot fold (NFR-02). An event
/// becomes a `Row`: its record and its 6 group keys. A tick then folds
/// every event with no allocation and no string-keyed map.
#[derive(Debug, Default)]
pub struct EventIndex {
    /// The row of each indexed event, in store order.
    row_of: Vec<u32>,
    rows: Vec<Row>,
    row_ids: HashMap<Row, u32>,
    /// The record ids and group keys.
    names: Vec<String>,
    name_ids: HashMap<String, u32>,
}

/// A record and its keys in [`store::GROUPS`] order, as name ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Row {
    record: u32,
    groups: [u32; 6],
}

impl EventIndex {
    /// Index the events after the ones indexed before. The store only
    /// appends; a shorter slice is a new store, indexed from the start.
    pub fn extend(&mut self, events: &[Event]) {
        if self.row_of.len() > events.len() {
            *self = EventIndex::default();
        }
        for e in &events[self.row_of.len()..] {
            let row = Row {
                record: self.name(&e.record_id),
                groups: store::GROUPS.map(|g| self.name(store::group_key(e, g))),
            };
            let next = self.rows.len() as u32;
            let id = *self.row_ids.entry(row).or_insert(next);
            if id == next {
                self.rows.push(row);
            }
            self.row_of.push(id);
        }
    }

    fn name(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.name_ids.get(name) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.name_ids.insert(name.to_string(), id);
        id
    }
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
    let mut index = EventIndex::default();
    index.extend(events);
    build_indexed(records, events, &index, quota, unread, now, collector)
}

/// [`build_snapshot`] with `events` already in `index`. Each sum adds its
/// events in store order, as [`store::rollup`] does, so the costs are the
/// same to the last bit.
fn build_indexed(
    records: &[Record],
    events: &[Event],
    index: &EventIndex,
    quota: QuotaView,
    unread: Vec<Unread>,
    now: DateTime<Utc>,
    collector: CollectorInfo,
) -> Snapshot {
    debug_assert_eq!(index.row_of.len(), events.len());
    let done = done_record_ids(records);
    let names = index.names.len();
    let rows = index.rows.len();
    let is_done: Vec<bool> = index.names.iter().map(|n| done.contains(n)).collect();
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
    let mut unpriced_models: [BTreeMap<&str, store::UnpricedRow>; WINDOWS.len()] =
        Default::default();
    // Insights over the 7-day window.
    let (mut total, mut orch, mut idle) = (0.0f64, 0.0f64, 0.0f64);
    let mut week_tokens = TokenClasses::default();
    let is_orch: Vec<bool> = index
        .rows
        .iter()
        .map(|r| index.names[r.groups[5] as usize] == KIND_ORCHESTRATOR)
        .collect();

    for (e, &row) in events.iter().zip(&index.row_of) {
        let row = row as usize;
        let r = index.rows[row];
        let record = r.record as usize;
        let ts = e.ts.as_str();
        let cost = e.cost_usd.unwrap_or(0.0);
        let no_price = e.cost_usd.is_none();
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
                store::add_unpriced(&mut unpriced_models[w], e);
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
                        (index.names[key as usize].clone(), sums)
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
            .name_ids
            .get(&r.record_id)
            .map(|id| &per[*id as usize])
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

    fn snapshot(records: &[Record], events: &[Event], index: &EventIndex) -> Snapshot {
        let policy = Policy::default();
        let quota = QuotaView::new(QuotaFile::default(), now(), policy, false);
        let info = CollectorInfo::default();
        build_indexed(records, events, index, quota, Vec::new(), now(), info)
    }

    /// NFR-02: the indexed fold gives what the per-event sums gave, and an
    /// index extended tick by tick gives what a fresh one gives.
    #[test]
    fn nfr_02_indexed_fold_matches_the_event_sums() {
        let records = records();
        property(0x4e46_5230_3220_0001, 200, |rng| {
            let events = events(rng);
            let mut fresh = EventIndex::default();
            fresh.extend(&events);
            let snap = snapshot(&records, &events, &fresh);

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

            let mut grown = EventIndex::default();
            let cut = events.len() / 2;
            grown.extend(&events[..cut]);
            grown.extend(&events);
            assert_eq!(snapshot(&records, &events, &grown), snap);
        });
    }
}
