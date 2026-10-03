//! One collector tick (section 8.3), and the snapshot it writes (section 9).
//!
//! 1. Read every ledger under the state root (a ledger that does not parse
//!    keeps its last good copy: a write may be in flight).
//! 2. Keep the records updated inside the retention window.
//! 3. Poll each record's reader.
//! 4. Append the events, sync, then save the cursors.
//! 5. Fold limit signals into the quota readings; probe when due.
//! 6. Write `snapshot.json` (temp file + rename).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::readers::{poll_record, Cursors};
use super::store::{self, Rollup, Store};
use super::{Event, Observation, QuotaSignal, TokenClasses, Unread};
use crate::clock;
use crate::ledger::{Record, KIND_ORCHESTRATOR, STATUS_DONE, STATUS_WORKING};
use crate::policy::Policy;
use crate::quota::{self, ProbeBins, QuotaEnv, QuotaFile, QuotaView};
use crate::usage::{self, Locations, Price};

/// Every record in every ledger under `state_root`, skipping a ledger that
/// does not parse right now. For readers outside the collector.
pub fn read_ledgers(state_root: &Path) -> Vec<Record> {
    let Ok(entries) = std::fs::read_dir(state_root) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|e| e.to_str()) == Some("json")
                && p.file_name().and_then(|n| n.to_str()) != Some("policy.json")
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| serde_json::from_str::<Vec<Record>>(&t).ok())
        .flatten()
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
    pub cost_usd: f64,
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
    pub fn open_at(
        state_root: &Path,
        loc: Locations,
        probing: Probing,
        now: DateTime<Utc>,
    ) -> Result<Collector> {
        // A2: from RuntimeContext
        let balance = std::env::var("HORCH_BALANCE").ok();
        let policy = Policy::load(state_root, balance.as_deref())?;
        let dir = super::dir(state_root);
        let store = Store::open(&dir, now, policy.retention_days)?;
        let cursors = store::load_cursors(&dir);
        Ok(Collector {
            state_root: state_root.to_path_buf(),
            bins: ProbeBins {
                codex_sessions: loc.codex_sessions.clone(),
                ..ProbeBins::from_env()
            },
            quota_env: quota_env_from_process(),
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
            fail_after_append: std::env::var("HORCH_FAULT").ok().as_deref() == Some("after-append"),
            abort_after_append: std::env::var("HORCH_FAULT").ok().as_deref()
                == Some("abort-after-append"),
        })
    }

    /// Every ledger's records, keeping the last good copy of one that does
    /// not parse right now.
    pub fn ledgers(&mut self) -> Vec<Record> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.state_root) else {
            return out;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|e| e.to_str()) == Some("json")
                    && p.file_name().and_then(|n| n.to_str()) != Some("policy.json")
            })
            .collect();
        paths.sort();
        for path in paths {
            let parsed = std::fs::read_to_string(&path).ok().and_then(|t| {
                if t.trim().is_empty() {
                    Some(Vec::new())
                } else {
                    serde_json::from_str::<Vec<Record>>(&t).ok()
                }
            });
            match parsed {
                Some(records) => {
                    self.last_good.insert(path.clone(), records.clone());
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
        let snapshot = build_snapshot(
            &records,
            &self.store.events,
            quota,
            unread,
            now,
            &self.policy,
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
            quota::probe_all(
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

/// `HORCH_QUOTA_FILE`, `HORCH_PROBE_TIMEOUT_MS` and the temp dir.
// A2: from RuntimeContext
fn quota_env_from_process() -> QuotaEnv {
    QuotaEnv {
        quota_file: std::env::var_os("HORCH_QUOTA_FILE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
        probe_timeout: std::env::var("HORCH_PROBE_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(std::time::Duration::from_millis),
        temp_root: std::env::temp_dir(),
    }
}

/// A reader's usage, stamped with the record it belongs to and priced.
pub fn event_for(r: &Record, u: super::RawUsage, prices: &BTreeMap<String, Price>) -> Event {
    let model = if u.model.is_empty() {
        r.model.clone()
    } else {
        u.model.clone()
    };
    let cost_usd = usage::price_for(prices, &model).map(|p| p.cost(&u.tokens.priced()));
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
    let done: BTreeSet<String> = records
        .iter()
        .filter(|r| r.status == STATUS_DONE)
        .map(|r| r.record_id.clone())
        .collect();
    let mut rollups = BTreeMap::new();
    for w in WINDOWS {
        let since = window_start(w, now);
        rollups.insert(
            w.to_string(),
            store::rollup(events, since.as_deref(), &done),
        );
    }

    // Per record: totals, last event, and the last 5 minutes for the rate.
    #[derive(Default)]
    struct Acc {
        tokens: TokenClasses,
        cost: f64,
        last: Option<String>,
        recent_tokens: u64,
        recent_cost: f64,
    }
    let recent = clock::stamp(now - Duration::minutes(5));
    let mut per: BTreeMap<&str, Acc> = BTreeMap::new();
    for e in events {
        let a = per.entry(e.record_id.as_str()).or_default();
        a.tokens.add(&e.tokens);
        a.cost += e.cost_usd.unwrap_or(0.0);
        if a.last.as_deref().is_none_or(|l| e.ts.as_str() > l) {
            a.last = Some(e.ts.clone());
        }
        if e.ts.as_str() >= recent.as_str() {
            a.recent_tokens += e.tokens.total();
            a.recent_cost += e.cost_usd.unwrap_or(0.0);
        }
    }
    let ten_min_ago = clock::stamp(now - Duration::minutes(10));
    let mut live = Vec::new();
    for r in records {
        let a = per.get(r.record_id.as_str());
        let last = a.and_then(|a| a.last.clone());
        let recent_event = last.as_deref().is_some_and(|l| l >= ten_min_ago.as_str());
        if r.status != STATUS_WORKING && !recent_event {
            continue;
        }
        let idle = r.task == IDLE_TASK || (r.status == STATUS_WORKING && !recent_event);
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

    // Insights over the 7-day window.
    let since = window_start("7d", now).unwrap_or_default();
    let week: Vec<&Event> = events
        .iter()
        .filter(|e| e.ts.as_str() >= since.as_str())
        .collect();
    let total: f64 = week.iter().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
    let orch: f64 = week
        .iter()
        .filter(|e| e.kind == KIND_ORCHESTRATOR)
        .map(|e| e.cost_usd.unwrap_or(0.0))
        .sum();
    let idle: f64 = week
        .iter()
        .filter(|e| e.idle)
        .map(|e| e.cost_usd.unwrap_or(0.0))
        .sum();
    let mut tokens = TokenClasses::default();
    for e in &week {
        tokens.add(&e.tokens);
    }
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
            cache_hit: tokens.cache_hit(),
            idle_spend_share: share(idle, total),
            top_plan,
        },
        unread,
        projects,
    }
}
