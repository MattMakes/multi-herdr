//! The report's tables, as row objects: per tier (workers only), the
//! orchestrator, per teammate, and per-teammate expected-skill use. The
//! columns are those of `ai_docs/reports/wave2/baseline.md`. They describe
//! spend; nothing here ranks or recommends.

use std::collections::{BTreeMap, BTreeSet};

use horch_core::roster::Roster;
use serde::Serialize;

use super::{expected_skills, Row};

/// One table line: sums over its sessions and means per session.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TableRow {
    /// The tier (`<harness> <model> <effort>`), the teammate, or `all`.
    pub key: String,
    pub sessions: u64,
    pub calls: u64,
    /// Uncached input.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    /// 5-minute and 1-hour cache writes together.
    pub cache_write: u64,
    pub cost: f64,
    pub mean_input: f64,
    pub mean_output: f64,
    pub mean_cache_read: f64,
    pub mean_cache_write: f64,
    pub mean_cost: f64,
    /// The same tokens at `--reprice`'s model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reprice_cost: Option<f64>,
    /// Sessions in this line whose transcript names a model with no price.
    /// Their priced models are in `cost`; the rest is not.
    pub partly_priced_sessions: u64,
}

impl TableRow {
    fn of<'a>(key: &str, rows: impl IntoIterator<Item = &'a Row>) -> Self {
        let mut t = TableRow {
            key: key.to_string(),
            ..TableRow::default()
        };
        for r in rows {
            t.sessions += 1;
            t.calls += r.calls;
            t.input += r.tokens.input;
            t.output += r.tokens.output;
            t.cache_read += r.tokens.cache_read;
            t.cache_write += r.tokens.cache_write_5m + r.tokens.cache_write_1h;
            t.cost += r.cost;
            if let Some(c) = r.reprice_cost {
                t.reprice_cost = Some(t.reprice_cost.unwrap_or(0.0) + c);
            }
            if !r.unpriced_models.is_empty() {
                t.partly_priced_sessions += 1;
            }
        }
        if t.sessions > 0 {
            let n = t.sessions as f64;
            t.mean_input = t.input as f64 / n;
            t.mean_output = t.output as f64 / n;
            t.mean_cache_read = t.cache_read as f64 / n;
            t.mean_cache_write = t.cache_write as f64 / n;
            t.mean_cost = t.cost / n;
        }
        t
    }
}

/// The 3 tables. `tiers` and `orchestrator` end with an `all` line when
/// they have any line.
#[derive(Debug, Default, Serialize)]
pub struct Tables {
    /// Workers only: one line per `<harness> <model> <effort>`.
    pub tiers: Vec<TableRow>,
    /// Orchestrator sessions, by the same tier key.
    pub orchestrator: Vec<TableRow>,
    /// One line per teammate, orchestrator and outside-ledger sessions
    /// included.
    pub teammates: Vec<TableRow>,
}

/// A session counts as the orchestrator by its ledger kind.
fn is_orchestrator(r: &Row) -> bool {
    r.kind == "orchestrator"
}

fn tier(r: &Row) -> String {
    format!(
        "{} {} {}",
        r.agent,
        if r.model.is_empty() { "-" } else { &r.model },
        r.effort.as_deref().unwrap_or("-")
    )
}

fn grouped<'a>(
    rows: impl Iterator<Item = &'a Row>,
    key: impl Fn(&Row) -> String,
    with_all: bool,
) -> Vec<TableRow> {
    let rows: Vec<&Row> = rows.collect();
    let mut groups: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        groups.entry(key(r)).or_default().push(r);
    }
    let mut out: Vec<TableRow> = groups
        .iter()
        .map(|(k, rs)| TableRow::of(k, rs.iter().copied()))
        .collect();
    if with_all && !rows.is_empty() {
        out.push(TableRow::of("all", rows.iter().copied()));
    }
    out
}

impl Tables {
    pub fn of(rows: &[Row]) -> Self {
        Tables {
            tiers: grouped(
                rows.iter()
                    .filter(|r| !is_orchestrator(r) && r.role != "extra"),
                tier,
                true,
            ),
            orchestrator: grouped(rows.iter().filter(|r| is_orchestrator(r)), tier, true),
            teammates: grouped(rows.iter(), |r| r.teammate.clone(), false),
        }
    }
}

/// One teammate's skill use in the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillUse {
    pub teammate: String,
    pub sessions: u64,
    /// Each skill its teammate file expects, with the number of sessions
    /// that loaded it.
    pub expected: Vec<SkillCount>,
    /// Skills loaded that the teammate file does not expect.
    pub other: Vec<SkillCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillCount {
    pub skill: String,
    /// Sessions that loaded it at least once.
    pub sessions: u64,
}

pub fn skill_use(rows: &[Row], roster: Option<&Roster>) -> Vec<SkillUse> {
    let mut by_teammate: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by_teammate.entry(&r.teammate).or_default().push(r);
    }
    by_teammate
        .into_iter()
        .map(|(teammate, rs)| {
            let expected = expected_skills(roster, teammate);
            let count = |skill: &str| {
                rs.iter()
                    .filter(|r| r.skills_used.contains_key(skill))
                    .count() as u64
            };
            let others: BTreeSet<&String> = rs
                .iter()
                .flat_map(|r| r.skills_used.keys())
                .filter(|s| !expected.contains(s))
                .collect();
            SkillUse {
                teammate: teammate.to_string(),
                sessions: rs.len() as u64,
                expected: expected
                    .iter()
                    .map(|s| SkillCount {
                        skill: s.clone(),
                        sessions: count(s),
                    })
                    .collect(),
                other: others
                    .into_iter()
                    .map(|s| SkillCount {
                        skill: s.clone(),
                        sessions: count(s),
                    })
                    .collect(),
            }
        })
        .collect()
}
