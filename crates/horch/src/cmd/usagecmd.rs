//! `horch usage` - where the tokens went, from the telemetry event store
//! (SPC-05).
//!
//! With no live collector it first runs one collector tick itself, so the
//! store is current; with one, it reads the store as it is. Every ledger
//! under the state root is covered (TEL-09), and the totals agree with
//! `horch cost` class by class (TEL-10): both read through the same readers.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use horch_core::clock;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, Probing, Snapshot};
use horch_core::telemetry::store::{self, RollupRow, GROUPS};
use horch_core::telemetry::{lock, Event, TokenClasses};
use horch_core::usage::Locations;
use serde::Serialize;

use crate::output;

pub struct UsageArgs {
    pub json: bool,
    pub since: Option<String>,
    pub project: Option<String>,
    pub by: String,
    pub window: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RecordRow {
    pub record_id: String,
    pub project: Option<String>,
    pub teammate: String,
    pub role: String,
    pub kind: String,
    pub agent: String,
    pub models: Vec<String>,
    pub events: u64,
    pub tokens: TokenClasses,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub generated_at: String,
    pub since: Option<String>,
    pub window: Option<String>,
    pub project: Option<String>,
    pub by: String,
    pub rows: Vec<RollupRow>,
    pub records: Vec<RecordRow>,
    pub total: RecordTotal,
    pub live: Vec<collect::LiveRow>,
    pub insights: collect::Insights,
    pub unread: Vec<horch_core::telemetry::Unread>,
    /// Whether this run read the store a live collector keeps, or ran its
    /// own catch-up tick.
    pub source: String,
}

#[derive(Debug, Default, Serialize)]
pub struct RecordTotal {
    pub tokens: TokenClasses,
    pub cost_usd: f64,
    pub records: u64,
    pub events: u64,
}

/// Run one collector tick unless a live collector does that already.
/// Returns the snapshot, and where it came from.
pub fn catch_up(ctx: &RuntimeContext) -> Result<(Snapshot, String)> {
    let root = ctx.paths.state_root.clone();
    let now = clock::now();
    if !lock::collector_live(&root) {
        let me = lock::this_process(&clock::stamp(now), ctx);
        if let Ok(Ok(held)) = lock::acquire(&root, &me) {
            let mut c = Collector::open_in(ctx, Locations::from_context(ctx), Probing::Never, now)?;
            let snap = c.tick(now)?;
            held.release();
            return Ok((snap, "catch-up tick".into()));
        }
    }
    let path = collect::snapshot_path(&root);
    let snap = Snapshot::read(&path).unwrap_or_default();
    Ok((snap, "live collector".into()))
}

pub fn usage(ctx: &RuntimeContext, args: UsageArgs) -> Result<()> {
    if !GROUPS.contains(&args.by.as_str()) {
        bail!("--by takes one of: {}", GROUPS.join(", "));
    }
    let now = clock::now();
    let since =
        match (&args.since, &args.window) {
            (Some(s), _) => Some(clock::parse(s).map(clock::stamp).ok_or_else(|| {
                anyhow::anyhow!("--since: '{s}' is not a date or an RFC 3339 time")
            })?),
            (None, Some(w)) => Some(
                collect::window_start(w, now)
                    .ok_or_else(|| anyhow::anyhow!("--window takes 5h, today or 7d"))?,
            ),
            (None, None) => None,
        };
    let (snap, source) = catch_up(ctx)?;
    let events: Vec<Event> = store::read_all(&horch_core::telemetry::dir(&ctx.paths.state_root))
        .into_iter()
        .filter(|e| since.as_deref().is_none_or(|s| e.ts.as_str() >= s))
        .filter(|e| {
            args.project
                .as_deref()
                .is_none_or(|p| e.project.as_deref() == Some(p))
        })
        .collect();
    let report = build(
        &ctx.paths.state_root,
        &args,
        &events,
        snap,
        since,
        source,
        now,
    );
    if args.json {
        output::println(&serde_json::to_string_pretty(&report)?);
    } else {
        output::print(&render(&report));
    }
    Ok(())
}

pub fn build(
    state_root: &std::path::Path,
    args: &UsageArgs,
    events: &[Event],
    snap: Snapshot,
    since: Option<String>,
    source: String,
    now: chrono::DateTime<chrono::Utc>,
) -> Report {
    let done = collect::done_record_ids(&collect::read_ledgers(state_root));
    let rows = store::rollup_rows(events.iter(), &args.by, &done);
    let mut per: BTreeMap<String, RecordRow> = BTreeMap::new();
    let mut total = RecordTotal::default();
    for e in events {
        let r = per.entry(e.record_id.clone()).or_insert_with(|| RecordRow {
            record_id: e.record_id.clone(),
            project: e.project.clone(),
            teammate: e.teammate.clone(),
            role: e.role.clone(),
            kind: e.kind.clone(),
            agent: e.agent.clone(),
            models: Vec::new(),
            events: 0,
            tokens: TokenClasses::default(),
            cost_usd: 0.0,
        });
        if !r.models.contains(&e.model) {
            r.models.push(e.model.clone());
        }
        r.events += 1;
        r.tokens.add(&e.tokens);
        r.cost_usd += e.cost_usd.unwrap_or(0.0);
        total.tokens.add(&e.tokens);
        total.cost_usd += e.cost_usd.unwrap_or(0.0);
        total.events += 1;
    }
    total.records = per.len() as u64;
    let mut records: Vec<RecordRow> = per.into_values().collect();
    records.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Report {
        generated_at: clock::stamp(now),
        since,
        window: args.window.clone(),
        project: args.project.clone(),
        by: args.by.clone(),
        rows,
        records,
        total,
        live: snap.live,
        insights: snap.insights,
        unread: snap.unread,
        source,
    }
}

fn k(n: u64) -> String {
    super::telemetry::human(n)
}

pub fn render(r: &Report) -> String {
    let mut out = format!(
        "Usage by {}{}{} ({}).\n\n",
        r.by,
        r.since
            .as_deref()
            .map(|s| format!(" since {s}"))
            .unwrap_or_default(),
        r.project
            .as_deref()
            .map(|p| format!(" in {p}"))
            .unwrap_or_default(),
        r.source
    );
    out.push_str(&format!(
        "{:<28} {:>8} {:>8} {:>8} {:>8} {:>10} {:>6} {:>9}\n",
        r.by.to_uppercase(),
        "fresh",
        "c.read",
        "out",
        "tokens",
        "$",
        "hit",
        "$/DONE"
    ));
    for row in &r.rows {
        out.push_str(&format!(
            "{:<28} {:>8} {:>8} {:>8} {:>8} {:>10.2} {:>6} {:>9}\n",
            super::telemetry::clip(&row.key, 28),
            k(row.tokens.fresh()),
            k(row.tokens.cache_read),
            k(row.tokens.output),
            k(row.tokens.total()),
            row.cost_usd,
            row.cache_hit
                .map(|h| format!("{:.0}%", h * 100.0))
                .unwrap_or_else(|| "-".into()),
            row.cost_per_done
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "-".into()),
        ));
    }
    out.push_str(&format!(
        "\nTotal: ${:.2} over {} record(s), {} event(s), {} tokens.\n",
        r.total.cost_usd,
        r.total.records,
        r.total.events,
        k(r.total.tokens.total())
    ));
    if !r.unread.is_empty() {
        out.push_str("\nUnread (never counted as 0):\n");
        for u in &r.unread {
            out.push_str(&format!("  {} ({}): {}\n", u.role, u.agent, u.reason));
        }
    }
    out
}
