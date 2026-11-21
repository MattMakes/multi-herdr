use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::budget::PassBudget;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::application::approval::trusted_approval;
use crate::fleetd::ideas::application::direct;
use crate::fleetd::ideas::application::plan_commit::{
    validate_plan, PlanCommit, Verdict, PLAN_YAML, VERDICT_FILE,
};
use crate::fleetd::ideas::domain::idea_folder::{FILE_LIMIT, IDEA_MD};
use crate::fleetd::integration::adapters::sqlite as feature_rows;
use crate::fleetd::integration::application::new_features::{
    name_feature_branch, BranchRef, NameSource, Named,
};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::taskledger::model::Actor;
use crate::taskledger::{runs, Store};

pub const CURSOR: &str = "ideas";

pub const DISPATCHED: [&str; 5] = [
    "captured",
    "clarifying",
    "awaiting_approval",
    "specifying",
    "ticketing",
];

pub const PLANNING: [&str; 4] = ["clarifying", "awaiting_approval", "specifying", "ticketing"];

#[derive(Debug, Clone, Copy)]
pub struct PrimaryTip<'a> {
    pub repo: &'a Path,
    pub branch: &'a str,
    pub tip: &'a str,
}

pub struct Dispatch<'a> {
    pub plan: PlanCommit<'a>,
    pub job: JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
    pub primaries: &'a [PrimaryTip<'a>],
    pub clock: FleetSeconds,
    pub stamp: &'a str,
    pub budget: PassBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub idea: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassReport {
    pub moves: Vec<Move>,
    pub refs: Vec<BranchRef>,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Dispatcher {
    stayed: HashMap<String, (String, Vec<(String, String)>)>,
}

enum Step {
    Moved(String),
    Stayed { wrote: bool },
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pass(&mut self, store: &mut Store, ctx: &Dispatch<'_>) -> Result<PassReport> {
        let chosen = store.immediate(|tx| {
            let keys = dispatched(tx)?;
            let mut cursor = cursors_store::load(tx, CURSOR, ctx.stamp)?;
            let chosen: Vec<String> = cursor
                .take(&keys, ctx.budget, ctx.stamp)
                .into_iter()
                .map(str::to_string)
                .collect();
            cursors_store::save(tx, CURSOR, &cursor)?;
            Ok(chosen)
        })?;
        let mut report = PassReport::default();
        for id in chosen {
            if let Err(e) = self.dispatch(store, ctx, &id, &mut report) {
                report.errors.push((id, format!("{e:#}")));
            }
        }
        Ok(report)
    }

    fn dispatch(
        &mut self,
        store: &mut Store,
        ctx: &Dispatch<'_>,
        id: &str,
        report: &mut PassReport,
    ) -> Result<()> {
        let Some(idea) = ideas_store::idea(store.conn(), id)? else {
            return Ok(());
        };
        let folder = IdeaFolder::new(ctx.plan.ideas, id, ctx.plan.owner.clone())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let print = fingerprint(folder.dir(), ctx);
        if self
            .stayed
            .get(id)
            .is_some_and(|(state, seen)| *state == idea.state && *seen == print)
        {
            return Ok(());
        }
        let step = match idea.state.as_str() {
            "captured" => captured(store, ctx, &idea, folder.dir())?,
            "clarifying" => clarifying(store, ctx, &idea, &folder, report)?,
            "awaiting_approval" => awaiting(store, ctx, &idea, &folder)?,
            "specifying" => specifying(store, ctx, &idea, &folder)?,
            "ticketing" => ticketing(store, ctx, &idea, folder.dir())?,
            _ => Step::Stayed { wrote: false },
        };
        match step {
            Step::Moved(to) => {
                self.stayed.remove(id);
                report.moves.push(Move {
                    idea: id.to_string(),
                    from: idea.state,
                    to,
                });
            }
            Step::Stayed { wrote: true } => {
                self.stayed.insert(id.to_string(), (idea.state, print));
            }
            Step::Stayed { wrote: false } => {
                self.stayed.remove(id);
            }
        }
        Ok(())
    }
}

fn dispatched(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ideas
         WHERE state IN ('captured', 'clarifying', 'awaiting_approval', 'specifying', 'ticketing')
         ORDER BY id",
    )?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn captured(store: &mut Store, ctx: &Dispatch<'_>, idea: &IdeaRow, dir: &Path) -> Result<Step> {
    let to = if direct::is_direct(store.conn(), &idea.id)? {
        if !direct::files_written(dir) {
            return Ok(Step::Stayed { wrote: false });
        }
        "ticketing"
    } else {
        if !std::fs::symlink_metadata(dir.join(IDEA_MD)).is_ok_and(|m| m.is_file()) {
            return Ok(Step::Stayed { wrote: false });
        }
        "clarifying"
    };
    store.immediate(|tx| {
        let ahead: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ideas
             WHERE feature_id = ?1 AND id <> ?2
               AND (state IN (?3, ?4, ?5, ?6)
                    OR (state = 'captured'
                        AND rowid < (SELECT rowid FROM ideas WHERE id = ?2)))",
            params![
                idea.feature_id,
                idea.id,
                PLANNING[0],
                PLANNING[1],
                PLANNING[2],
                PLANNING[3]
            ],
            |r| r.get(0),
        )?;
        if ahead > 0 {
            return Ok(Step::Stayed { wrote: false });
        }
        let text = if to == "ticketing" {
            "the --direct idea has its files and no other idea of its feature is in planning"
        } else {
            "no other idea of its feature is in planning"
        };
        move_idea(tx, ctx, &idea.id, "captured", to, text)
    })
}

fn clarifying(
    store: &mut Store,
    ctx: &Dispatch<'_>,
    idea: &IdeaRow,
    folder: &IdeaFolder,
    report: &mut PassReport,
) -> Result<Step> {
    let Ok(assumptions) = folder.read_assumptions() else {
        return Ok(Step::Stayed { wrote: false });
    };
    store.immediate(|tx| {
        let feature = feature_rows::feature(tx, &idea.feature_id)?
            .with_context(|| format!("the ledger has no feature {}", idea.feature_id))?;
        if feature.branch.is_none() {
            let run = runs::feature_run(tx, &feature.id)?
                .with_context(|| format!("the feature {} has no internal run", feature.id))?;
            let repo = Path::new(&run.project_root);
            let Some(primary) = ctx
                .primaries
                .iter()
                .find(|p| p.repo == repo && p.branch == feature.primary_branch)
            else {
                return Ok(Step::Stayed { wrote: false });
            };
            let source = NameSource::SubOrchestrator {
                idea: &idea.id,
                name: assumptions.feature.as_deref().unwrap_or(""),
            };
            let named = name_feature_branch(
                tx,
                &ctx.job,
                ctx.git,
                &feature.id,
                source,
                primary.tip,
                ctx.clock,
                by_fleetd("assumptions.md names the feature"),
            )?;
            match named {
                Named::Accepted { effect, .. } => report.refs.push(effect),
                Named::Refused { .. } => return Ok(Step::Stayed { wrote: true }),
            }
        }
        move_idea(
            tx,
            ctx,
            &idea.id,
            "clarifying",
            "awaiting_approval",
            "assumptions.md validates",
        )
    })
}

fn awaiting(
    store: &mut Store,
    ctx: &Dispatch<'_>,
    idea: &IdeaRow,
    folder: &IdeaFolder,
) -> Result<Step> {
    store.immediate(|tx| trusted_approval(tx, ctx.plan.guard, folder, &idea.id))?;
    Ok(Step::Stayed { wrote: true })
}

fn specifying(
    store: &mut Store,
    ctx: &Dispatch<'_>,
    idea: &IdeaRow,
    folder: &IdeaFolder,
) -> Result<Step> {
    if folder.read_spec().is_err() || folder.read_requirements().is_err() {
        return Ok(Step::Stayed { wrote: false });
    }
    store.immediate(|tx| {
        move_idea(
            tx,
            ctx,
            &idea.id,
            "specifying",
            "ticketing",
            "spec.md and requirements.yaml validate",
        )
    })
}

fn ticketing(store: &mut Store, ctx: &Dispatch<'_>, idea: &IdeaRow, dir: &Path) -> Result<Step> {
    if !dir.join(PLAN_YAML).exists() {
        return Ok(Step::Stayed { wrote: false });
    }
    Ok(match validate_plan(store, &ctx.plan, &idea.id)? {
        Verdict::Committed { .. } => Step::Moved("in_progress".to_string()),
        Verdict::Refused { .. } => Step::Stayed { wrote: true },
    })
}

fn move_idea(
    tx: &Connection,
    ctx: &Dispatch<'_>,
    id: &str,
    from: &str,
    to: &str,
    text: &str,
) -> Result<Step> {
    let now: String = tx.query_row("SELECT state FROM ideas WHERE id = ?1", [id], |r| r.get(0))?;
    if now != from {
        return Ok(Step::Stayed { wrote: false });
    }
    transition(
        tx,
        ctx.plan.guard,
        Entity::Idea,
        id,
        to,
        by_fleetd(text),
        ctx.plan.refusals,
    )?;
    Ok(Step::Moved(to.to_string()))
}

fn fingerprint(dir: &Path, ctx: &Dispatch<'_>) -> Vec<(String, String)> {
    let rule = Rule {
        root: dir,
        owner: ctx.plan.owner,
        limit: FILE_LIMIT,
    };
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if rel == VERDICT_FILE {
                continue;
            }
            let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
            if is_dir {
                dirs.push(path);
                continue;
            }
            let seen = match read_protected(&path, &rule) {
                Ok(bytes) => sha256_bytes(&bytes).hex(),
                Err(r) => r.reason,
            };
            out.push((rel, seen));
        }
    }
    out.sort();
    out
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
