//! horch idea approve and revise: write approval.json, and hold an idea
//! whose assumptions changed (I7).
//!
//! Each use case writes its rows in the caller's transaction and returns
//! the file to write after the commit (each effect is a committed row
//! first). fleetd keeps the sha256 of each approval.json that it writes in
//! `ideas.approval_sha256`, and ignores and logs an approval.json with
//! another content (FR-10, FM-32).

use std::path::Path;

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::sqlite::has_fact;
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

/// approval.json as fleetd writes it, to write after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalFile {
    pub text: String,
    /// The sha256 of `text`, as `ideas.approval_sha256` holds it.
    pub sha256: String,
}

impl ApprovalFile {
    /// Replace the approval.json of `folder` with this file.
    pub fn write(&self, folder: &IdeaFolder) -> Result<()> {
        Ok(folder.write_approval(&self.text)?)
    }
}

/// What [`approve`] did.
#[derive(Debug)]
pub enum Approved {
    /// The idea is in `specifying`; write the file after the commit.
    Approved(ApprovalFile),
    /// Nothing changed.
    Refused(Refusal),
}

/// 1 amendment of a revise, to write after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amendment {
    pub n: u32,
    pub text: String,
}

impl Amendment {
    /// Create the amendment file `n` of `folder` with this text.
    pub fn write(&self, folder: &IdeaFolder) -> Result<()> {
        folder.write_amendment(self.n, &self.text)?;
        Ok(())
    }
}

/// What [`revise`] did.
#[derive(Debug)]
pub enum Revised {
    /// The idea is in `clarifying`; write the amendment after the commit.
    Revised(Amendment),
    /// Nothing changed.
    Refused(Refusal),
}

/// approval.json on disk.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalJson {
    idea: String,
    file: String,
    sha256: String,
    approved_at: String,
    by: String,
}

/// `ApproveAssumptions`: your approval of the assumptions.md of `idea` with
/// the hash `seen`, the hash that `horch idea show` printed to you. An idea
/// that is not in `awaiting_approval`, or whose assumptions.md now has
/// another hash, is refused. Else the idea moves to `specifying` and its
/// approval is recorded ([`record_approval`]).
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

/// Record `approval` in `tx`: `approved_sha256` (the approved hash, or the
/// marker `release`) and the sha256 of its approval.json, with 1 event.
/// The caller writes the returned file after the commit.
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

/// `ReviseAssumptions`: your text `text` for an idea in
/// `awaiting_approval`. The idea goes back to `clarifying`, and the text
/// becomes the next amendment of its folder.
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
    let n = u32::try_from(folder.read_amendments()?.len())? + 1;
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
    Ok(Revised::Revised(Amendment {
        n,
        text: text.to_string(),
    }))
}

/// The approval.json of `idea` when fleetd wrote it: its sha256 is
/// `ideas.approval_sha256`. Another file is ignored, and 1 event of the
/// idea logs it 1 time. No file is `None`.
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

/// I7, the approval part, for `idea` in 1 pass: `true` when it holds. An
/// idea past `awaiting_approval` whose approved file now has another hash
/// is held: 1 open needs-you item says so, and the dispatch and the plan
/// commit of the idea wait for `true`. When it holds, that item is
/// resolved.
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

/// The sha256 of the approved file `file` now, or `None` when it cannot be
/// read. A path in the idea folder is read with the checks of SR-6; an
/// absolute path is the `MAP.md` of a run in flight (MAP.md, Q6).
fn current_sha256(folder: &IdeaFolder, file: &str) -> Option<String> {
    if Path::new(file).is_absolute() {
        return std::fs::read(file)
            .ok()
            .map(|bytes| sha256_bytes(&bytes).hex());
    }
    folder.sha256_of(file).ok()
}

/// Log 1 time that fleetd ignored an approval.json of `idea`, with `cause`.
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
