//! The end of a stack edge: rescue, rebase and free the upper feature when
//! its lower feature is `merged` or `abandoned`.
//!
//! `docs/specs/fleetd/design.md`, "Between features" (A stack edge ends with
//! its lower feature; Continuous rebase), I10, FM-42;
//! `docs/tickets/fleetd-p6/38-stack-edges.md`. The owner of each step is
//! fleetd. [`follow`] is the pass part. For each `open` or `ready` feature
//! whose `stacked_on` names a `merged` or `abandoned` feature:
//!
//! 1. The target: the fetched primary tip. For an `abandoned` lower feature
//!    the rebase also takes out each commit of the tip of its branch
//!    (`git rebase --onto <primary> <lower tip>`). A lower branch that git
//!    does not have puts 1 item on the needs-you list, and nothing moves.
//! 2. When the upper branch is on the target (`rebase::is_on`), fleetd
//!    clears `stacked_on`, with its event. The branch then has a new tip with
//!    no gate result, so the fast gate (of each new bundle) and the full gate
//!    run again by their own rules, and the rebase rule queues the push of
//!    the new tip. The push moves the base of the pull request to the
//!    primary branch (`pr = gh`), or changes the needs-you item of the
//!    compare URL (`pr = manual`).
//! 3. Else, as the rebase rule of Phase 4: no rebase while a rebase of the
//!    branch is active or the gate tree is busy (FM-37); the gate tree is
//!    made first; the rescue ref of the upper tip is written after the
//!    commit of the pass and recorded on the branch row (I10); then 1
//!    `rebase` job with the 2 tips and the cut as its input key.
//! 4. A run of that key that ended in a conflict opens 1 resolver task, on
//!    the path of the resolver tasks of a stack (`stacks.rs`): the youngest
//!    idea in flight of the upper feature owns it, and it lands on the
//!    branch of the upper feature. With no idea in flight, 1 item goes on
//!    the needs-you list. The edge stays until the branch is on the target.
//!
//! Each step reads the rows and git again, so a crash at any point repeats
//! the step. A pull request of the lower feature that is closed and not
//! merged ends no edge: the feature is still in flight, and the fetch job
//! puts the pull request on the needs-you list.

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::jobs::{result_data, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::features::{self, Tips};
use crate::fleetd::integration::application::rebase::{
    self, gate_tree_busy, is_on, rebase_key, rebase_key_cut, Effect, Rebased, Rebases,
};
use crate::fleetd::integration::application::stacks::youngest_idea_in_flight;
use crate::fleetd::task_execution::application::planned_tasks::{
    add_planned_tasks, committed_tasks, PlannedTask,
};
use crate::taskledger::model::{Actor, EventKind, WorkKind};
use crate::taskledger::{runs, util};

/// The plan of an idea, in its folder: the file that a resolver task reads
/// first.
const PLAN_YAML: &str = "plan.yaml";

/// The kind of a resolver task: a code kind.
const RESOLVER_KIND: WorkKind = WorkKind::Feature;

/// The kind of the needs-you item of a stack edge that cannot end.
pub const STACK_EDGE_KIND: &str = "stack_edge";

/// The key of the needs-you item of the stack edge of `upper`.
pub fn stack_edge_key(upper: &str) -> String {
    format!("{STACK_EDGE_KIND}:{upper}")
}

/// What [`follow`] works with: the rebase rule, and the facts of a
/// resolver task.
pub struct Edges<'a> {
    pub rebases: &'a Rebases<'a>,
    /// The `ideas/` folder of the fleet root.
    pub ideas: &'a Path,
    /// The profile of a resolver task.
    pub profile: &'a str,
}

/// How the lower feature of an edge ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Merged,
    Abandoned,
}

impl End {
    fn as_str(self) -> &'static str {
        match self {
            End::Merged => "merged",
            End::Abandoned => "abandoned",
        }
    }
}

/// 1 stack edge whose lower feature ended.
struct Edge {
    upper: String,
    lower: String,
    end: End,
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The pass part, in `tx`, at `clock`: act on each stack edge whose lower
/// feature is `merged` or `abandoned`. Gives the rescue effects to run
/// after the commit (`rebase::run`, then `rebase::observe`).
pub fn follow(e: &Edges<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Vec<Effect>> {
    follow_in(e, tx, clock, None)
}

/// The upper feature of each stack edge whose lower feature is `merged` or
/// `abandoned`, by id.
pub fn ended_uppers(tx: &Connection) -> Result<Vec<String>> {
    Ok(ended_edges(tx)?.into_iter().map(|e| e.upper).collect())
}

/// [`follow`] for the stack edge of the upper feature `upper` only, or for
/// each edge with `None`: the pass gives each upper feature the profile of
/// its own resolver tasks.
pub fn follow_in(
    e: &Edges<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    upper: Option<&str>,
) -> Result<Vec<Effect>> {
    let mut effects = Vec::new();
    let edges = ended_edges(tx)?
        .into_iter()
        .filter(|edge| upper.is_none_or(|u| u == edge.upper));
    for edge in edges {
        let effect = follow_one(e, tx, clock, &edge)
            .with_context(|| format!("the stack edge of {} on {}", edge.upper, edge.lower))?;
        effects.extend(effect);
    }
    Ok(effects)
}

/// Each `open` or `ready` feature on a `merged` or `abandoned` feature, by
/// id of the upper feature.
fn ended_edges(tx: &Connection) -> Result<Vec<Edge>> {
    let mut stmt = tx.prepare(
        "SELECT u.id, l.id, l.state FROM features u JOIN features l ON l.id = u.stacked_on
         WHERE u.state IN ('open', 'ready') AND l.state IN ('merged', 'abandoned')
         ORDER BY u.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let state: String = r.get(2)?;
        Ok(Edge {
            upper: r.get(0)?,
            lower: r.get(1)?,
            end: if state == "merged" {
                End::Merged
            } else {
                End::Abandoned
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn follow_one(
    e: &Edges<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    edge: &Edge,
) -> Result<Option<Effect>> {
    let r = e.rebases;
    let upper = edge.upper.as_str();
    let Some(run) = runs::feature_run(tx, upper)? else {
        return Ok(None);
    };
    let row = rows::feature(tx, upper)?.context("no such feature in the ledger")?;
    let Some(branch) = row.branch.clone() else {
        return Ok(None);
    };
    let Ok(tips) = features::read_tips(r.git, tx, &run.project_root, &row)? else {
        return Ok(None);
    };
    let cut = match edge.end {
        End::Merged => None,
        End::Abandoned => match lower_tip(e, tx, &tips, &edge.lower)? {
            Some(tip) => Some(tip),
            None => {
                let text = format!(
                    "Feature {upper} is on the abandoned feature {}, and git has no branch of \
                     it, so fleetd cannot take its commits out of {branch}. Rebase {branch} \
                     on the primary branch yourself.",
                    edge.lower
                );
                wait(e, tx, upper, &text)?;
                return Ok(None);
            }
        },
    };
    if is_on(
        r.refs,
        &tips.repo,
        &tips.feature,
        &tips.primary,
        cut.as_deref(),
    )? {
        return end_edge(e, tx, edge).map(|()| None);
    }
    let key = match cut.as_deref() {
        Some(cut) => rebase_key_cut(&tips.feature, &tips.primary, cut),
        None => rebase_key(&tips.feature, &tips.primary),
    };
    match jobs_store::latest(tx, Kind::Rebase, &branch, &key)? {
        Some(last) if last.state == State::Failed => {
            return conflicted(e, tx, edge, &branch, &last).map(|()| None);
        }
        Some(last) if last.state == State::Passed => return Ok(None),
        _ => {}
    }
    if jobs_store::active(tx, Kind::Rebase, &branch)?.is_some() || gate_tree_busy(tx, upper)? {
        return Ok(None);
    }
    if !rebase::gate_tree_ready(r, tx, clock, &run, upper, &branch, &tips)? {
        return Ok(None);
    }
    if !rebase::rescued(r, tx, &branch, &tips)? {
        return Ok(Some(Effect::Rescue {
            repo: tips.repo.clone(),
            branch,
            tip: tips.feature.clone(),
        }));
    }
    rebase::queue(r, tx, clock, &branch, &tips.repo, &key)?;
    Ok(None)
}

/// The tip of the branch of the feature `lower`, or `None` when it has no
/// branch in git.
fn lower_tip(e: &Edges<'_>, tx: &Connection, tips: &Tips, lower: &str) -> Result<Option<String>> {
    let Some(branch) = rows::feature(tx, lower)?.and_then(|f| f.branch) else {
        return Ok(None);
    };
    e.rebases
        .git
        .tip(&tips.repo, &format!("refs/heads/{branch}"))
}

/// Put the stack edge of `upper` on the needs-you list with `text`.
fn wait(e: &Edges<'_>, tx: &Connection, upper: &str, text: &str) -> Result<()> {
    let key = stack_edge_key(upper);
    let item = NewItem {
        key: &key,
        kind: STACK_EDGE_KIND,
        entity: Entity::Feature,
        entity_id: upper,
        text,
    };
    e.rebases.jobs.needs_you.open_item(tx, &item)
}

/// Clear `stacked_on` of the upper feature of `edge`, with its event, and
/// end the needs-you item of the edge.
fn end_edge(e: &Edges<'_>, tx: &Connection, edge: &Edge) -> Result<()> {
    let guard = e.rebases.jobs.guard;
    check_writable(tx, guard)?;
    let changed = tx.execute(
        "UPDATE features SET stacked_on = NULL, updated_at = ?3
         WHERE id = ?1 AND stacked_on = ?2",
        params![edge.upper, edge.lower, util::now()],
    )?;
    if changed != 1 {
        bail!("the feature {} is not on {}", edge.upper, edge.lower);
    }
    let why = format!(
        "the lower feature {} is {}, and the branch is on the primary tip",
        edge.lower,
        edge.end.as_str()
    );
    let data = json!({
        "stacked_on": null,
        "lower": edge.lower,
        "lower_state": edge.end.as_str(),
    });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        &edge.upper,
        by_fleetd(&why),
        &data,
    )?;
    e.rebases
        .jobs
        .needs_you
        .resolve_item(tx, &stack_edge_key(&edge.upper))?;
    Ok(())
}

/// Act on the failed rebase run `last` of the upper branch `branch`: a
/// conflict opens 1 resolver task, 1 time for the run. Another failure
/// changes nothing: its tips moved, so a new key follows.
fn conflicted(
    e: &Edges<'_>,
    tx: &Connection,
    edge: &Edge,
    branch: &str,
    last: &JobRow,
) -> Result<()> {
    let result = result_data(last).and_then(|d| serde_json::from_value::<Rebased>(d).ok());
    let Some(Rebased::Conflict { files }) = result else {
        return Ok(());
    };
    let upper = edge.upper.as_str();
    let run = runs::feature_run(tx, upper)?
        .with_context(|| format!("the feature {upper} has no internal run"))?;
    let key = format!("rebase-{upper}-{}", last.id);
    if resolver_exists(tx, upper, &key)? {
        return Ok(());
    }
    let Some(idea) = youngest_idea_in_flight(tx, upper)? else {
        let text = format!(
            "The rebase of feature {upper} after the end of its stack edge conflicts in {}, \
             and the feature has no idea in flight to own a resolver task.",
            files.join(", ")
        );
        return wait(e, tx, upper, &text);
    };
    let rescue = rows::branch(tx, branch)?.and_then(|b| b.rescue_ref);
    let rebase = RebaseFacts {
        edge,
        branch,
        job: last,
        files: &files,
        rescue: rescue.as_deref(),
    };
    let planned = resolver_task(e, &idea, &key, &rebase);
    let guard = e.rebases.jobs.guard;
    let why = format!("the rebase job {} of {branch} conflicts", last.id);
    let ids = add_planned_tasks(tx, guard, &run, &idea, &[planned], by_fleetd(&why))?;
    let data = json!({
        "feature_id": upper,
        "resolver_task": ids.first().map(|(_, id)| id),
        "conflict": files,
        "rebase_job": last.id,
    });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        upper,
        by_fleetd(&why),
        &data,
    )?;
    Ok(())
}

/// Whether an idea of `feature` has a committed task with the plan key
/// `key`.
fn resolver_exists(tx: &Connection, feature: &str, key: &str) -> Result<bool> {
    for idea in ideas_store::of_feature(tx, feature)? {
        for task in committed_tasks(tx, &idea.id)? {
            let spec: serde_json::Value = serde_json::from_str(&task.spec_json)
                .with_context(|| format!("the plan entry of task {}", task.task_id))?;
            if spec["key"] == key {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The facts of 1 conflicted rebase of an upper branch.
struct RebaseFacts<'a> {
    edge: &'a Edge,
    branch: &'a str,
    job: &'a JobRow,
    files: &'a [String],
    rescue: Option<&'a str>,
}

/// The plan entry of the resolver task `key` of the conflicted rebase
/// `f`, which fleetd writes for the idea `idea`.
fn resolver_task(e: &Edges<'_>, idea: &str, key: &str, f: &RebaseFacts<'_>) -> PlannedTask {
    let upper = f.edge.upper.as_str();
    let parts = rebase::read_key(&f.job.input_key);
    let title = format!(
        "Resolve the rebase of {upper} after its lower feature {} ended: {}",
        f.edge.lower,
        f.files.join(", ")
    );
    let spec = json!({
        "key": key,
        "title": title,
        "ticket": null,
        "kind": RESOLVER_KIND.as_str(),
        "profile": e.profile,
        "covers": [],
        "inputs": f.files,
        "outputs": f.files,
        "acceptance": [],
        "after": [],
        "judges": null,
        "budget": null,
        "reason": null,
        "no_decisions": false,
        "rebases": {
            "upper": upper,
            "lower": f.edge.lower,
            "lower_state": f.edge.end.as_str(),
            "tip": parts.map(|k| k.tip),
            "onto": parts.map(|k| k.onto),
            "cut": parts.and_then(|k| k.cut),
            "job": f.job.id,
            "files": f.files,
        },
    });
    PlannedTask {
        key: key.to_string(),
        title: title.clone(),
        task_file: e
            .ideas
            .join(idea)
            .join(PLAN_YAML)
            .to_string_lossy()
            .into_owned(),
        ticket_snapshot: Some(resolver_ticket(&title, f)),
        work_kind: Some(RESOLVER_KIND),
        profile: e.profile.to_string(),
        land_check: None,
        spec_json: spec.to_string(),
        after: Vec::new(),
        judges: None,
    }
}

/// The ticket of the resolver task of the conflicted rebase `f`.
fn resolver_ticket(title: &str, f: &RebaseFacts<'_>) -> String {
    let upper = f.edge.upper.as_str();
    let lower = f.edge.lower.as_str();
    let parts = rebase::read_key(&f.job.input_key);
    let onto = parts.map_or("the primary tip", |k| k.onto);
    let tip = parts.map_or("its tip", |k| k.tip);
    let (how, goal) = match parts.and_then(|k| k.cut) {
        Some(cut) => (
            format!(" without the commits of {lower} at `{cut}`"),
            format!(" and holds no work of {lower}"),
        ),
        None => (String::new(), String::new()),
    };
    let mut out = format!(
        "# {title}\n\n**Kind:** feature\n\n\
         Feature {upper} was on feature {lower}, which is {}. \
         fleetd rebased the branch `{}` at `{tip}` onto the primary tip `{onto}`{how}, \
         and the rebase conflicts. The branch did not move.",
        f.edge.end.as_str(),
        f.branch,
    );
    if let Some(rescue) = f.rescue {
        out.push_str(&format!(" Its old tip is in `{rescue}`."));
    }
    out.push_str(&format!(
        "\n\n## Rules\n\n\
         - Resolve the conflict of each file below, so that the branch of {upper} holds its \
         work on the primary tip{goal}.\n\
         - Run the unit tests of {upper}.\n\
         - This task lands on the branch of {upper}.\n\n## Files\n\n"
    ));
    for file in f.files {
        out.push_str(&format!("- `{file}`\n"));
    }
    out
}
