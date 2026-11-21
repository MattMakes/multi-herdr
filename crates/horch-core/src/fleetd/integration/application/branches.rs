use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::{
    result_data, JobCtx, NeedsYouItems, NewItem,
};
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BranchRow, NewBranch,
};
use crate::fleetd::integration::application::features;
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::integration::application::rebase::{read_key, RebaseKey, Rebased};
use crate::fleetd::integration::application::stacks::StackGit;
use crate::fleetd::integration::domain::invariants::{
    feature_of, is_fleet_branch, may_delete, DeletionFacts, FeatureBranch, FLEET_PREFIX,
};
use crate::fleetd::integration::domain::stack::{Conflict, StackPlanner};
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::{facts_of, RefFacts};
use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkKind};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::tasks::{self, NewTask, Task};

const RESOLVER_KIND: WorkKind = WorkKind::Feature;

pub struct Resolvers<'a> {
    pub git: &'a dyn IntegrationGitGateway,
    pub conflicts: &'a dyn StackGit,
    pub planner: StackPlanner,
    pub profile: &'a str,
    pub guard: Option<Generation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub feature_id: String,
    pub branch: String,
    pub tip: String,
    pub onto: String,
    pub tasks: Vec<String>,
}

pub fn open_rebase_resolvers(
    tx: &Connection,
    r: &Resolvers<'_>,
    cause: Cause<'_>,
) -> Result<Vec<Opened>> {
    open_rebase_resolvers_in(tx, r, cause, None)
}

pub fn open_rebase_resolvers_in(
    tx: &Connection,
    r: &Resolvers<'_>,
    cause: Cause<'_>,
    features: Option<&[String]>,
) -> Result<Vec<Opened>> {
    let mut out = Vec::new();
    for (feature_id, branch, _) in rows::live_feature_branches(tx)? {
        if features.is_some_and(|ids| !ids.contains(&feature_id)) {
            continue;
        }
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
    let Some(RebaseKey {
        tip,
        onto,
        cut: None,
    }) = read_key(&last.input_key)
    else {
        return Ok(None);
    };
    if rows::feature(tx, feature_id)?.is_none_or(|f| f.stacked_on.is_some()) {
        return Ok(None);
    }
    let Some(run) = runs::feature_run(tx, feature_id)? else {
        return Ok(None);
    };
    let repo = Path::new(&run.project_root);
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
        primary_tests: features::project_settings(r.git, repo, onto)?
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

pub fn resolver_persona(
    conn: &Connection,
    feature_id: &str,
    branch: &str,
) -> Result<Option<String>> {
    if let Some(task) = rows::branch(conn, branch)?.and_then(|b| b.task_id) {
        if let Some(persona) = tasks::get(conn, &task)?.primary_persona {
            return Ok(Some(persona));
        }
    }
    let Some(run) = runs::feature_run(conn, feature_id)? else {
        return Ok(None);
    };
    Ok(tasks::of_run(conn, &run.run_id)?
        .into_iter()
        .rev()
        .find_map(|t| t.primary_persona))
}

fn pair_line(branch: &str, tip: &str, onto: &str) -> String {
    format!("<!-- horch-rebase-resolver: {branch} {tip} {onto} -->")
}

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

struct Sides<'a> {
    branch: &'a str,
    tip: &'a str,
    onto: &'a str,
    pair: &'a str,
    feature_tests: Vec<String>,
    primary_tests: Option<String>,
}

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

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

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

pub const ORPHAN_KIND: &str = "orphan_branch";

const ORPHAN_OWNER: &str = "operator";

pub const LISTING_KEY: &str = "fleet_branches";

pub const LISTING_ERROR_KEY: &str = "fleet_branches_error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub name: String,
    pub tip: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub branches: Vec<Listed>,
    pub stash_branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoListing {
    pub repo: String,
    pub listing: Option<Listing>,
    pub rescue: Option<Vec<RefFacts>>,
}

impl RepoListing {
    pub fn of(repo: String, data: &serde_json::Value) -> RepoListing {
        let listing = data
            .get(LISTING_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        RepoListing {
            repo,
            listing,
            rescue: facts_of(data),
        }
    }
}

pub fn listings(conn: &Connection) -> Result<Vec<RepoListing>> {
    Ok(jobs_store::last_passed(conn, Kind::WorktreeList)?
        .into_iter()
        .filter_map(|row| {
            let data = result_data(&row)?;
            Some(RepoListing::of(row.subject, &data))
        })
        .collect())
}

pub fn read_listing(
    conn: &Connection,
    git: &dyn IntegrationGitGateway,
    repo: &Path,
) -> Result<Listing> {
    let features = features_of_repo(conn, repo)?;
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for b in git.fleet_branches(repo)? {
        found.insert(b.name, b.tip);
    }
    for f in features
        .iter()
        .filter(|f| !f.branch.starts_with(FLEET_PREFIX))
    {
        if let Some(tip) = git.tip(repo, &format!("refs/heads/{}", f.branch))? {
            found.insert(f.branch.clone(), tip);
        }
        for b in git.task_branches(repo, &f.branch)? {
            found.insert(b.name, b.tip);
        }
    }
    let branches = found
        .into_iter()
        .filter(|(name, _)| is_fleet_branch(name, &features))
        .map(|(name, tip)| Listed { name, tip })
        .collect();
    Ok(Listing {
        branches,
        stash_branches: git.stash_branches(repo)?,
    })
}

fn features_of_repo(conn: &Connection, repo: &Path) -> Result<Vec<FeatureBranch>> {
    let mut out: Vec<FeatureBranch> = Vec::new();
    for run in runs::of_project(conn, repo)? {
        let Some(branch) = run.run_branch else {
            continue;
        };
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM features WHERE branch = ?1",
                [&branch],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = id {
            if !out.iter().any(|f| f.id == id) {
                out.push(FeatureBranch { id, branch });
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub repo: String,
    pub branch: String,
    pub tip: String,
    pub feature_id: Option<String>,
}

pub fn adopt_orphans(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    lists: &[RepoListing],
    clock: FleetSeconds,
) -> Result<Vec<Orphan>> {
    let mut out = Vec::new();
    for list in lists {
        let Some(listing) = &list.listing else {
            continue;
        };
        let features = features_of_repo(tx, Path::new(&list.repo))?;
        for b in &listing.branches {
            if rows::branch(tx, &b.name)?.is_some() {
                continue;
            }
            let feature_id = feature_of(&b.name, &features).map(|f| f.id.clone());
            if let Some(id) = &feature_id {
                adopt(tx, ctx.guard, &b.name, id, clock)?;
            }
            let orphan = Orphan {
                repo: list.repo.clone(),
                branch: b.name.clone(),
                tip: b.tip.clone(),
                feature_id,
            };
            report(ctx.needs_you, tx, &orphan)?;
            out.push(orphan);
        }
    }
    Ok(out)
}

fn adopt(
    tx: &Connection,
    guard: Option<Generation>,
    branch: &str,
    feature_id: &str,
    clock: FleetSeconds,
) -> Result<()> {
    let cause = Cause {
        actor: Actor::Horch,
        text: "fleetd adopted a fleet branch with no row as an orphan (I11)",
    };
    let new = NewBranch {
        name: branch,
        feature_id,
        task_id: None,
        owner: ORPHAN_OWNER,
        due_at: clock.0,
    };
    rows::insert_branch(tx, guard, &new, cause)?;
    rows::set_branch_fact(tx, guard, branch, BranchFact::Orphan, cause)
}

fn report(needs_you: &dyn NeedsYouItems, tx: &Connection, o: &Orphan) -> Result<()> {
    let row = match &o.feature_id {
        Some(id) => format!("fleetd adopted it as an orphan row of feature {id}"),
        None => "no feature has the branch, so it has no row".to_string(),
    };
    let text = format!(
        "The fleet branch {} at {} in {} had no ledger row: {row}. \
         fleetd never deletes it. Land its work, or delete the branch yourself.",
        o.branch, o.tip, o.repo
    );
    needs_you.open_item(
        tx,
        &NewItem {
            key: &format!("{ORPHAN_KIND}:{}", o.branch),
            kind: ORPHAN_KIND,
            entity: Entity::TaskBranch,
            entity_id: &o.branch,
            text: &text,
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub repo: PathBuf,
    pub branch: String,
    pub tip: String,
}

pub fn deletions(tx: &Connection, lists: &[RepoListing]) -> Result<Vec<Deletion>> {
    let live: Vec<WorktreeState> = WorktreeState::ALL
        .into_iter()
        .filter(|s| *s != WorktreeState::Removed)
        .collect();
    let trees = trees::in_states(tx, &live)?;
    let mut out = Vec::new();
    for list in lists {
        let Some(listing) = &list.listing else {
            continue;
        };
        let held: BTreeSet<&str> = trees
            .iter()
            .filter(|t| t.repo_root == list.repo)
            .map(|t| t.branch.as_str())
            .collect();
        for b in &listing.branches {
            let Some(row) = rows::branch(tx, &b.name)? else {
                continue;
            };
            let facts = DeletionFacts {
                orphan: row.orphan,
                landing: row.landing_state.as_deref().and_then(LandingState::parse),
                rescued: rescued(list.rescue.as_deref(), &row, &b.tip),
                stashed: listing.stash_branches.contains(&b.name),
                in_worktree: held.contains(b.name.as_str()),
            };
            if may_delete(&facts).is_ok() {
                out.push(Deletion {
                    repo: PathBuf::from(&list.repo),
                    branch: b.name.clone(),
                    tip: b.tip.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn rescued(refs: Option<&[RefFacts]>, row: &BranchRow, tip: &str) -> bool {
    refs.unwrap_or_default().iter().any(|r| {
        r.tip == tip
            && (r.branch.as_deref() == Some(row.name.as_str())
                || row.rescue_ref.as_deref() == Some(r.name.as_str()))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    Deleted,
    Stashed,
    Mismatch { actual: Option<String> },
}

pub fn run(git: &dyn IntegrationGitGateway, d: &Deletion) -> Result<Ran> {
    if git.stash_branches(&d.repo)?.contains(&d.branch) {
        return Ok(Ran::Stashed);
    }
    let name = format!("refs/heads/{}", d.branch);
    Ok(match git.delete_ref(&d.repo, &name, &d.tip)? {
        RefMove::Moved => Ran::Deleted,
        RefMove::Mismatch { actual } => Ran::Mismatch { actual },
    })
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
