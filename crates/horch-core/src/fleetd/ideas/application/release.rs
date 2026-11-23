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

pub const SOURCE: &str = "release";

pub const RELEASED_CAUSE: &str = "idea released";

const APPROVED_CAUSE: &str = "the release approves the tickets of the run";

const CONFLICT_CAUSE: &str = "idea.md is not the idea.md of the release: no approval";

const MAP_MD: &str = "MAP.md";

pub struct Release<'a> {
    pub guard: Option<Generation>,
    pub ideas: &'a Path,
    pub owner: User,
    pub budget: PassBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedApproval {
    pub idea: String,
    pub file: ApprovalFile,
}

impl ReleasedApproval {
    pub fn write(&self, ideas: &Path, owner: User) -> Result<()> {
        self.file
            .write(&IdeaFolder::create(ideas, &self.idea, owner)?)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Released {
    pub approvals: Vec<ReleasedApproval>,
    pub orchestrators: Vec<String>,
}

pub fn release(tx: &Connection, ctx: &Release<'_>) -> Result<Released> {
    let mut out = Released::default();
    for id in unreleased(tx, ctx.budget)? {
        let row = ideas_store::idea(tx, &id)?.with_context(|| format!("no idea {id}"))?;
        release_idea(tx, ctx, &row, &mut out)?;
    }
    Ok(out)
}

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

fn run_of(conn: &Connection, row: &IdeaRow) -> Result<Run> {
    runs::of_feature(conn, &row.feature_id)?
        .with_context(|| format!("the backfilled idea {} has no run", row.id))
}

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
