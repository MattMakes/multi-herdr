//! `horch cost` - what a fleet run cost, per worker, read from transcripts.
//!
//! Every worker in the project's ledger is priced from its own harness's
//! transcript (see `horch_core::usage`). Sessions that cannot be priced - no
//! session id, no transcript, an unread harness, an unknown model - are listed
//! separately and never folded into a total as zero.
//!
//! The same pass audits skills: which skills each worker actually loaded,
//! and which of the skills its teammate file says it is expected to use it
//! never touched. That is the measurement behind skill reinforcement.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{bail, Result};
use horch_core::ledger::{Ledger, Record};
use horch_core::teammates::{Phase, Roster};
use horch_core::usage::{self, Locations, Missing, Price, Tokens};
use serde::Serialize;

use crate::output;

pub struct CostArgs {
    pub json: bool,
    pub reprice: Option<String>,
    pub pricing: Option<String>,
    /// Only records created at or after this UTC timestamp (ledger format,
    /// e.g. 2026-09-24T00:00:00Z; a date prefix like 2026-09-24 works too).
    pub since: Option<String>,
    /// Only these records (record or session ids).
    pub records: Vec<String>,
    /// Extra sessions outside the ledger, as `<agent>:<session-id>` - the
    /// orchestrator's own claude session, say.
    pub sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub record_id: String,
    pub teammate: String,
    pub role: String,
    pub agent: String,
    pub model: String,
    pub effort: Option<String>,
    pub family: String,
    pub calls: u64,
    pub tokens: Tokens,
    /// Dollars for the models the price table knows.
    pub cost: f64,
    /// What the same tokens would cost on `--reprice`'s model.
    pub reprice_cost: Option<f64>,
    /// Models in the transcript the price table does not know. Their tokens
    /// are counted but not priced.
    pub unpriced_models: Vec<String>,
    pub transcript: Option<PathBuf>,
    pub skills_used: BTreeMap<String, u64>,
    pub expected_unused: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NotPriced {
    pub record_id: String,
    pub teammate: String,
    pub agent: String,
    pub reason: Missing,
}

#[derive(Debug, Default, Serialize)]
pub struct Rollup {
    pub sessions: u64,
    pub calls: u64,
    pub tokens: Tokens,
    pub cost: f64,
    pub reprice_cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub prices_as_of: &'static str,
    pub reprice: Option<String>,
    pub rows: Vec<Row>,
    pub not_priced: Vec<NotPriced>,
    pub by_teammate: BTreeMap<String, Rollup>,
    pub by_harness: BTreeMap<String, Rollup>,
    pub by_family: BTreeMap<String, Rollup>,
    pub total: Rollup,
}

pub fn cost(args: CostArgs) -> Result<()> {
    let ledger = Ledger::open()?;
    let mut records = ledger.read()?;
    if let Some(since) = &args.since {
        records.retain(|r| r.created_at.as_str() >= since.as_str());
    }
    if !args.records.is_empty() {
        records.retain(|r| {
            args.records.iter().any(|k| {
                *k == r.record_id || r.session_id.as_deref() == Some(k.as_str())
            })
        });
    }
    for extra in &args.sessions {
        let Some((agent, id)) = extra.split_once(':') else {
            bail!("--session takes <agent>:<session-id>, e.g. claude:0f10e145-...");
        };
        records.push(Record {
            record_id: format!("extra:{id}"),
            session_id: Some(id.to_string()),
            agent: agent.to_string(),
            tier: "(outside ledger)".into(),
            model: String::new(),
            effort: None,
            phase: None,
            role: "extra".into(),
            status: String::new(),
            task: String::new(),
            history: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        });
    }
    let prices = usage::load_prices(args.pricing.as_deref().map(std::path::Path::new))?;
    let reprice = match &args.reprice {
        Some(model) => match usage::price_for(&prices, model) {
            Some(p) => Some((model.clone(), p)),
            None => bail!("--reprice: no price for '{model}' (known: {})", known(&prices)),
        },
        None => None,
    };
    let roster = Roster::load().ok();
    let report = build(
        &records,
        roster.as_ref(),
        &Locations::from_env(),
        &prices,
        reprice.as_ref().map(|(m, p)| (m.as_str(), p)),
    );
    if args.json {
        output::println(&serde_json::to_string_pretty(&report)?);
    } else {
        output::print(&render(&report, ledger.path().display().to_string().as_str()));
    }
    Ok(())
}

fn known(prices: &BTreeMap<String, Price>) -> String {
    prices.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// The role family a phase belongs to, as cezaar#40 groups spend.
fn family(phase: Option<Phase>, role: &str) -> String {
    match phase {
        _ if role == "extra" => "extra",
        Some(Phase::Research) => "research",
        Some(Phase::Plan) => "plan",
        Some(Phase::Implementation) => "build",
        Some(Phase::Validation) => "review",
        None => "other",
    }
    .to_string()
}

/// The skills a teammate is expected to use, as the transcripts name them:
/// bundled ones bare (`tdd`), plugin ones qualified (`code:review`).
fn expected_skills(roster: Option<&Roster>, teammate: &str) -> Vec<String> {
    let Some(t) = roster.and_then(|r| r.get(teammate)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = t.skills.clone();
    for (plugin, skills) in &t.plugin_skills {
        out.extend(skills.iter().map(|s| format!("{plugin}:{s}")));
    }
    out
}

pub fn build(
    records: &[Record],
    roster: Option<&Roster>,
    loc: &Locations,
    prices: &BTreeMap<String, Price>,
    reprice: Option<(&str, &Price)>,
) -> Report {
    let mut report = Report {
        prices_as_of: "2026-09-24",
        reprice: reprice.map(|(m, _)| m.to_string()),
        rows: Vec::new(),
        not_priced: Vec::new(),
        by_teammate: BTreeMap::new(),
        by_harness: BTreeMap::new(),
        by_family: BTreeMap::new(),
        total: Rollup::default(),
    };
    for r in records {
        let (path, used) = match usage::read_session(loc, &r.agent, r.session_id.as_deref()) {
            Ok(found) => found,
            Err(reason) => {
                report.not_priced.push(NotPriced {
                    record_id: r.record_id.clone(),
                    teammate: r.tier.clone(),
                    agent: r.agent.clone(),
                    reason,
                });
                continue;
            }
        };
        let mut cost = 0.0;
        let mut unpriced = Vec::new();
        for (model, tokens) in &used.by_model {
            // A transcript that never names its model is priced as the
            // model the ledger launched.
            let model = if model.is_empty() { &r.model } else { model };
            match usage::price_for(prices, model) {
                Some(p) => cost += p.cost(tokens),
                None => unpriced.push(model.clone()),
            }
        }
        let tokens = used.tokens();
        let skills_used: BTreeMap<String, u64> = used
            .skills
            .iter()
            .map(|(k, v)| (k.strip_prefix("horch:").unwrap_or(k).to_string(), *v))
            .collect();
        let expected_unused = expected_skills(roster, &r.tier)
            .into_iter()
            .filter(|s| !skills_used.contains_key(s))
            .collect();
        let models: BTreeSet<&str> = used
            .by_model
            .keys()
            .map(|m| if m.is_empty() { r.model.as_str() } else { m.as_str() })
            .collect();
        let row = Row {
            record_id: r.record_id.clone(),
            teammate: r.tier.clone(),
            role: r.role.clone(),
            agent: r.agent.clone(),
            model: models.into_iter().collect::<Vec<_>>().join(", "),
            effort: r.effort.clone(),
            family: family(r.phase, &r.role),
            calls: used.calls,
            tokens,
            cost,
            reprice_cost: reprice.map(|(_, p)| p.cost(&tokens)),
            unpriced_models: unpriced,
            transcript: Some(path),
            skills_used,
            expected_unused,
        };
        for (map, key) in [
            (&mut report.by_teammate, row.teammate.clone()),
            (&mut report.by_harness, row.agent.clone()),
            (&mut report.by_family, row.family.clone()),
        ] {
            add(map.entry(key).or_default(), &row);
        }
        add(&mut report.total, &row);
        report.rows.push(row);
    }
    report
}

fn add(roll: &mut Rollup, row: &Row) {
    roll.sessions += 1;
    roll.calls += row.calls;
    roll.tokens.add(&row.tokens);
    roll.cost += row.cost;
    if let Some(r) = row.reprice_cost {
        roll.reprice_cost = Some(roll.reprice_cost.unwrap_or(0.0) + r);
    }
}

fn money(x: f64) -> String {
    format!("${x:.2}")
}

fn k(n: u64) -> String {
    if n >= 10_000 {
        format!("{:.0}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

pub fn render(report: &Report, ledger: &str) -> String {
    let rep_head = report
        .reprice
        .as_ref()
        .map(|m| format!(" | on {m}"))
        .unwrap_or_default();
    let rep_sep = if report.reprice.is_some() { "|---:" } else { "" };
    let rep = |x: Option<f64>| x.map(|v| format!(" | {}", money(v))).unwrap_or_default();

    let mut out = format!(
        "# Fleet cost\n\nLedger: {ledger}\nPrices as of {} (USD per MTok; see ai_docs/reports/model-guide-2026-09.md).\n\n",
        report.prices_as_of
    );
    out.push_str("## Workers\n\n");
    out.push_str(&format!(
        "| teammate | role | agent | model | effort | calls | input | cache write | cache read | output | cost{rep_head} |\n\
         |---|---|---|---|---|---:|---:|---:|---:|---:|---:{rep_sep}|\n"
    ));
    for r in &report.rows {
        let t = &r.tokens;
        let flag = if r.unpriced_models.is_empty() { "" } else { " *" };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}{flag}{} |\n",
            r.teammate,
            r.role,
            r.agent,
            r.model,
            r.effort.as_deref().unwrap_or("-"),
            r.calls,
            k(t.input),
            k(t.cache_write_5m + t.cache_write_1h),
            k(t.cache_read),
            k(t.output),
            money(r.cost),
            rep(r.reprice_cost),
        ));
    }
    let unpriced: Vec<String> = report
        .rows
        .iter()
        .filter(|r| !r.unpriced_models.is_empty())
        .map(|r| format!("{} ({})", r.teammate, r.unpriced_models.join(", ")))
        .collect();
    if !unpriced.is_empty() {
        out.push_str(&format!(
            "\n\\* partly unpriced: no price for {}. Add them with --pricing.\n",
            unpriced.join("; ")
        ));
    }

    for (title, map) in [
        ("By family", &report.by_family),
        ("By harness", &report.by_harness),
        ("By teammate", &report.by_teammate),
    ] {
        out.push_str(&format!(
            "\n## {title}\n\n| | sessions | calls | tokens | cache read share | cost{rep_head} |\n|---|---:|---:|---:|---:|---:{rep_sep}|\n"
        ));
        for (key, roll) in map {
            out.push_str(&format!(
                "| {key} | {} | {} | {} | {} | {}{} |\n",
                roll.sessions,
                roll.calls,
                k(roll.tokens.total()),
                share(&roll.tokens),
                money(roll.cost),
                rep(roll.reprice_cost),
            ));
        }
    }
    out.push_str(&format!(
        "\n**Total: {}**{} across {} priced session(s).\n",
        money(report.total.cost),
        report
            .reprice
            .as_ref()
            .zip(report.total.reprice_cost)
            .map(|(m, v)| format!(" ({} on {m})", money(v)))
            .unwrap_or_default(),
        report.total.sessions
    ));

    if !report.not_priced.is_empty() {
        out.push_str("\n## Not priced\n\n");
        for n in &report.not_priced {
            let why = match n.reason {
                Missing::NoSessionId => "the ledger never learned its session id",
                Missing::NoTranscript => "no transcript found for its session id",
                Missing::NotRead => "this harness's usage is not read (see the model guide)",
            };
            out.push_str(&format!("- {} ({}, {}): {why}\n", n.teammate, n.agent, n.record_id));
        }
    }

    out.push_str("\n## Skills\n\n| teammate | role | loaded | expected but not loaded |\n|---|---|---|---|\n");
    for r in &report.rows {
        let used: Vec<String> = r
            .skills_used
            .iter()
            .map(|(s, n)| if *n > 1 { format!("{s} x{n}") } else { s.clone() })
            .collect();
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.teammate,
            r.role,
            if used.is_empty() { "-".into() } else { used.join(", ") },
            if r.expected_unused.is_empty() { "-".into() } else { r.expected_unused.join(", ") },
        ));
    }
    out
}

/// Cache reads as a share of all tokens: cezaar#40 found them at 60-70%, and
/// they are what a cheaper cache-read price actually saves.
fn share(t: &Tokens) -> String {
    let total = t.total();
    if total == 0 {
        return "-".into();
    }
    format!("{:.0}%", t.cache_read as f64 * 100.0 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, agent: &str, tier: &str, sid: Option<&str>, phase: Option<Phase>) -> Record {
        Record {
            record_id: id.into(),
            session_id: sid.map(str::to_owned),
            agent: agent.into(),
            tier: tier.into(),
            model: String::new(),
            effort: Some("medium".into()),
            phase,
            role: format!("{tier}-1"),
            status: "done".into(),
            task: String::new(),
            history: Vec::new(),
            created_at: "2026-09-24T00:00:00Z".into(),
            updated_at: String::new(),
        }
    }

    /// Priced rows roll up per family and harness; everything that cannot be
    /// priced is listed with its reason, and never counted as $0.
    #[test]
    fn a_run_is_priced_per_worker_and_the_rest_is_listed_not_zeroed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../horch-core/tests/fixtures/usage");
        let claude = home.join(".claude/projects/-proj");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::copy(fixtures.join("claude.jsonl"), claude.join("sid-c.jsonl")).unwrap();
        let codex = home.join(".codex/sessions/2026/09/24");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::copy(
            fixtures.join("codex.jsonl"),
            codex.join("rollout-2026-09-24T00-00-00-sid-x.jsonl"),
        )
        .unwrap();
        let loc = Locations {
            home: home.to_path_buf(),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
        };
        let records = vec![
            record("r1", "claude", "backend-developer", Some("sid-c"), Some(Phase::Implementation)),
            record("r2", "codex", "codex-reviewer", Some("sid-x"), Some(Phase::Validation)),
            record("r3", "opencode", "opencode-pickle", Some("ses_1"), Some(Phase::Implementation)),
            record("r4", "codex", "codex-sol", None, Some(Phase::Implementation)),
        ];
        let roster = Roster::builtin().unwrap();
        let prices = usage::builtin_prices();
        let opus = prices["claude-opus-5-5"];
        let report = build(&records, Some(&roster), &loc, &prices, Some(("claude-opus-5-5", &opus)));

        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.not_priced.len(), 2);
        assert_eq!(report.not_priced[0].reason, Missing::NotRead);
        assert_eq!(report.not_priced[1].reason, Missing::NoSessionId);
        assert_eq!(report.by_family["build"].sessions, 1);
        assert_eq!(report.by_family["review"].sessions, 1);
        assert!(report.total.cost > 0.0);
        let sum: f64 = report.rows.iter().map(|r| r.cost).sum();
        assert!((report.total.cost - sum).abs() < 1e-12);
        assert!(report.total.reprice_cost.is_some());

        // backend-developer is expected to use tdd and security-review; the
        // transcript loaded tdd only.
        let backend = &report.rows[0];
        assert_eq!(backend.skills_used.get("tdd"), Some(&1));
        assert_eq!(backend.expected_unused, vec!["security-review".to_string()]);
        // codex-reviewer loaded code-review and never security-review.
        let reviewer = &report.rows[1];
        assert_eq!(reviewer.expected_unused, vec!["security-review".to_string()]);

        let md = render(&report, "ledger.json");
        assert!(md.contains("| backend-developer | backend-developer-1 | claude | claude-opus-5-5 | medium |"), "{md}");
        assert!(md.contains("## Not priced"), "{md}");
        assert!(md.contains("opencode-pickle (opencode, r3): this harness's usage is not read"), "{md}");
        assert!(md.contains("| on claude-opus-5-5 |"), "{md}");
    }
}
