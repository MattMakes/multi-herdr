//! The use cases of task branches: the rebase resolver task and the branch invariants (tickets 42 and 44).
//!
//! `docs/specs/fleetd/design.md`, "Between features": Continuous rebase (a
//! conflict becomes a resolver task) and Starting values (a resolver task
//! covers at most `resolver_max_lines` changed lines and
//! `resolver_max_files` files, else fleetd splits it by file).
//!
//! [`open_rebase_resolvers`] reads the last `rebase` run of each live
//! feature branch. When it ended with a conflict and the branch is still at
//! the tip of its input key, fleetd adds the resolver tasks of the conflict
//! to the run of the feature through `tasks::add`: a code kind, with a
//! ticket that fleetd writes (the conflict files in `zdiff3` form, the 2
//! tips and the tests of both sides). A task of the run lands on the
//! feature branch as any task. Each ticket starts with the line of its pair
//! of tips, so the same pair never opens a second set, also after a cancel.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::result_data;
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::features;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::application::rebase::{read_key, Rebased};
use crate::fleetd::integration::application::stacks::StackGit;
use crate::fleetd::integration::domain::stack::{Conflict, StackPlanner};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkKind};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::tasks::{self, NewTask, Task};

/// The kind of a resolver task: a code kind.
const RESOLVER_KIND: WorkKind = WorkKind::Feature;

/// What [`open_rebase_resolvers`] works with.
pub struct Resolvers<'a> {
    /// The tips of the branches and the `fleet.toml` of a tip.
    pub git: &'a dyn IntegrationGitGateway,
    /// The conflict of 1 file in `zdiff3` form.
    pub conflicts: &'a dyn StackGit,
    /// `resolver_max_lines` and `resolver_max_files` of `fleet.toml`.
    pub planner: StackPlanner,
    /// The profile of a resolver task.
    pub profile: &'a str,
    /// The generation of fleetd, or `None` outside fleetd.
    pub guard: Option<Generation>,
}

/// The resolver tasks that 1 rebase conflict opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub feature_id: String,
    pub branch: String,
    /// The tip of the branch that the rebase did not move.
    pub tip: String,
    /// The primary tip that the rebase moved onto.
    pub onto: String,
    /// The task ids, 1 for each file group.
    pub tasks: Vec<String>,
}

/// For each live feature branch whose last rebase run ended with a
/// conflict of its current tip, add its resolver tasks to the run of the
/// feature, in `tx`, 1 time for each pair of tips.
pub fn open_rebase_resolvers(
    tx: &Connection,
    r: &Resolvers<'_>,
    cause: Cause<'_>,
) -> Result<Vec<Opened>> {
    let mut out = Vec::new();
    for (feature_id, branch, _) in rows::live_feature_branches(tx)? {
        if let Some(opened) = open_one(tx, r, &feature_id, &branch, cause)? {
            out.push(opened);
        }
    }
    Ok(out)
}

fn open_one(
    tx: &Connection,
    r: &Resolvers<'_>,
    feature_id: &str,
    branch: &str,
    cause: Cause<'_>,
) -> Result<Option<Opened>> {
    let Some(last) = jobs_store::last(tx, Kind::Rebase, branch)? else {
        return Ok(None);
    };
    if !matches!(last.state, State::Passed | State::Failed) {
        return Ok(None);
    }
    let result = result_data(&last).and_then(|d| serde_json::from_value::<Rebased>(d).ok());
    let Some(Rebased::Conflict { files }) = result else {
        return Ok(None);
    };
    let Some((tip, onto)) = read_key(&last.input_key) else {
        return Ok(None);
    };
    let Some(run) = runs::feature_run(tx, feature_id)? else {
        return Ok(None);
    };
    let repo = Path::new(&run.project_root);
    // A branch that moved after the run makes a new rebase run: this
    // conflict is old.
    if r.git.tip(repo, &format!("refs/heads/{branch}"))?.as_deref() != Some(tip) {
        return Ok(None);
    }
    let pair = pair_line(branch, tip, onto);
    let run_tasks = tasks::of_run(tx, &run.run_id)?;
    let opened_before = run_tasks.iter().any(|t| {
        t.ticket_snapshot
            .as_deref()
            .is_some_and(|s| s.contains(&pair))
    });
    if opened_before {
        return Ok(None);
    }
    let mut conflicts = Vec::with_capacity(files.len());
    for file in files {
        let zdiff3 = r.conflicts.conflict(repo, onto, tip, &file)?;
        conflicts.push(Conflict { file, zdiff3 });
    }
    let sides = Sides {
        branch,
        tip,
        onto,
        pair: &pair,
        feature_tests: feature_tests(&run_tasks),
        primary_tests: features::project_settings(r.git, repo, onto)
            .ok()
            .and_then(|s| s.gates.fast),
    };
    let groups = r.planner.split(conflicts);
    let mut ids = Vec::with_capacity(groups.len());
    for group in &groups {
        ids.push(add_task(tx, r, &run, &sides, group)?.task_id);
    }
    let data = json!({
        "feature_id": feature_id,
        "rebase_job": last.id,
        "tip": tip,
        "onto": onto,
        "resolver_tasks": ids,
    });
    record(
        tx,
        r.guard,
        EventKind::Note,
        Entity::Feature,
        feature_id,
        cause,
        &data,
    )?;
    Ok(Some(Opened {
        feature_id: feature_id.to_string(),
        branch: branch.to_string(),
        tip: tip.to_string(),
        onto: onto.to_string(),
        tasks: ids,
    }))
}

/// The first line of each ticket of the conflict of `branch` at `tip` onto
/// `onto`: the key of the pair of tips.
fn pair_line(branch: &str, tip: &str, onto: &str) -> String {
    format!("<!-- horch-rebase-resolver: {branch} {tip} {onto} -->")
}

/// The check of each task of the run that is not cancelled, each 1 time.
fn feature_tests(run_tasks: &[Task]) -> Vec<String> {
    let mut tests: Vec<String> = Vec::new();
    for task in run_tasks {
        let Some(check) = &task.land_check else {
            continue;
        };
        if task.status != TaskStatus::Cancelled.as_str() && !tests.contains(check) {
            tests.push(check.clone());
        }
    }
    tests
}

/// The facts of both sides of a rebase conflict, for its tickets.
struct Sides<'a> {
    branch: &'a str,
    tip: &'a str,
    onto: &'a str,
    pair: &'a str,
    /// The checks of the tasks of the feature.
    feature_tests: Vec<String>,
    /// The fast gate of the project at the primary tip.
    primary_tests: Option<String>,
}

/// Add the resolver task of `group` to `run`.
fn add_task(
    tx: &Connection,
    r: &Resolvers<'_>,
    run: &Run,
    sides: &Sides<'_>,
    group: &[Conflict],
) -> Result<Task> {
    let files: Vec<String> = group.iter().map(|c| c.file.clone()).collect();
    let title = format!(
        "Resolve the rebase conflict of {} onto {}: {}",
        sides.branch,
        short(sides.onto),
        files.join(", ")
    );
    let mut new = NewTask::work(&title, &files[0], r.profile);
    new.work_kind = Some(RESOLVER_KIND);
    new.own = files;
    new.ticket_snapshot = Some(ticket(&title, sides, group));
    tasks::add(tx, run, &new, Actor::Horch)
}

/// The first 12 characters of the object id `id`.
fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// The ticket of the resolver task `title` of `group`.
fn ticket(title: &str, sides: &Sides<'_>, group: &[Conflict]) -> String {
    let mut out = format!(
        "{}\n# {title}\n\n**Kind:** feature\n\n\
         fleetd rebased the feature branch `{}` at `{}` onto the primary tip `{}`. \
         The rebase conflicts, so fleetd did not move the branch.\n\n\
         ## Rules\n\n\
         - In your worktree, merge `{}` and resolve the conflict of each file below, \
         so that each file holds the work of both sides.\n\
         - Change only the files below. Each other conflict file has its own resolver task.\n\
         - Run the tests of both sides.\n\
         - This task lands on the feature branch `{}`.\n\n\
         ## Tests of both sides\n\n",
        sides.pair, sides.branch, sides.tip, sides.onto, sides.onto, sides.branch,
    );
    out.push_str("The feature:\n\n");
    if sides.feature_tests.is_empty() {
        out.push_str("- No task of the feature has a check.\n");
    }
    for test in &sides.feature_tests {
        out.push_str(&format!("- `{test}`\n"));
    }
    out.push_str("\nThe primary branch (the fast gate of the project):\n\n");
    match &sides.primary_tests {
        Some(gate) => out.push_str(&format!("- `{gate}`\n")),
        None => out.push_str("- The project declares no fast gate.\n"),
    }
    out.push_str("\n## Conflict files\n");
    for c in group {
        out.push_str(&format!("\n### {}\n\n```\n{}", c.file, c.zdiff3));
        if !c.zdiff3.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pair_line_names_the_branch_and_both_tips() {
        assert_eq!(
            pair_line("fleet/r1", "a1", "b2"),
            "<!-- horch-rebase-resolver: fleet/r1 a1 b2 -->"
        );
    }

    #[test]
    fn a_short_id_is_12_characters_or_the_whole_id() {
        assert_eq!(short("0123456789abcdef"), "0123456789ab");
        assert_eq!(short("abc"), "abc");
    }
}
