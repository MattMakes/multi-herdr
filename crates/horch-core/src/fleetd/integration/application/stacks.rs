//! The stacks: stack the younger feature of a conflict and make its
//! resolver task.
//!
//! Use cases `StackFeature` and `OpenResolverTask`
//! (`docs/specs/fleetd/design.md`, "Between features", "Stack on evidence"
//! and "Starting values"; `ddd/08-code/integration/design.md`). When the
//! trial merge of 2 in-flight features conflicts, [`stack_conflicts`] sets
//! `stacked_on` of the younger feature to the older one, 1 time, and adds
//! its resolver tasks in the same transaction:
//!
//! - a resolver task has a code kind (`feature`), so it follows the code
//!   path, and it lands on the branch of the younger feature;
//! - its ticket holds the conflict in `zdiff3` form, the requirements and
//!   the unit tests of both features, and the rule of 1 commit for each
//!   requirement;
//! - it belongs to the youngest idea in flight of the younger feature
//!   (FR-12; MAP.md of fleetd-p6, Q17), and goes in through
//!   `add_planned_tasks` with a plan entry that fleetd writes;
//! - a conflict over the size limits is 1 resolver task for each file.
//!
//! A conflict of tips that are no longer the tips of the 2 features is old,
//! and it changes nothing. With no idea in flight, nothing is written: no
//! stack edge without its resolver task.

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

/// The plan of an idea, in its folder: the file that a resolver task reads
/// first.
const PLAN_YAML: &str = "plan.yaml";

/// The state word of a cancelled idea and of a cancelled task.
const CANCELLED: &str = "cancelled";

/// The kind of a resolver task: a code kind.
const RESOLVER_KIND: WorkKind = WorkKind::Feature;

/// The git reads of [`stack_conflicts`]. Each is short and local: no
/// working tree, no network, and no write.
pub trait StackGit {
    /// The tip of the ref `name`, or `None` when it does not exist.
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;

    /// The conflict of `file` when `upper_tip` merges `lower_tip`, in
    /// `zdiff3` form.
    fn conflict(&self, repo: &Path, lower_tip: &str, upper_tip: &str, file: &str)
        -> Result<String>;
}

/// What the stacks need besides the ledger.
#[derive(Debug, Clone, Copy)]
pub struct Stacking<'a> {
    /// The repository of the features.
    pub repo: &'a Path,
    /// The `ideas/` folder of the fleet root.
    pub ideas: &'a Path,
    /// The profile of a resolver task.
    pub profile: &'a str,
    pub planner: StackPlanner,
    /// The generation of fleetd, or `None` outside fleetd.
    pub guard: Option<Generation>,
}

/// The outcome of 1 conflict of 2 features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `upper` is now on `lower`, with the resolver tasks `tasks` of the
    /// idea `idea`.
    Stacked {
        upper: String,
        lower: String,
        idea: String,
        tasks: Vec<String>,
    },
    /// `upper` was on `lower` already. Nothing changed.
    AlreadyStacked { upper: String, lower: String },
    /// `upper` is on the feature `on`. Its conflict waits for the end of
    /// that edge. Nothing changed.
    Held { upper: String, on: String },
    /// `upper` has no idea in flight to own a resolver task. Nothing
    /// changed.
    NoIdea { upper: String },
    /// The trial merge is of old tips, or of a feature that is not in
    /// flight. Nothing changed.
    Stale,
}

/// A feature of a conflict, from the ledger and git.
struct Side {
    id: String,
    branch: String,
    created_at: String,
    stacked_on: Option<String>,
}

/// Act on each conflicting trial merge of 2 in-flight features, in `tx`.
/// Returns the outcome of each, oldest trial merge first.
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

/// Act on the conflicting trial merge `row`, in `tx`: stack the younger
/// feature on the older one and add its resolver tasks, or change nothing.
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

/// Each conflicting trial merge of 2 in-flight features that is the newest
/// trial merge of its 2 refs, oldest first, with the id of its younger
/// feature: the feature that a stack puts on top. An older trial merge of
/// the same 2 refs is of old tips, so the pass reads git for it no more.
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

/// The facts of `f` that the planner reads.
fn view(f: &Side) -> StackSide<'_> {
    StackSide {
        id: &f.id,
        created_at: &f.created_at,
        stacked_on: f.stacked_on.as_deref(),
    }
}

/// The in-flight feature whose branch is the ref `ref_name`, or `None`.
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

/// The states of an idea that is not in flight (design, "A merge with
/// ideas in flight").
const NOT_IN_FLIGHT: [&str; 4] = ["integrating", "merged", "parked", "cancelled"];

/// The youngest idea in flight of `feature`, or `None`.
pub(crate) fn youngest_idea_in_flight(tx: &Connection, feature: &str) -> Result<Option<String>> {
    Ok(ideas_store::of_feature(tx, feature)?
        .into_iter()
        .filter(|i| !NOT_IN_FLIGHT.contains(&i.state.as_str()))
        .max_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)))
        .map(|i| i.id))
}

/// The requirements and the unit tests of a feature, from the ledger.
struct Facts {
    requirements: Vec<Requirement>,
    tests: Vec<String>,
}

impl Facts {
    /// The requirements of each idea of `feature` that is not cancelled,
    /// and the check of each task of its run that is not cancelled, each 1
    /// time. The ledger maps no requirement to a file, so each one is
    /// given.
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

/// `StackFeature`: set `stacked_on` of `upper` to `lower`, with its event,
/// in `tx`. Only a feature in flight with no lower feature moves.
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

/// The plan entry of the resolver task `index` of `conflicts`, which fleetd
/// writes for the idea `idea`.
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
    }
}

/// The event `resolver-task-opened` of the resolver task `task_id` of
/// `feature`, in `tx`.
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
