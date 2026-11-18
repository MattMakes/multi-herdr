//! The rows of `ideas` and `requirements` in SQLite.
//!
//! No other code writes these tables. A new idea and its event go through
//! `created()` of the 1 door. A state change is not a function of this
//! store: the use cases call `transition()` (`docs/specs/fleetd/design.md`,
//! "Ledger", rule 1 "One door"). Each write of a fact that is not a state
//! records 1 event of its idea (OR-2).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, record, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::domain::idea::FIRST;
use crate::taskledger::model::EventKind;
use crate::taskledger::util;

/// 1 row of `ideas`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaRow {
    pub id: String,
    pub feature_id: String,
    pub state: String,
    pub priority: i64,
    pub pane: Option<String>,
    pub waiting_on_you_since: Option<String>,
    pub approved_sha256: Option<String>,
    /// The sha256 of the `approval.json` that fleetd wrote.
    pub approval_sha256: Option<String>,
    pub plan_sha256: Option<String>,
    pub phase_attempts: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A new idea, in [`FIRST`].
#[derive(Debug, Clone, Copy)]
pub struct NewIdea<'a> {
    pub id: &'a str,
    pub feature_id: &'a str,
    pub priority: i64,
}

/// 1 requirement of an idea, from its `requirements.yaml`.
#[derive(Debug, Clone, Copy)]
pub struct Requirement<'a> {
    pub req_id: &'a str,
    pub text: &'a str,
}

/// Insert the idea `new` in [`FIRST`], with its event, in `tx`.
pub fn insert_idea(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewIdea<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    // `created()` checks this too, but after the insert.
    check_writable(tx, guard)?;
    let now = util::now();
    tx.execute(
        "INSERT INTO ideas (id, feature_id, state, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![new.id, new.feature_id, FIRST, new.priority, now],
    )?;
    created(tx, guard, Entity::Idea, new.id, cause)?;
    Ok(())
}

/// The idea `id`, or `None`.
pub fn idea(conn: &Connection, id: &str) -> Result<Option<IdeaRow>> {
    let row = conn
        .query_row(
            "SELECT id, feature_id, state, priority, pane, waiting_on_you_since,
                approved_sha256, approval_sha256, plan_sha256, phase_attempts, created_at,
                updated_at
             FROM ideas WHERE id = ?1",
            [id],
            |r| {
                Ok(IdeaRow {
                    id: r.get(0)?,
                    feature_id: r.get(1)?,
                    state: r.get(2)?,
                    priority: r.get(3)?,
                    pane: r.get(4)?,
                    waiting_on_you_since: r.get(5)?,
                    approved_sha256: r.get(6)?,
                    approval_sha256: r.get(7)?,
                    plan_sha256: r.get(8)?,
                    phase_attempts: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Record the approval of the idea `id` in `tx`: `approved_sha256`, the
/// sha256 of the approved file (or the marker `release`), and
/// `approval_sha256`, the sha256 of the `approval.json` that fleetd writes,
/// with 1 event of the idea that holds both.
pub fn set_approval(
    tx: &Connection,
    guard: Option<Generation>,
    id: &str,
    approved_sha256: &str,
    approval_sha256: &str,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "UPDATE ideas SET approved_sha256 = ?2, approval_sha256 = ?3, updated_at = ?4
         WHERE id = ?1",
        params![id, approved_sha256, approval_sha256, util::now()],
    )?;
    let data = json!({ "approved_sha256": approved_sha256, "approval_sha256": approval_sha256 });
    record(tx, guard, EventKind::Note, Entity::Idea, id, cause, &data)?;
    Ok(())
}

/// Move the idea `id` from the feature `from` to the feature `to` in `tx`,
/// with 1 event of the idea. `false`, with no write, when the idea is not
/// an idea of `from`.
pub fn set_feature(
    tx: &Connection,
    guard: Option<Generation>,
    id: &str,
    from: &str,
    to: &str,
    cause: Cause<'_>,
) -> Result<bool> {
    check_writable(tx, guard)?;
    let changed = tx.execute(
        "UPDATE ideas SET feature_id = ?3, updated_at = ?4 WHERE id = ?1 AND feature_id = ?2",
        params![id, from, to, util::now()],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    let data = json!({ "feature_id": to, "from": from });
    record(tx, guard, EventKind::Note, Entity::Idea, id, cause, &data)?;
    Ok(true)
}

/// Insert the requirements `reqs` of the idea `idea_id`, with 1 event of
/// the idea that names their ids, in `tx`.
pub fn insert_requirements(
    tx: &Connection,
    guard: Option<Generation>,
    idea_id: &str,
    reqs: &[Requirement<'_>],
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    for req in reqs {
        tx.execute(
            "INSERT INTO requirements (idea_id, req_id, text) VALUES (?1, ?2, ?3)",
            params![idea_id, req.req_id, req.text],
        )?;
    }
    let ids: Vec<&str> = reqs.iter().map(|r| r.req_id).collect();
    let data = json!({ "requirements": ids });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Idea,
        idea_id,
        cause,
        &data,
    )?;
    Ok(())
}

/// The ideas of the feature `feature_id`, oldest first.
pub fn of_feature(conn: &Connection, feature_id: &str) -> Result<Vec<IdeaRow>> {
    let mut stmt =
        conn.prepare("SELECT id FROM ideas WHERE feature_id = ?1 ORDER BY created_at, id")?;
    let ids = stmt
        .query_map([feature_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.extend(idea(conn, &id)?);
    }
    Ok(out)
}

/// The requirements of the idea `idea_id`, as (`req_id`, `text`), by id.
pub fn requirements(conn: &Connection, idea_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT req_id, text FROM requirements WHERE idea_id = ?1 ORDER BY req_id")?;
    let rows = stmt
        .query_map([idea_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}
