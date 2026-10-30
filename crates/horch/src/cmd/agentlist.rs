//! `horch agent-list` - every agent harness horch can drive, whether its
//! binary is there, and who uses it.
//!
//! The facts are gathered here (a path lookup, a `--version` probe, the cached
//! quota view); `horch_core::harness::inventory` turns them into rows. Nothing
//! here starts a model turn or refreshes the quota.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use horch_core::clock;
use horch_core::harness::inventory::{inventory, AgentRow, BinaryFacts, ModelUse};
use horch_core::harness::HarnessKind;
use horch_core::routing::quota::QuotaView;
use horch_core::routing::quota_probe::{probe_version, VersionProbe};
use horch_core::routing::snapshot;
use horch_core::runtime::{process, RuntimeContext};

use crate::output;

pub fn agent_list(ctx: &RuntimeContext, json: bool, no_probe: bool) -> Result<()> {
    let roster = super::load_roster(ctx, None)?;
    let teammates: Vec<_> = roster
        .names()
        .iter()
        .filter_map(|n| roster.get(n))
        .collect();
    let facts = gather(ctx, !no_probe);
    let rows = inventory(&teammates, &facts, cached_view(ctx).as_ref());
    if json {
        output::println(&serde_json::to_string_pretty(&rows)?);
    } else {
        output::print(&table(&rows, no_probe));
    }
    Ok(())
}

/// The quota as last written, never refreshed. `None` when it cannot be read.
/// A live `--version` answer from [`gather`] wins over its cached harness
/// error (`inventory`, U-67).
fn cached_view(ctx: &RuntimeContext) -> Option<QuotaView> {
    let root = ctx.paths.state_root.clone();
    let policy = super::quotacmd::load_policy(ctx, &root).ok()?;
    snapshot::obtain(
        &root,
        clock::now(),
        &policy,
        false,
        &super::quotacmd::quota_env(ctx),
    )
    .ok()
}

/// Where each harness binary is, and (when `probe`) its `--version`. The
/// probes run in parallel, each with the time limit of `probe_version`.
pub(crate) fn gather(ctx: &RuntimeContext, probe: bool) -> BTreeMap<&'static str, BinaryFacts> {
    let paths: Vec<(&'static str, Option<PathBuf>)> = HarnessKind::ALL
        .iter()
        .filter_map(|k| {
            let bin = k.binary(&ctx.bins.harness)?;
            Some((k.as_str(), process::resolve(ctx, &bin)))
        })
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = paths
            .into_iter()
            .map(|(name, path)| {
                let probing = probe.then(|| path.clone()).flatten();
                let handle = s.spawn(move || probing.map(|p| probe_version(&p)));
                (name, path, handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, path, handle)| {
                let probe = handle.join().ok().flatten();
                let broken = probe.as_ref().and_then(|p| p.broken()).map(str::to_owned);
                let version = probe.and_then(VersionProbe::version);
                (
                    name,
                    BinaryFacts {
                        path,
                        version,
                        broken,
                    },
                )
            })
            .collect()
    })
}

fn table(rows: &[AgentRow], no_probe: bool) -> String {
    let mut out = String::new();
    let agent_w = rows.iter().map(|r| r.agent.len()).max().unwrap_or(0).max(5);
    let status = |r: &AgentRow| r.status.clone();
    let status_w = "unavailable".len();
    out.push_str(&format!(
        "{:<agent_w$}  {:<status_w$}  {:<9}  {}\n",
        "AGENT", "STATUS", "POOL", "BINARY  VERSION",
    ));
    for r in rows {
        out.push_str(&format!(
            "{:<agent_w$}  {:<status_w$}  {:<9}  {}  {}\n",
            r.agent,
            status(r),
            r.pool_state,
            r.path.as_deref().unwrap_or("(not found)"),
            version_label(r, no_probe),
        ));
        out.push_str(&format!(
            "{:<agent_w$}    efforts: {}\n",
            "",
            if r.efforts.is_empty() {
                "none".to_string()
            } else {
                r.efforts.join(" ")
            }
        ));
        out.push_str(&format!(
            "{:<agent_w$}    can: resume={} headless={} skills={}\n",
            "", r.capabilities.resume, r.capabilities.headless, r.capabilities.skills
        ));
        for m in &r.models {
            out.push_str(&model_line(agent_w, m));
        }
    }
    out
}

/// The last column of a harness row: its version, or why it has none.
fn version_label(r: &AgentRow, no_probe: bool) -> String {
    match (&r.version, &r.broken, r.found, no_probe) {
        (Some(v), _, _, _) => v.clone(),
        (None, Some(why), true, _) => format!("(broken: {why})"),
        (None, None, true, true) => "(not probed)".into(),
        (None, None, true, false) => "(no answer)".into(),
        (None, _, false, _) => "-".into(),
    }
}

/// The line of one model and the teammates that run on it, indented under
/// the agent column.
fn model_line(agent_w: usize, m: &ModelUse) -> String {
    let users: Vec<String> = m
        .teammates
        .iter()
        .map(|t| match &t.effort {
            Some(e) => format!("{} ({e})", t.name),
            None => t.name.clone(),
        })
        .collect();
    format!(
        "{:<agent_w$}    model {}: {}\n",
        "",
        m.model.as_deref().unwrap_or("-"),
        users.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::harness::inventory::{CapabilitySummary, TeammateUse};

    const HEADER: &str = "AGENT  STATUS       POOL       BINARY  VERSION\n";

    fn row(agent: &str) -> AgentRow {
        AgentRow {
            agent: agent.into(),
            found: true,
            path: Some(format!("/bin/{agent}")),
            version: None,
            broken: None,
            status: "available".into(),
            efforts: Vec::new(),
            capabilities: CapabilitySummary {
                resume: true,
                headless: false,
                skills: "plugin-dir".into(),
            },
            models: Vec::new(),
            pools: Vec::new(),
            pool_state: "ok".into(),
            available: true,
        }
    }

    fn lines(lines: &[&str]) -> String {
        lines.iter().map(|l| format!("{l}\n")).collect()
    }

    #[test]
    fn an_empty_inventory_prints_the_header_alone() {
        assert_eq!(table(&[], false), HEADER);
        assert_eq!(table(&[], true), HEADER);
    }

    #[test]
    fn a_row_prints_its_binary_efforts_capabilities_and_models() {
        let mut agent = row("antigravity");
        agent.efforts = vec!["low".into(), "high".into()];
        agent.models = vec![
            ModelUse {
                model: Some("m1".into()),
                teammates: vec![
                    TeammateUse {
                        name: "alice".into(),
                        effort: Some("high".into()),
                    },
                    TeammateUse {
                        name: "bob".into(),
                        effort: None,
                    },
                ],
            },
            ModelUse {
                model: None,
                teammates: Vec::new(),
            },
        ];
        // The agent column is as wide as the longest name.
        assert_eq!(
            table(&[agent], false),
            lines(&[
                "AGENT        STATUS       POOL       BINARY  VERSION",
                "antigravity  available    ok         /bin/antigravity  (no answer)",
                "               efforts: low high",
                "               can: resume=true headless=false skills=plugin-dir",
                "               model m1: alice (high), bob",
                "               model -: ",
            ])
        );
    }

    #[test]
    fn the_version_column_follows_what_the_probe_found() {
        let mut shown = row("a-version");
        shown.version = Some("1.2.3".into());
        // A version wins over a broken note.
        shown.broken = Some("ignored".into());
        let mut broken = row("b-broken");
        broken.broken = Some("exit 1".into());
        let silent = row("c-silent");
        let mut missing = row("d-missing");
        missing.found = false;
        missing.path = None;
        missing.status = "unavailable".into();
        // A missing binary is a dash, whatever else is known.
        missing.broken = Some("stale".into());
        let rows = [shown, broken, silent, missing];

        let probed = lines(&[
            "AGENT      STATUS       POOL       BINARY  VERSION",
            "a-version  available    ok         /bin/a-version  1.2.3",
            "             efforts: none",
            "             can: resume=true headless=false skills=plugin-dir",
            "b-broken   available    ok         /bin/b-broken  (broken: exit 1)",
            "             efforts: none",
            "             can: resume=true headless=false skills=plugin-dir",
            "c-silent   available    ok         /bin/c-silent  (no answer)",
            "             efforts: none",
            "             can: resume=true headless=false skills=plugin-dir",
            "d-missing  unavailable  ok         (not found)  -",
            "             efforts: none",
            "             can: resume=true headless=false skills=plugin-dir",
        ]);
        assert_eq!(table(&rows, false), probed);
        // Without a probe, only the silent binary reads differently.
        assert_eq!(
            table(&rows, true),
            probed.replace("(no answer)", "(not probed)")
        );
    }
}
