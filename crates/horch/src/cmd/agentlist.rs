//! `horch agent-list` - every agent harness horch can drive, whether its
//! binary is there, and who uses it.
//!
//! The facts are gathered here (a path lookup, a `--version` probe, the cached
//! quota view); `horch_core::harness::inventory` turns them into rows. Nothing
//! here starts a model turn or refreshes the quota.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_core::clock;
use horch_core::harness::inventory::{inventory, AgentRow, BinaryFacts};
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
            Some((k.as_str(), resolve(ctx, &bin)))
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

/// The binary a name or path stands for, when it exists.
fn resolve(ctx: &RuntimeContext, bin: &Path) -> Option<PathBuf> {
    if bin.components().count() > 1 || bin.is_absolute() {
        return bin.is_file().then(|| bin.to_path_buf());
    }
    process::which(
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
        &bin.to_string_lossy(),
    )
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
        let version = match (&r.version, &r.broken, r.found, no_probe) {
            (Some(v), _, _, _) => v.clone(),
            (None, Some(why), true, _) => format!("(broken: {why})"),
            (None, None, true, true) => "(not probed)".into(),
            (None, None, true, false) => "(no answer)".into(),
            (None, _, false, _) => "-".into(),
        };
        out.push_str(&format!(
            "{:<agent_w$}  {:<status_w$}  {:<9}  {}  {}\n",
            r.agent,
            status(r),
            r.pool_state,
            r.path.as_deref().unwrap_or("(not found)"),
            version,
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
            let users: Vec<String> = m
                .teammates
                .iter()
                .map(|t| match &t.effort {
                    Some(e) => format!("{} ({e})", t.name),
                    None => t.name.clone(),
                })
                .collect();
            out.push_str(&format!(
                "{:<agent_w$}    model {}: {}\n",
                "",
                m.model.as_deref().unwrap_or("-"),
                users.join(", ")
            ));
        }
    }
    out
}
