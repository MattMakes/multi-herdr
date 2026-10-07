//! `horch usage` - where the tokens went, from the telemetry event store
//! (SPC-05).
//!
//! With no live collector it first runs one collector tick itself, so the
//! store is current; with one, it reads the store as it is. Every ledger
//! under the state root is covered (TEL-09), and the totals agree with
//! `horch cost` class by class (TEL-10): both read through the same readers
//! and price with the same function. An event stored before its model had a
//! price is priced on read; an event whose model still has no price is
//! listed as unpriced with its tokens, never counted as $0.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use horch_core::clock;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, Probing, Snapshot};
use horch_core::telemetry::store::{self, RollupRow, UnpricedRow, GROUPS};
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
    /// The priced events' cost.
    pub cost_usd: f64,
    /// Models with no price. Their tokens are in `tokens`, not in `cost_usd`.
    pub unpriced_models: Vec<String>,
    pub unpriced_events: u64,
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
    /// The unpriced events, per model.
    pub unpriced: Vec<UnpricedRow>,
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
    /// Events not in `cost_usd`: their model has no price.
    pub unpriced_events: u64,
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
            unpriced_models: Vec::new(),
            unpriced_events: 0,
        });
        if !r.models.contains(&e.model) {
            r.models.push(e.model.clone());
        }
        r.events += 1;
        r.tokens.add(&e.tokens);
        total.tokens.add(&e.tokens);
        total.events += 1;
        match e.cost_usd {
            Some(c) => {
                r.cost_usd += c;
                total.cost_usd += c;
            }
            None => {
                if !r.unpriced_models.contains(&e.model) {
                    r.unpriced_models.push(e.model.clone());
                }
                r.unpriced_events += 1;
                total.unpriced_events += 1;
            }
        }
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
        unpriced: store::unpriced(events.iter()),
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
        "{:<28} {:>8} {:>8} {:>8} {:>8} {:>11} {:>6} {:>9}\n",
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
            "{:<28} {:>8} {:>8} {:>8} {:>8} {:>11} {:>6} {:>9}\n",
            super::telemetry::clip(&row.key, 28),
            k(row.tokens.fresh()),
            k(row.tokens.cache_read),
            k(row.tokens.output),
            k(row.tokens.total()),
            // `*`: some of the row's events have no price (listed below).
            format!(
                "{:.2}{}",
                row.cost_usd,
                if row.unpriced_events > 0 { "*" } else { " " }
            ),
            row.cache_hit
                .map(|h| format!("{:.0}%", h * 100.0))
                .unwrap_or_else(|| "-".into()),
            row.cost_per_done
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "-".into()),
        ));
    }
    out.push_str(&format!(
        "\nTotal: ${:.2} over {} record(s), {} event(s), {} tokens{}.\n",
        r.total.cost_usd,
        r.total.records,
        r.total.events,
        k(r.total.tokens.total()),
        if r.total.unpriced_events > 0 {
            format!(", plus {} unpriced event(s)", r.total.unpriced_events)
        } else {
            String::new()
        }
    ));
    if !r.unpriced.is_empty() {
        out.push_str("\n* Unpriced (no price for the model; never counted as $0):\n");
        for u in &r.unpriced {
            out.push_str(&format!(
                "  {}: {} event(s), {} tokens\n",
                u.model,
                u.events,
                k(u.tokens.total())
            ));
        }
    }
    if !r.unread.is_empty() {
        out.push_str("\nUnread (never counted as 0):\n");
        for u in &r.unread {
            out.push_str(&format!("  {} ({}): {}\n", u.role, u.agent, u.reason));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::usage;

    fn line(id: &str, at: &str, model: &str, input: u64, read: u64, out: u64) -> String {
        serde_json::json!({
            "type": "assistant", "sessionId": "sid-1", "timestamp": at,
            "uuid": format!("u-{id}"),
            "message": {
                "id": id, "type": "message", "role": "assistant", "model": model,
                "content": [],
                "usage": {
                    "input_tokens": input, "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": read, "output_tokens": out,
                },
            },
        })
        .to_string()
    }

    /// A state root with 1 ledger record and its Claude transcript: 2 calls
    /// on a priced model and 1 on a model with no price.
    fn fixture(root: &std::path::Path) -> (std::path::PathBuf, Locations) {
        let home = root.join("home");
        let state = root.join("state");
        let dir = home.join(".claude/projects/-p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let transcript = [
            line(
                "m1",
                "2026-09-28T17:00:00.000Z",
                "claude-sonnet-5-5",
                10,
                9000,
                300,
            ),
            line(
                "m2",
                "2026-09-28T17:01:00.000Z",
                "mystery-model-9",
                20,
                500,
                70,
            ),
            line(
                "m3",
                "2026-09-28T17:02:00.000Z",
                "claude-sonnet-5-5",
                5,
                9100,
                40,
            ),
        ];
        std::fs::write(dir.join("sid-1.jsonl"), transcript.join("\n") + "\n").unwrap();
        let ledger = serde_json::json!([{
            "record_id": "r1", "session_id": "sid-1", "agent": "claude", "tier": "sonnet",
            "model": "claude-sonnet-5-5", "role": "sonnet-1", "status": "working",
            "task": "t", "history": [], "project": "/p",
            "created_at": "2026-09-28T16:00:00Z", "updated_at": "2026-09-28T17:00:00Z"
        }]);
        std::fs::write(state.join("-p.json"), ledger.to_string()).unwrap();
        let loc = Locations::under_home(&home, &horch_core::runtime::Inherited::default());
        (state, loc)
    }

    fn args() -> UsageArgs {
        UsageArgs {
            json: true,
            since: None,
            project: None,
            by: "teammate".into(),
            window: None,
        }
    }

    /// TEL-10: `horch usage` and `horch cost` give the same cost per record
    /// on one fixture with an unpriced model. Neither counts the unpriced
    /// call as $0; both name its model. It holds again after the stored
    /// costs are erased, as an older collector left them: `horch usage`
    /// prices them on read.
    #[test]
    fn tel_10_usage_and_cost_agree_per_record_with_an_unpriced_model() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, loc) = fixture(tmp.path());
        let now = clock::parse("2026-09-28T18:00:00Z").unwrap();
        let snap = Collector::open_at(&state, loc.clone(), Probing::Never, now)
            .unwrap()
            .tick(now)
            .unwrap();
        let prices = usage::builtin_prices();
        let cost =
            super::super::cost::build(&collect::read_ledgers(&state), None, &loc, &prices, None);
        assert_eq!(cost.rows.len(), 1);
        let want = &cost.rows[0];
        assert_eq!(want.unpriced_models, vec!["mystery-model-9".to_string()]);
        assert!(want.cost > 0.0);

        let tel = horch_core::telemetry::dir(&state);
        for pass in ["as collected", "costs erased"] {
            let events = store::read_all(&tel);
            let report = build(
                &state,
                &args(),
                &events,
                snap.clone(),
                None,
                "t".into(),
                now,
            );
            let got = &report.records[0];
            assert_eq!(got.record_id, want.record_id, "{pass}");
            assert!(
                (got.cost_usd - want.cost).abs() < 1e-9,
                "{pass}: {} vs {}",
                got.cost_usd,
                want.cost
            );
            assert_eq!(got.unpriced_models, want.unpriced_models, "{pass}");
            assert_eq!(got.unpriced_events, 1, "{pass}");
            assert_eq!(got.tokens.priced(), want.tokens, "{pass}");
            assert_eq!(report.total.unpriced_events, 1, "{pass}");
            assert_eq!(report.unpriced.len(), 1, "{pass}");
            assert_eq!(report.unpriced[0].events, 1, "{pass}");
            assert_eq!(report.unpriced[0].tokens.output, 70, "{pass}");
            let text = render(&report);
            assert!(text.contains("plus 1 unpriced event(s)"), "{pass}: {text}");
            assert!(
                text.contains("mystery-model-9: 1 event(s)"),
                "{pass}: {text}"
            );

            // Erase every stored cost, as a collector without the price did.
            for entry in std::fs::read_dir(&tel).unwrap().flatten() {
                let p = entry.path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if !name.starts_with("events-") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                let erased: Vec<String> = text
                    .lines()
                    .map(|l| {
                        let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                        v["cost_usd"] = serde_json::Value::Null;
                        v.to_string()
                    })
                    .collect();
                std::fs::write(&p, erased.join("\n") + "\n").unwrap();
            }
        }
    }
}
