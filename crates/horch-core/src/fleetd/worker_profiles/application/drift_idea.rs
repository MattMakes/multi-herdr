use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::{
    append_named_fact, last_named_facts, NamedFact,
};
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::intake::{add_idea, Added, Target, MAX_TEXT};
use crate::fleetd::worker_profiles::adapters::docs_dir::docs_root;
use crate::fleetd::worker_profiles::domain::conformance::{
    AdapterConformanceFailed, ConformanceReport, Verdict,
};
use crate::taskledger::model::{Actor, EventKind};

pub const KIND: &str = AdapterConformanceFailed::NAME;

const ABOUT: &str = "adapter";

const CHANGES_FILE: &str = "CHANGES.md";

const CUT: &str = "\n[CHANGES.md is cut here: read the file for the rest]\n";

pub fn add_drift_idea(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    report: &ConformanceReport,
    fleet_root: &Path,
    target: Target<'_>,
) -> Result<Option<Added>> {
    if report.failure().is_none() {
        return Ok(None);
    }
    let key = request_key(report);
    let done = last_named_facts(tx, ABOUT, &[EventKind::Added])?;
    if done.iter().any(|f| f.id == key) {
        return Ok(None);
    }
    let docs = docs_root(fleet_root).join(&report.cli);
    let changes_path = docs.join(CHANGES_FILE);
    let conformance_path = docs.join(&report.version).join("conformance.json");
    let changes = read_changes(&changes_path)?;
    let text = idea_text(report, &changes_path, changes.as_deref(), &conformance_path);
    let cause = Cause {
        actor: Actor::Horch,
        text: KIND,
    };
    let added = add_idea(tx, ctx, &text, KIND, target, cause)?;
    append_named_fact(
        tx,
        ctx.guard,
        &NamedFact {
            kind: EventKind::Added,
            about: ABOUT,
            id: &key,
            cause: KIND,
            actor: Actor::Horch,
            data_json: &json!({ "idea": added.file.id }).to_string(),
        },
    )?;
    let item_key = format!("{KIND}:{key}");
    let item_text = format!(
        "{}\n\nRead the idea {} in ideas/{}/idea.md.",
        title(report),
        added.file.id,
        added.file.id
    );
    ctx.needs_you.open_item(
        tx,
        &NewItem {
            key: &item_key,
            kind: KIND,
            entity: Entity::Idea,
            entity_id: &added.file.id,
            text: &item_text,
        },
    )?;
    Ok(Some(added))
}

fn request_key(report: &ConformanceReport) -> String {
    format!("{} {} {}", report.cli, report.version, report.adapter_hash)
}

const VERB: &str = "Update";

fn title(report: &ConformanceReport) -> String {
    format!("{VERB} the {} adapter for {}", report.cli, report.version)
}

fn read_changes(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn idea_text(
    report: &ConformanceReport,
    changes_path: &Path,
    changes: Option<&str>,
    conformance_path: &Path,
) -> String {
    let head = format!(
        "{}\n\nThe conformance run of {} {} failed (adapter hash {}).\n\n## {}\n\n",
        title(report),
        report.cli,
        report.version,
        report.adapter_hash,
        changes_path.display()
    );
    let mut checks = format!("\n## Failed checks of {}\n\n", conformance_path.display());
    for r in report.checks.iter().filter(|r| r.verdict == Verdict::Fail) {
        checks.push_str(&format!("- {}: {}\n", r.check.name(), r.evidence));
    }
    let changes = match changes {
        None => "No CHANGES.md was written.\n".to_string(),
        Some(text) => {
            let room = (MAX_TEXT as usize).saturating_sub(head.len() + checks.len() + CUT.len());
            cut(text, room)
        }
    };
    format!("{head}{changes}{checks}")
}

fn cut(text: &str, room: usize) -> String {
    if text.len() <= room + CUT.len() {
        return text.to_string();
    }
    let mut end = room;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CUT}", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_changes_is_cut_at_a_char_boundary() {
        let text = "é".repeat(100);
        let out = cut(&text, 5);
        assert!(out.starts_with("éé"));
        assert!(out.ends_with(CUT));
    }

    #[test]
    fn a_short_changes_stays_whole() {
        assert_eq!(cut("abc", 3), "abc");
    }
}
