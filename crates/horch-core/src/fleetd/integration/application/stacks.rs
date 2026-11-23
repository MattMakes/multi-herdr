use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::integration::adapters::trial_merges_store::{self as store, TrialMergeRow};
use crate::fleetd::integration::domain::stack::{
    resolver_ticket, resolver_title, Conflict, Decision, Requirement, ResolverSide, StackPlanner,
    StackSide,
};
use crate::fleetd::task_execution::application::planned_tasks::{add_planned_tasks, PlannedTask};
use crate::taskledger::model::{EventKind, WorkKind};
use crate::taskledger::{runs, tasks, util};

const PLAN_YAML: &str = "plan.yaml";

const CANCELLED: &str = "cancelled";

const RESOLVER_KIND: WorkKind = WorkKind::Feature;

pub trait StackGit {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;

    fn conflict(&self, repo: &Path, lower_tip: &str, upper_tip: &str, file: &str)
        -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
pub struct Stacking<'a> {
    pub repo: &'a Path,
    pub ideas: &'a Path,
    pub profile: &'a str,
    pub planner: StackPlanner,
    pub guard: Option<Generation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Stacked {
        upper: String,
        lower: String,
        idea: String,
        tasks: Vec<String>,
    },
    AlreadyStacked {
        upper: String,
        lower: String,
    },
    Held {
        upper: String,
        on: String,
    },
    NoIdea {
        upper: String,
    },
    Stale,
}

struct Side {
    id: String,
    branch: String,
    created_at: String,
    stacked_on: Option<String>,
}

pub fn stack_conflicts(
    tx: &Connection,
    s: &Stacking<'_>,
    git: &dyn StackGit,
    cause: Cause<'_>,
) -> Result<Vec<Outcome>> {
    let mut stmt = tx.prepare(
        "SELECT t.left_tip, t.right_tip FROM trial_merges t
         JOIN features l ON 'refs/heads/' || l.branch = t.left_ref
         JOIN features r ON 'refs/heads/' || r.branch = t.right_ref
         WHERE t.clean = 0 AND l.state IN ('open', 'ready') AND r.state IN ('open', 'ready')
         ORDER BY t.at, t.left_tip, t.right_tip",
    )?;
    let keys = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("reading the conflicting trial merges")?;
    let mut out = Vec::with_capacity(keys.len());
    for (left, right) in keys {
        let Some(row) = store::get(tx, &left, &right)? else {
            continue;
        };
        out.push(stack_conflict(tx, s, git, &row, cause)?);
    }
    Ok(out)
}

pub fn stack_conflict(
    tx: &Connection,
    s: &Stacking<'_>,
    git: &dyn StackGit,
    row: &TrialMergeRow,
    cause: Cause<'_>,
) -> Result<Outcome> {
    if row.clean != Some(false) {
        return Ok(Outcome::Stale);
    }
    let (Some(left), Some(right)) = (side(tx, &row.left_ref)?, side(tx, &row.right_ref)?) else {
        return Ok(Outcome::Stale);
    };
    let current = |name: &str, tip: &str| -> Result<bool> {
        Ok(git.tip(s.repo, name)?.as_deref() == Some(tip))
    };
    if !current(&row.left_ref, &row.left_tip)? || !current(&row.right_ref, &row.right_tip)? {
        return Ok(Outcome::Stale);
    }
    let (lower_id, upper_id) = match s.planner.decide(&view(&left), &view(&right)) {
        Decision::Stack { lower, upper } => (lower, upper),
        Decision::Stacked { lower, upper } => {
            return Ok(Outcome::AlreadyStacked { upper, lower });
        }
        Decision::Held { upper, on } => return Ok(Outcome::Held { upper, on }),
    };
    let tips = BTreeMap::from([
        (left.id.clone(), (&left, row.left_tip.as_str())),
        (right.id.clone(), (&right, row.right_tip.as_str())),
    ]);
    let (lower, lower_tip) = tips[&lower_id];
    let (upper, upper_tip) = tips[&upper_id];
    let Some(idea) = youngest_idea_in_flight(tx, &upper.id)? else {
        return Ok(Outcome::NoIdea { upper: upper_id });
    };
    let mut conflicts = Vec::with_capacity(row.conflict_files.len());
    for file in &row.conflict_files {
        let zdiff3 = git.conflict(s.repo, lower_tip, upper_tip, file)?;
        conflicts.push(Conflict {
            file: file.clone(),
            zdiff3,
        });
    }
    let lower_facts = Facts::read(tx, &lower.id)?;
    let upper_facts = Facts::read(tx, &upper.id)?;
    let lower_side = lower_facts.side(lower, lower_tip);
    let upper_side = upper_facts.side(upper, upper_tip);
    stack(tx, s.guard, &upper.id, &lower.id, cause)?;
    let groups = s.planner.split(conflicts);
    let planned: Vec<PlannedTask> = groups
        .iter()
        .enumerate()
        .map(|(i, group)| resolver_task(s, &idea, i, &lower_side, &upper_side, group))
        .collect();
    let run = runs::of_feature(tx, &upper.id)?
        .with_context(|| format!("the feature {} has no internal run", upper.id))?;
    let ids = add_planned_tasks(tx, s.guard, &run, &idea, &planned, cause)?;
    let mut tasks = Vec::with_capacity(ids.len());
    for ((_, id), group) in ids.into_iter().zip(&groups) {
        opened(tx, s.guard, &upper.id, &id, group, cause)?;
        tasks.push(id);
    }
    Ok(Outcome::Stacked {
        upper: upper_id,
        lower: lower_id,
        idea,
        tasks,
    })
}

pub fn newest_conflicts(
    tx: &Connection,
    planner: &StackPlanner,
) -> Result<Vec<(TrialMergeRow, String)>> {
    let mut stmt = tx.prepare(
        "SELECT t.left_tip, t.right_tip FROM trial_merges t
         JOIN features l ON 'refs/heads/' || l.branch = t.left_ref
         JOIN features r ON 'refs/heads/' || r.branch = t.right_ref
         WHERE t.clean = 0 AND l.state IN ('open', 'ready') AND r.state IN ('open', 'ready')
             AND NOT EXISTS (SELECT 1 FROM trial_merges n
                 WHERE n.left_ref = t.left_ref AND n.right_ref = t.right_ref AND n.at > t.at)
         ORDER BY t.at, t.left_tip, t.right_tip",
    )?;
    let keys = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("reading the newest conflicting trial merges")?;
    let mut out = Vec::with_capacity(keys.len());
    for (left, right) in keys {
        let Some(row) = store::get(tx, &left, &right)? else {
            continue;
        };
        let (Some(l), Some(r)) = (side(tx, &row.left_ref)?, side(tx, &row.right_ref)?) else {
            continue;
        };
        let upper = match planner.decide(&view(&l), &view(&r)) {
            Decision::Stack { upper, .. }
            | Decision::Stacked { upper, .. }
            | Decision::Held { upper, .. } => upper,
        };
        out.push((row, upper));
    }
    Ok(out)
}

fn view(f: &Side) -> StackSide<'_> {
    StackSide {
        id: &f.id,
        created_at: &f.created_at,
        stacked_on: f.stacked_on.as_deref(),
    }
}

fn side(tx: &Connection, ref_name: &str) -> Result<Option<Side>> {
    tx.query_row(
        "SELECT id, branch, created_at, stacked_on FROM features
         WHERE 'refs/heads/' || branch = ?1 AND state IN ('open', 'ready')",
        [ref_name],
        |r| {
            Ok(Side {
                id: r.get(0)?,
                branch: r.get(1)?,
                created_at: r.get(2)?,
                stacked_on: r.get(3)?,
            })
        },
    )
    .optional()
    .with_context(|| format!("reading the feature of {ref_name}"))
}

const NOT_IN_FLIGHT: [&str; 4] = ["integrating", "merged", "parked", "cancelled"];

pub(crate) fn youngest_idea_in_flight(tx: &Connection, feature: &str) -> Result<Option<String>> {
    Ok(ideas_store::of_feature(tx, feature)?
        .into_iter()
        .filter(|i| !NOT_IN_FLIGHT.contains(&i.state.as_str()))
        .max_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)))
        .map(|i| i.id))
}

struct Facts {
    requirements: Vec<Requirement>,
    tests: Vec<String>,
}

impl Facts {
    fn read(tx: &Connection, feature: &str) -> Result<Facts> {
        let mut requirements = Vec::new();
        for idea in ideas_store::of_feature(tx, feature)? {
            if idea.state == CANCELLED {
                continue;
            }
            for (id, text) in ideas_store::requirements(tx, &idea.id)? {
                requirements.push(Requirement {
                    idea: idea.id.clone(),
                    id,
                    text,
                });
            }
        }
        let mut tests: Vec<String> = Vec::new();
        if let Some(run) = runs::of_feature(tx, feature)? {
            for task in tasks::of_run(tx, &run.run_id)? {
                let Some(check) = task.land_check else {
                    continue;
                };
                if task.status != CANCELLED && !tests.contains(&check) {
                    tests.push(check);
                }
            }
        }
        Ok(Facts {
            requirements,
            tests,
        })
    }

    fn side<'a>(&'a self, f: &'a Side, tip: &'a str) -> ResolverSide<'a> {
        ResolverSide {
            feature: &f.id,
            branch: &f.branch,
            tip,
            requirements: &self.requirements,
            tests: &self.tests,
        }
    }
}

fn stack(
    tx: &Connection,
    guard: Option<Generation>,
    upper: &str,
    lower: &str,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let changed = tx.execute(
        "UPDATE features SET stacked_on = ?2, updated_at = ?3
         WHERE id = ?1 AND stacked_on IS NULL AND state IN ('open', 'ready')",
        params![upper, lower, util::now()],
    )?;
    if changed != 1 {
        bail!("the feature {upper} is not in flight with no lower feature");
    }
    let data = json!({ "stacked_on": lower });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        upper,
        cause,
        &data,
    )?;
    Ok(())
}

fn resolver_task(
    s: &Stacking<'_>,
    idea: &str,
    index: usize,
    lower: &ResolverSide<'_>,
    upper: &ResolverSide<'_>,
    conflicts: &[Conflict],
) -> PlannedTask {
    let key = format!("resolve-{}-{}-{}", upper.feature, lower.feature, index + 1);
    let title = resolver_title(lower.feature, upper.feature, conflicts);
    let files: Vec<&str> = conflicts.iter().map(|c| c.file.as_str()).collect();
    let covers: Vec<String> = [lower, upper]
        .iter()
        .flat_map(|side| side.requirements.iter())
        .map(|r| format!("{}:{}", r.idea, r.id))
        .collect();
    let spec = json!({
        "key": key,
        "title": title,
        "ticket": null,
        "kind": RESOLVER_KIND.as_str(),
        "profile": s.profile,
        "covers": covers,
        "inputs": files,
        "outputs": files,
        "acceptance": [],
        "after": [],
        "judges": null,
        "budget": null,
        "reason": null,
        "no_decisions": false,
        "resolves": {
            "lower": lower.feature,
            "lower_tip": lower.tip,
            "upper": upper.feature,
            "upper_tip": upper.tip,
            "files": files,
        },
    });
    PlannedTask {
        key,
        title,
        task_file: s
            .ideas
            .join(idea)
            .join(PLAN_YAML)
            .to_string_lossy()
            .into_owned(),
        ticket_snapshot: Some(resolver_ticket(lower, upper, conflicts)),
        work_kind: Some(RESOLVER_KIND),
        profile: s.profile.to_string(),
        land_check: None,
        spec_json: spec.to_string(),
        after: Vec::new(),
        judges: None,
        own: Vec::new(),
    }
}

fn opened(
    tx: &Connection,
    guard: Option<Generation>,
    feature: &str,
    task_id: &str,
    conflicts: &[Conflict],
    cause: Cause<'_>,
) -> Result<()> {
    let files: Vec<&str> = conflicts.iter().map(|c| c.file.as_str()).collect();
    let data = json!({
        "feature_id": feature,
        "resolver_task": task_id,
        "conflict": files,
    });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        feature,
        cause,
        &data,
    )?;
    Ok(())
}
