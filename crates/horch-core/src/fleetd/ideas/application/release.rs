//! The release of Phase 6 at the first pass: the folder of each backfilled
//! idea, and the end of the questions and wakes of the old orchestrator
//! (`docs/specs/fleetd/design.md`, "What a release does to a run in
//! flight", row Phase 6; `docs/tickets/fleetd-p6/50-release.md`; MAP.md of
//! fleetd-p6, Q6 and Q11).
//!
//! The migration gives each run in flight 1 idea with the marker
//! [`RELEASE`] in `approved_sha256`, and moves each question of the run to
//! the feature, owned by its sub-orchestrator (ticket 02). It reads no file.
//! [`release`] does the rest, for each backfilled idea that has no
//! approval yet, within the work budget of the pass:
//!
//! - idea.md, created 1 time: `source: release`, the run slug and the path
//!   of the run's `MAP.md` ([`SOURCE`]). Its content comes only from the
//!   committed rows of the migration, so a write again has the same bytes.
//! - approval.json, through the approval of ticket 18: it names the
//!   absolute path of the run's `MAP.md` and its sha256, so I7 compares
//!   that file. A run with no `MAP.md` (a ticket added alone) names its
//!   idea.md: fleetd wrote it, and it never changes. `approved_sha256`
//!   keeps the marker. The row is committed first, and the caller writes
//!   the file after the commit ([`ReleasedApproval::write`]). After a crash
//!   between the 2, [`pending_approvals`] gives the same file again from
//!   the event [`RELEASED_CAUSE`].
//! - The run's `orchestrator_worker_id` is cleared, with the old value in
//!   that event: each wake and each respawn of the old flow reads it, so
//!   the old orchestrator gets no more questions and no more wakes. Its
//!   process is not stopped (ticket 45): it ends by itself.
//!
//! The run's `MAP.md` and the ticket snapshots of its tasks are not
//! changed: `claim` and the attempt files print the snapshots that
//! `add-map` stored. The sub-orchestrator of the feature takes the open
//! questions of the run ([`super::sub_orchestrators`]).

use std::path::Path;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::sqlite::{fact_data, has_fact};
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::budget::PassBudget;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::application::approval::{record_approval, ApprovalFile};
use crate::fleetd::ideas::domain::approval::{Approval, APPROVAL_JSON, BY_OPERATOR, RELEASE};
use crate::fleetd::ideas::domain::idea_folder::{IdeaFile, IDEA_MD};
use crate::fsx::FsxError;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::{tasks, util};

/// The `source` of the idea.md of a backfilled idea.
pub const SOURCE: &str = "release";

/// The cause of the event of a released idea. Its `data_json` holds the
/// text of its approval.json, the run, and the old orchestrator.
pub const RELEASED_CAUSE: &str = "idea released";

/// The cause of the approval of a released idea.
const APPROVED_CAUSE: &str = "the release approves the tickets of the run";

/// The cause of the event of a backfilled idea whose idea.md is not the
/// file that the release writes. It gets no approval, so I7 holds it.
const CONFLICT_CAUSE: &str = "idea.md is not the idea.md of the release: no approval";

/// The file name of the map of a run of the old flow.
const MAP_MD: &str = "MAP.md";

/// What 1 pass needs besides the ledger.
pub struct Release<'a> {
    /// The generation of the fleetd that writes.
    pub guard: Option<Generation>,
    /// The folder of the idea folders.
    pub ideas: &'a Path,
    /// The ledger owner, the owner of each idea folder.
    pub owner: User,
    pub budget: PassBudget,
}

/// The approval.json of 1 released idea, to write after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedApproval {
    pub idea: String,
    pub file: ApprovalFile,
}

impl ReleasedApproval {
    /// Write the approval.json of the idea folder in `ideas`.
    pub fn write(&self, ideas: &Path, owner: User) -> Result<()> {
        self.file
            .write(&IdeaFolder::create(ideas, &self.idea, owner)?)
    }
}

/// What 1 pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Released {
    /// The effects to do after the commit.
    pub approvals: Vec<ReleasedApproval>,
    /// The old orchestrators that get no more questions and no more wakes.
    pub orchestrators: Vec<String>,
}

/// The release duty, in the transaction `tx` of part 2 of a pass: each
/// backfilled idea with no approval, oldest first, at most
/// `ctx.budget` of them. idea.md is written here: its row is the idea row
/// that the migration committed. An idea leaves the work when its approval
/// is recorded, so the next pass goes on with the next ones.
pub fn release(tx: &Connection, ctx: &Release<'_>) -> Result<Released> {
    let mut out = Released::default();
    for id in unreleased(tx, ctx.budget)? {
        let row = ideas_store::idea(tx, &id)?.with_context(|| format!("no idea {id}"))?;
        release_idea(tx, ctx, &row, &mut out)?;
    }
    Ok(out)
}

/// The approval.json of each released idea whose file is missing: the
/// effects that a crash after the commit left undone.
pub fn pending_approvals(conn: &Connection, ideas: &Path) -> Result<Vec<ReleasedApproval>> {
    let mut stmt = conn.prepare(
        "SELECT id, approval_sha256 FROM ideas
         WHERE approved_sha256 = ?1 AND approval_sha256 IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt
        .query_map([RELEASE], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut pending = Vec::new();
    for (idea, sha) in rows {
        if std::fs::symlink_metadata(ideas.join(&idea).join(APPROVAL_JSON)).is_ok() {
            continue;
        }
        let Some(data) = fact_data(conn, Entity::Idea, &idea, EventKind::Note, RELEASED_CAUSE)?
        else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&data)
            .with_context(|| format!("the released event of {idea} is not JSON"))?;
        let Some(text) = v["approval"].as_str() else {
            bail!("the released event of {idea} has no approval");
        };
        // An approval that is not the recorded one is not written.
        if sha256_bytes(text.as_bytes()).hex() == sha {
            pending.push(ReleasedApproval {
                idea,
                file: ApprovalFile {
                    text: text.to_string(),
                    sha256: sha,
                },
            });
        }
    }
    Ok(pending)
}

/// The ids of the backfilled ideas with no approval, at most `budget`.
fn unreleased(conn: &Connection, budget: PassBudget) -> Result<Vec<String>> {
    let limit = i64::try_from(budget.rows()).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(
        "SELECT id FROM ideas WHERE approved_sha256 = ?1 AND approval_sha256 IS NULL
         ORDER BY id LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(params![RELEASE, limit], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// Release the idea `row`: its idea.md, its approval, and the end of the
/// old orchestrator of its run.
fn release_idea(
    tx: &Connection,
    ctx: &Release<'_>,
    row: &IdeaRow,
    out: &mut Released,
) -> Result<()> {
    let run = run_of(tx, row)?;
    let map = map_of(tx, &run)?;
    let folder = IdeaFolder::create(ctx.ideas, &row.id, ctx.owner.clone())?;
    match folder.write_idea(&idea_file(row, &run, map.as_ref().map(|(p, _)| p.as_str()))) {
        Ok(_) => {}
        Err(FolderError::Write(FsxError::Conflict { .. })) => {
            return conflict(tx, ctx.guard, &row.id);
        }
        Err(e) => return Err(e.into()),
    }
    let (file, sha256) = match map {
        Some(found) => found,
        None => (IDEA_MD.to_string(), folder.sha256_of(IDEA_MD)?),
    };
    let approval = Approval {
        idea: row.id.clone(),
        file,
        sha256,
        approved_at: util::now(),
        by: BY_OPERATOR.to_string(),
    };
    let cause = |text| Cause {
        actor: Actor::Horch,
        text,
    };
    let file = record_approval(tx, ctx.guard, &approval, RELEASE, cause(APPROVED_CAUSE))?;
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::Idea,
        &row.id,
        cause(RELEASED_CAUSE),
        &json!({
            "approval": file.text,
            "run": run.run_id,
            "orchestrator": run.orchestrator_worker_id,
        }),
    )?;
    if let Some(orchestrator) = run.orchestrator_worker_id {
        runs::clear_orchestrator(tx, &run.run_id)?;
        out.orchestrators.push(orchestrator);
    }
    out.approvals.push(ReleasedApproval {
        idea: row.id.clone(),
        file,
    });
    Ok(())
}

/// The run of the backfilled idea `row`: the internal run of its feature
/// (`schema::run_idea`; MAP.md of fleetd-p6, Q5).
fn run_of(conn: &Connection, row: &IdeaRow) -> Result<Run> {
    runs::of_feature(conn, &row.feature_id)?
        .with_context(|| format!("the backfilled idea {} has no run", row.id))
}

/// The `MAP.md` of `run` and its sha256: the `MAP.md` beside the first
/// ticket of the run, in the order of the map, that has one. `None` when no
/// ticket has one. The path is absolute, as I7 reads it.
fn map_of(conn: &Connection, run: &Run) -> Result<Option<(String, String)>> {
    let root = Path::new(&run.project_root);
    let files = tasks::of_run(conn, &run.run_id)?
        .into_iter()
        .filter_map(|t| t.task_file);
    for file in files {
        let Some(dir) = root.join(file).parent().map(Path::to_path_buf) else {
            continue;
        };
        let map = dir.join(MAP_MD);
        if !map.is_absolute() || !map.is_file() {
            continue;
        }
        let (Some(path), Ok(bytes)) = (map.to_str(), std::fs::read(&map)) else {
            continue;
        };
        return Ok(Some((path.to_string(), sha256_bytes(&bytes).hex())));
    }
    Ok(None)
}

/// idea.md of the backfilled idea `row` of `run` with the map `map`.
fn idea_file(row: &IdeaRow, run: &Run, map: Option<&str>) -> IdeaFile {
    let slug = &run.slug;
    IdeaFile {
        id: row.id.clone(),
        created: row.created_at.clone(),
        source: SOURCE.to_string(),
        priority: row.priority,
        depends_on: Vec::new(),
        text: format!(
            "# {slug}\n\nThe run {slug} ({}) was in flight at the release of Phase 6. \
             Its tickets stand for your approval.\n\nrun: {slug}\nmap: {}\n",
            run.run_id,
            map.unwrap_or("none"),
        ),
    }
}

/// Record 1 time that the idea.md of `idea` is not the file that the
/// release writes. The idea gets no approval, so I7 holds it with 1
/// needs-you item.
fn conflict(tx: &Connection, guard: Option<Generation>, idea: &str) -> Result<()> {
    if has_fact(tx, Entity::Idea, idea, EventKind::Note, CONFLICT_CAUSE)? {
        return Ok(());
    }
    let cause = Cause {
        actor: Actor::Horch,
        text: CONFLICT_CAUSE,
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
