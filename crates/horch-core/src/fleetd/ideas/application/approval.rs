use std::path::Path;

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::sqlite::{facts_of, has_fact};
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::application::needs_you::{open_item, resolve, NeedsYouList};
use crate::fleetd::ideas::domain::approval::{
    approved_target, check_approve, check_revise, i7_approval_holds, i7_key, i7_text, Approval,
    Refusal, APPROVED_TO, I7_KIND, PAST_APPROVAL, REVISED_TO,
};
use crate::fleetd::ideas::domain::idea_folder::ASSUMPTIONS_MD;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::util;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalFile {
    pub text: String,
    pub sha256: String,
}

impl ApprovalFile {
    pub fn write(&self, folder: &IdeaFolder) -> Result<()> {
        Ok(folder.write_approval(&self.text)?)
    }
}

#[derive(Debug)]
pub enum Approved {
    Approved(ApprovalFile),
    Refused(Refusal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amendment {
    pub n: u32,
    pub text: String,
}

impl Amendment {
    pub fn write(&self, folder: &IdeaFolder) -> Result<()> {
        folder.write_amendment(self.n, &self.text)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum Revised {
    Revised(Amendment),
    Refused(Refusal),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalJson {
    idea: String,
    file: String,
    sha256: String,
    approved_at: String,
    by: String,
}

pub fn approve(
    tx: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
    seen: &str,
) -> Result<Approved> {
    let row = idea_row(tx, idea)?;
    if let Err(r) = check_revise(idea, &row.state) {
        return Ok(Approved::Refused(r));
    }
    let now = match folder.sha256_of(ASSUMPTIONS_MD) {
        Ok(sha) => sha,
        Err(e) => {
            return Ok(Approved::Refused(Refusal::NoAssumptions {
                idea: idea.to_string(),
                reason: e.to_string(),
            }))
        }
    };
    if let Err(r) = check_approve(idea, &row.state, seen, &now) {
        return Ok(Approved::Refused(r));
    }
    let cause = Cause {
        actor: Actor::Operator,
        text: "you approved the assumptions",
    };
    transition(
        tx,
        guard,
        Entity::Idea,
        idea,
        APPROVED_TO,
        cause,
        &NeedsYouList,
    )?;
    let approval = Approval::of_assumptions(idea, &now, &util::now());
    let file = record_approval(tx, guard, &approval, &now, cause)?;
    Ok(Approved::Approved(file))
}

pub fn record_approval(
    tx: &Connection,
    guard: Option<Generation>,
    approval: &Approval,
    approved_sha256: &str,
    cause: Cause<'_>,
) -> Result<ApprovalFile> {
    let json = ApprovalJson {
        idea: approval.idea.clone(),
        file: approval.file.clone(),
        sha256: approval.sha256.clone(),
        approved_at: approval.approved_at.clone(),
        by: approval.by.clone(),
    };
    let text = format!("{}\n", serde_json::to_string_pretty(&json)?);
    let sha256 = sha256_bytes(text.as_bytes()).hex();
    ideas_store::set_approval(tx, guard, &approval.idea, approved_sha256, &sha256, cause)?;
    Ok(ApprovalFile { text, sha256 })
}

pub fn revise(
    tx: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
    text: &str,
) -> Result<Revised> {
    let row = idea_row(tx, idea)?;
    if let Err(r) = check_revise(idea, &row.state) {
        return Ok(Revised::Refused(r));
    }
    let cause = Cause {
        actor: Actor::Operator,
        text: "you revised the assumptions",
    };
    transition(
        tx,
        guard,
        Entity::Idea,
        idea,
        REVISED_TO,
        cause,
        &NeedsYouList,
    )?;
    Ok(Revised::Revised(record_amendment(
        tx, guard, folder, idea, text, cause,
    )?))
}

pub fn record_amendment(
    tx: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
    text: &str,
    cause: Cause<'_>,
) -> Result<Amendment> {
    let recorded = facts_of(tx, Entity::Idea, idea, EventKind::Note)?
        .iter()
        .filter_map(|f| {
            serde_json::from_str::<serde_json::Value>(&f.data_json).ok()?["amendment"].as_u64()
        })
        .max()
        .unwrap_or(0);
    let files = u64::try_from(folder.read_amendments()?.len())?;
    let n = u32::try_from(recorded.max(files))? + 1;
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Idea,
        idea,
        cause,
        &json!({ "amendment": n, "text": text }),
    )?;
    Ok(Amendment {
        n,
        text: text.to_string(),
    })
}

pub fn trusted_approval(
    tx: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
) -> Result<Option<Approval>> {
    let row = idea_row(tx, idea)?;
    let text = match folder.read_approval() {
        Ok(text) => text,
        Err(FolderError::Missing { .. }) => return Ok(None),
        Err(e @ (FolderError::Rejected { .. } | FolderError::Invalid { .. })) => {
            ignored(tx, guard, idea, &format!("approval.json ignored: {e}"))?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let sha = sha256_bytes(text.as_bytes()).hex();
    if row.approval_sha256.as_deref() != Some(sha.as_str()) {
        let cause = format!(
            "approval.json ignored: its sha256 {sha} is not the sha256 of the approval.json that fleetd wrote"
        );
        ignored(tx, guard, idea, &cause)?;
        return Ok(None);
    }
    let json: ApprovalJson = serde_json::from_str(&text)?;
    Ok(Some(Approval {
        idea: json.idea,
        file: json.file,
        sha256: json.sha256,
        approved_at: json.approved_at,
        by: json.by,
    }))
}

pub fn check_i7(
    tx: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
) -> Result<bool> {
    let row = idea_row(tx, idea)?;
    let key = i7_key(idea);
    if !PAST_APPROVAL.contains(&row.state.as_str()) {
        resolve(tx, &key)?;
        return Ok(true);
    }
    let approval = trusted_approval(tx, guard, folder, idea)?;
    let target = approved_target(row.approved_sha256.as_deref(), approval.as_ref());
    let current = target.and_then(|(file, _)| current_sha256(folder, file));
    if i7_approval_holds(&row.state, target.map(|(_, sha)| sha), current.as_deref()) {
        resolve(tx, &key)?;
        return Ok(true);
    }
    let file = target.map_or(ASSUMPTIONS_MD, |(file, _)| file);
    let text = i7_text(idea, file);
    open_item(
        tx,
        &Write {
            key: &key,
            kind: I7_KIND,
            entity: Entity::Idea.as_str(),
            entity_id: idea,
            text: &text,
        },
    )?;
    Ok(false)
}

fn current_sha256(folder: &IdeaFolder, file: &str) -> Option<String> {
    if Path::new(file).is_absolute() {
        return std::fs::read(file)
            .ok()
            .map(|bytes| sha256_bytes(&bytes).hex());
    }
    folder.sha256_of(file).ok()
}

fn ignored(tx: &Connection, guard: Option<Generation>, idea: &str, cause: &str) -> Result<()> {
    if has_fact(tx, Entity::Idea, idea, EventKind::Note, cause)? {
        return Ok(());
    }
    let cause = Cause {
        actor: Actor::Horch,
        text: cause,
    };
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Idea,
        idea,
        cause,
        &json!({}),
    )?;
    Ok(())
}

fn idea_row(conn: &Connection, idea: &str) -> Result<IdeaRow> {
    ideas_store::idea(conn, idea)?.ok_or_else(|| anyhow!("no idea {idea} in the ledger"))
}
