use std::collections::{BTreeMap, BTreeSet};

use horch_core::roster::Roster;
use serde::Serialize;

use super::{expected_skills, Row};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TableRow {
    pub key: String,
    pub sessions: u64,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
    pub mean_input: f64,
    pub mean_output: f64,
    pub mean_cache_read: f64,
    pub mean_cache_write: f64,
    pub mean_cost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reprice_cost: Option<f64>,
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

#[derive(Debug, Default, Serialize)]
pub struct Tables {
    pub tiers: Vec<TableRow>,
    pub orchestrator: Vec<TableRow>,
    pub teammates: Vec<TableRow>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillUse {
    pub teammate: String,
    pub sessions: u64,
    pub sessions_not_read: u64,
    pub expected: Vec<SkillCount>,
    pub other: Vec<SkillCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillCount {
    pub skill: String,
    pub sessions: u64,
}

pub fn skill_use(rows: &[Row], roster: Option<&Roster>) -> Vec<SkillUse> {
    let mut by_teammate: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by_teammate.entry(&r.teammate).or_default().push(r);
    }
    by_teammate
        .into_iter()
        .map(|(teammate, all)| {
            let rs: Vec<&Row> = all.iter().copied().filter(|r| r.skills_read).collect();
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
                sessions_not_read: (all.len() - rs.len()) as u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(teammate: &str, agent: &str, skills_read: bool, loaded: &[&str]) -> Row {
        Row {
            record_id: String::new(),
            teammate: teammate.into(),
            role: format!("{teammate}-1"),
            kind: "worker".into(),
            agent: agent.into(),
            model: String::new(),
            effort: None,
            family: "build".into(),
            calls: 1,
            tokens: Default::default(),
            cost: 0.0,
            reprice_cost: None,
            unpriced_models: Vec::new(),
            transcript: None,
            skills_read,
            skills_used: loaded.iter().map(|s| (s.to_string(), 1)).collect(),
            expected_unused: Vec::new(),
        }
    }

    #[test]
    fn sessions_whose_skill_loads_are_not_read_are_counted_apart() {
        let rows = [
            row("a", "claude", true, &["tdd"]),
            row("a", "opencode", false, &[]),
            row("b", "opencode", false, &[]),
        ];
        let use_ = skill_use(&rows, None);
        assert_eq!(use_.len(), 2);
        assert_eq!((use_[0].sessions, use_[0].sessions_not_read), (1, 1));
        assert_eq!(
            use_[0].other,
            [SkillCount {
                skill: "tdd".into(),
                sessions: 1
            }]
        );
        assert_eq!((use_[1].sessions, use_[1].sessions_not_read), (0, 1));
    }
}
