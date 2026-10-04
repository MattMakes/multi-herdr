//! The agent inventory behind `horch agent-list`: every harness horch can
//! drive, whether its binary is there, and who uses it.
//!
//! A pure function of the roster, what the caller found out about each
//! binary, and the cached quota view. The caller gathers the facts (a path
//! lookup, a `--version` probe); nothing here touches the machine.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::harness::HarnessKind;
use crate::roster::Teammate;
use crate::routing::quota::{pool_for, QuotaView, State};

/// What the caller found out about one harness's binary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinaryFacts {
    /// Where the binary is, when it was found.
    pub path: Option<PathBuf>,
    /// The first line of `--version`. `None` when not probed or no answer.
    pub version: Option<String>,
}

/// A teammate that uses a model, with its effort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeammateUse {
    pub name: String,
    /// `None`: the teammate sets no effort, so the harness default applies.
    pub effort: Option<String>,
}

/// One model the roster runs on a harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelUse {
    /// `None`: the teammate sets no model.
    pub model: Option<String>,
    pub teammates: Vec<TeammateUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilitySummary {
    pub resume: bool,
    pub headless: bool,
    /// How the CLI finds skills (`plugin-dir`, `skill-flag`, `none`, ...).
    pub skills: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentRow {
    pub agent: String,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub efforts: Vec<String>,
    pub capabilities: CapabilitySummary,
    pub models: Vec<ModelUse>,
    /// The usage pools the models draw from, sorted.
    pub pools: Vec<String>,
    /// The worst state of those pools in the cached view; `unknown` with
    /// no view.
    pub pool_state: String,
    /// The binary is found and the pool is not exhausted.
    pub available: bool,
}

/// One row per real harness (never `None`), in [`HarnessKind::ALL`] order.
pub fn inventory(
    teammates: &[&Teammate],
    facts: &BTreeMap<&'static str, BinaryFacts>,
    view: Option<&QuotaView>,
) -> Vec<AgentRow> {
    HarnessKind::ALL
        .iter()
        .copied()
        .filter(|k| *k != HarnessKind::None)
        .map(|kind| row(kind, teammates, facts.get(kind.as_str()), view))
        .collect()
}

fn row(
    kind: HarnessKind,
    teammates: &[&Teammate],
    facts: Option<&BinaryFacts>,
    view: Option<&QuotaView>,
) -> AgentRow {
    let caps = kind.capabilities();
    let mut by_model: BTreeMap<Option<String>, Vec<TeammateUse>> = BTreeMap::new();
    for t in teammates.iter().filter(|t| t.agent == kind) {
        by_model
            .entry(t.model.clone())
            .or_default()
            .push(TeammateUse {
                name: t.name.clone(),
                effort: t.effort.clone(),
            });
    }
    let models: Vec<ModelUse> = by_model
        .into_iter()
        .map(|(model, mut teammates)| {
            teammates.sort_by(|a, b| a.name.cmp(&b.name));
            ModelUse { model, teammates }
        })
        .collect();

    // A harness the roster does not use is judged by its default pool.
    let model_names: Vec<&str> = if models.is_empty() {
        vec![""]
    } else {
        models
            .iter()
            .map(|m| m.model.as_deref().unwrap_or(""))
            .collect()
    };
    let mut pools: Vec<String> = model_names
        .iter()
        .map(|m| pool_for(kind.as_str(), m).to_string())
        .collect();
    pools.sort();
    pools.dedup();
    let state = view.map_or(State::Unknown, |v| {
        model_names
            .iter()
            .map(|m| v.assess(kind.as_str(), m).state)
            .min()
            .unwrap_or(State::Unknown)
    });

    let found = facts.is_some_and(|f| f.path.is_some());
    AgentRow {
        agent: kind.as_str().to_string(),
        found,
        path: facts
            .and_then(|f| f.path.as_ref())
            .map(|p| p.display().to_string()),
        version: facts.and_then(|f| f.version.clone()),
        efforts: caps.effort.iter().map(|e| e.to_string()).collect(),
        capabilities: CapabilitySummary {
            resume: caps.resumes,
            headless: caps.headless,
            skills: caps.skill_exposure.as_str().to_string(),
        },
        models,
        pools,
        pool_state: state.as_str().to_string(),
        available: found && state != State::Exhausted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::Roster;
    use crate::routing::policy::Policy;
    use crate::routing::quota::{QuotaFile, Window};

    fn teammate(
        base: &Teammate,
        name: &str,
        agent: HarnessKind,
        model: &str,
        effort: &str,
    ) -> Teammate {
        let mut t = base.clone();
        t.name = name.to_string();
        t.agent = agent;
        t.model = Some(model.to_string());
        t.effort = Some(effort.to_string());
        t
    }

    fn found(path: &str, version: Option<&str>) -> BinaryFacts {
        BinaryFacts {
            path: Some(PathBuf::from(path)),
            version: version.map(String::from),
        }
    }

    /// A view whose codex pool has one 7d window, `used` full.
    fn view_with_codex(used: f64) -> QuotaView {
        let mut file = QuotaFile::default();
        file.pools.insert(
            "codex".into(),
            crate::routing::quota::PoolReading {
                observed_at: Some("2026-09-28T17:55:00Z".into()),
                windows: vec![Window::new(
                    10_080,
                    used,
                    Some("2026-09-29T18:00:00Z".into()),
                    None,
                )],
                ..Default::default()
            },
        );
        QuotaView::new(
            file,
            crate::clock::parse("2026-09-28T18:00:00Z").unwrap(),
            Policy::default(),
            true,
        )
    }

    #[test]
    fn agent_list_inventory_covers_every_real_kind() {
        let rows = inventory(&[], &BTreeMap::new(), None);
        let want: Vec<&str> = HarnessKind::ALL
            .iter()
            .filter(|k| **k != HarnessKind::None)
            .map(|k| k.as_str())
            .collect();
        let got: Vec<&str> = rows.iter().map(|r| r.agent.as_str()).collect();
        assert_eq!(got, want);
        assert!(rows.iter().all(|r| !r.found && !r.available));
    }

    #[test]
    fn agent_list_inventory_table() {
        let roster = Roster::builtin().unwrap();
        let base = roster.offered()[0].clone();
        let team = [
            teammate(&base, "a-claude", HarnessKind::Claude, "opus", "high"),
            teammate(&base, "b-claude", HarnessKind::Claude, "opus", "low"),
            teammate(&base, "c-claude", HarnessKind::Claude, "sonnet", "medium"),
            teammate(&base, "d-codex", HarnessKind::Codex, "gpt-5.6-sol", "xhigh"),
        ];
        let refs: Vec<&Teammate> = team.iter().collect();
        let mut facts = BTreeMap::new();
        facts.insert("claude", found("/bin/claude", Some("2.1.0")));
        facts.insert("codex", found("/bin/codex", None));

        // (state of the codex pool, codex available)
        for (used, state, available) in [(0.1, "ok", true), (1.0, "exhausted", false)] {
            let view = view_with_codex(used);
            let rows = inventory(&refs, &facts, Some(&view));
            let claude = rows.iter().find(|r| r.agent == "claude").unwrap();
            assert!(claude.found && claude.available);
            assert_eq!(claude.path.as_deref(), Some("/bin/claude"));
            assert_eq!(claude.version.as_deref(), Some("2.1.0"));
            assert_eq!(claude.efforts, ["low", "medium", "high", "xhigh", "max"]);
            assert_eq!(claude.pools, ["claude"]);
            // (model, [(teammate, effort)])
            type ModelRow<'a> = (Option<&'a str>, Vec<(&'a str, Option<&'a str>)>);
            let models: Vec<ModelRow> = claude
                .models
                .iter()
                .map(|m| {
                    (
                        m.model.as_deref(),
                        m.teammates
                            .iter()
                            .map(|t| (t.name.as_str(), t.effort.as_deref()))
                            .collect(),
                    )
                })
                .collect();
            assert_eq!(
                models,
                [
                    (
                        Some("opus"),
                        vec![("a-claude", Some("high")), ("b-claude", Some("low"))]
                    ),
                    (Some("sonnet"), vec![("c-claude", Some("medium"))]),
                ]
            );
            let codex = rows.iter().find(|r| r.agent == "codex").unwrap();
            assert!(codex.found);
            assert_eq!(codex.version, None);
            assert_eq!(codex.pool_state, state);
            assert_eq!(codex.available, available, "codex pool {state}");
        }
    }

    #[test]
    fn agent_list_inventory_missing_binary_is_not_available() {
        let facts = BTreeMap::from([("pi", BinaryFacts::default())]);
        let rows = inventory(&[], &facts, None);
        let pi = rows.iter().find(|r| r.agent == "pi").unwrap();
        assert!(!pi.found && !pi.available);
        assert_eq!(pi.pool_state, "unknown");
        assert!(pi.models.is_empty());
    }
}
