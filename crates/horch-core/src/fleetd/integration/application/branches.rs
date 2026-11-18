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
//! The rebase of a stack edge (a feature with `stacked_on`, or a key with a
//! cut) is not read here: `stack_edges.rs` opens its resolver task.
//!
//! I11 and I12 (`docs/specs/fleetd/design.md`, "No work is lost" items 4
//! and 5; preservation row 04 W6; MAP.md of fleetd-p4, Q17):
//! - [`read_listing`] runs in the `worktree_list` job: the fleet branches
//!   with their tips and the branches that a `git stash` entry names.
//! - [`adopt_orphans`] is the pass part of I11: each listed fleet branch
//!   with no row is adopted as an orphan row and reported.
//! - [`deletions`] is the pass part of I12: each listed branch whose row
//!   passes the guard of `domain/invariants.rs` gets 1 [`Deletion`].
//! - [`run`] is the effect after the commit: `git update-ref -d` on the
//!   listed tip, the compare-and-set form of `git branch -D`. It records
//!   nothing, so a crash at any point gives the deletion again on the next
//!   pass, and a tip that moved after the listing is never deleted.

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
    // A rebase with a cut, or of a feature with a lower feature, is the
    // rebase of a stack edge: `stack_edges.rs` opens its resolver task.
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

// ─── I11 and I12 (ticket 44) ────────────────────────────────────────────

/// The kind of the needs-you item of an orphan branch.
pub const ORPHAN_KIND: &str = "orphan_branch";

/// The owner of an orphan row: you decide what happens to it.
const ORPHAN_OWNER: &str = "operator";

/// The key of the fleet branches in the data of a `worktree_list` run.
pub const LISTING_KEY: &str = "fleet_branches";

/// The key of the error text when the job could not list them.
pub const LISTING_ERROR_KEY: &str = "fleet_branches_error";

/// A fleet branch and its tip, as the `worktree_list` job read them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub name: String,
    pub tip: String,
}

/// The fleet branches and the stash names of 1 repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub branches: Vec<Listed>,
    pub stash_branches: Vec<String>,
}

/// The facts of the last passed `worktree_list` run of 1 repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoListing {
    pub repo: String,
    /// `None` when the run did not list the fleet branches.
    pub listing: Option<Listing>,
    /// `None` when the run did not read the rescue refs.
    pub rescue: Option<Vec<RefFacts>>,
}

impl RepoListing {
    /// The facts in the result data `data` of a run of `repo`.
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

/// The facts of the newest passed `worktree_list` run of each repository.
pub fn listings(conn: &Connection) -> Result<Vec<RepoListing>> {
    Ok(jobs_store::last_passed(conn, Kind::WorktreeList)?
        .into_iter()
        .filter_map(|row| {
            let data = result_data(&row)?;
            Some(RepoListing::of(row.subject, &data))
        })
        .collect())
}

/// The fleet branches and the stash names of `repo`, for the
/// `worktree_list` job (Q17): each branch below `fleet/`, and the branch
/// and the task branches of each feature of `repo` outside `fleet/`.
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

/// The feature of each run of `repo` that has a run branch.
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

/// A fleet branch with no row: adopted when a feature has it, else only
/// reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub repo: String,
    pub branch: String,
    pub tip: String,
    /// The feature of the adopted row, `None` when no feature has the
    /// branch.
    pub feature_id: Option<String>,
}

/// I11, in the transaction `tx` of the pass: each listed fleet branch with
/// no row is adopted as an orphan row of its feature, and 1 needs-you item
/// reports it. A branch of no feature can have no row: it is reported in
/// each pass. Reads no git.
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

/// Write the orphan row of `branch` in the feature `feature_id`. It has no
/// task and no landing state, so no rule of landing acts on it.
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

/// Open the needs-you item of `o`.
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

/// 1 deletion of a task branch, to run after the commit of its pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub repo: PathBuf,
    pub branch: String,
    /// The tip that the listing gave: git deletes the branch only at it.
    pub tip: String,
}

/// I12, in the transaction `tx` of the pass: 1 deletion for each listed
/// branch whose row passes the guard [`may_delete`]. Reads no git and
/// writes no row: the next listing observes the effect.
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

/// Whether a rescue ref of `row` is at `tip`: a ref of its branch, or the
/// ref that the row names.
fn rescued(refs: Option<&[RefFacts]>, row: &BranchRow, tip: &str) -> bool {
    refs.unwrap_or_default().iter().any(|r| {
        r.tip == tip
            && (r.branch.as_deref() == Some(row.name.as_str())
                || row.rescue_ref.as_deref() == Some(r.name.as_str()))
    })
}

/// What [`run`] did with 1 [`Deletion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// The branch was at the listed tip, and git deleted it.
    Deleted,
    /// A `git stash` entry names the branch now, so nothing was deleted.
    Stashed,
    /// The branch was not at the listed tip, and nothing was deleted.
    /// `actual` is the tip that git has, or `None` when it is gone.
    Mismatch { actual: Option<String> },
}

/// Run `d` after the commit of its pass. A stash entry made after the
/// listing still keeps the branch. The deletion is a compare-and-set on the
/// listed tip, so a commit after the listing is never lost. A failure of
/// git is an error, and the next pass gives the deletion again.
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
