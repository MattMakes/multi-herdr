//! The use cases of the feature state machine (ticket 36 of fleetd-p4,
//! ticket 34 of fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "The feature state machine", "Between
//! features" (The full gate; Merged), I8; `docs/tickets/fleetd-p4/MAP.md`,
//! Q5 and Q20. In Phases 4 and 5 each run is 1 feature: the feature
//! `F-<run rowid>` of the run, its branch is the run branch.
//!
//! [`follow`] is the pass part. For each `open` or `ready` feature it reads
//! the facts, and [`rules::next`] gives the move:
//! - the feature tip is the local feature branch. The primary tip is the
//!   fetched tip of the primary branch in `remote_tips`, or the local
//!   primary branch when the project file at the feature tip sets
//!   `remote = none`.
//! - each idea of the feature is `integrating`, `cancelled` or `parked`, and
//!   1 task branch is `landed`: the full gate runs 1 time as a job on the
//!   feature tip over the primary tip. Its pass on these 2 tips makes the
//!   feature `ready`. A pass on a tip that moved is stale, and a new idea
//!   of the feature holds it, so a `ready` feature goes back to `open`.
//! - git shows the changes of the feature tip in the primary tip by
//!   content: `merged` (I8). A `merged` feature is terminal: [`follow`]
//!   does not read git for it again.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::adapters::exec_gate::{self, GateRequest};
use crate::fleetd::integration::adapters::project_file;
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::domain::feature::{
    self as rules, Facts, FeatureState, FullGate, Next,
};
use crate::fleetd::integration::domain::project::{ProjectSettings, Remote};
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::worktree_custody::application::gate_tree::{self, GateTreeRequest};
use crate::taskledger::model::Actor;
use crate::taskledger::runs;

/// The word of the full gate in `gate_runs`.
const FULL_GATE: &str = "full";

/// The states of an idea that do not hold its feature in `open` (design,
/// "The feature state machine").
const SETTLED_IDEA: [&str; 3] = ["integrating", "cancelled", "parked"];

/// The kind of the needs-you item of a feature that [`follow`] cannot
/// follow: its rows roll back, and the other features of the pass go on.
pub const FOLLOW_FAILED_KIND: &str = "feature_follow_failed";

/// What [`follow`] reads with.
pub struct Features<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

/// What [`follow`] did with 1 feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Stayed,
    Entered(FeatureState),
    /// The full gate job of the current tips is queued, or it is queued
    /// already.
    FullGateQueued,
    /// fleetd cannot read a fact of the feature yet, for this reason. The
    /// feature does not move.
    Waits(String),
}

/// Why a full gate waits: the gate tree of its feature is not made yet.
pub const GATE_TREE_NOT_MADE: &str = "the gate tree of the feature is not made yet";

/// Why a settled feature has no full gate yet: its tip does not contain the
/// fetched primary tip.
pub const WAITS_FOR_REBASE: &str =
    "the feature tip does not contain the fetched primary tip; the rebase comes first";

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The pass part of the feature state machine, in `tx` at the fleet clock
/// `clock`: each `open` or `ready` feature, by id, with what it did.
pub fn follow(
    f: &Features<'_>,
    tx: &Connection,
    clock: FleetSeconds,
) -> Result<Vec<(String, Step)>> {
    follow_in(f, tx, clock, None)
}

/// [`follow`] for the work budget of 1 pass: only the features of
/// `features`. `None` is each feature.
pub fn follow_in(
    f: &Features<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    features: Option<&[String]>,
) -> Result<Vec<(String, Step)>> {
    let mut steps = Vec::new();
    for id in live_features(tx)? {
        if features.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        tx.execute_batch("SAVEPOINT feature_follow")?;
        let step = match follow_one(f, tx, &id, clock) {
            Ok(step) => {
                tx.execute_batch("RELEASE feature_follow")?;
                f.jobs.needs_you.resolve_item(tx, &follow_failed_key(&id))?;
                step
            }
            Err(e) => {
                tx.execute_batch("ROLLBACK TO feature_follow; RELEASE feature_follow")?;
                open_follow_failed(f, tx, &id, &e)?;
                Step::Waits(format!("{e:#}"))
            }
        };
        steps.push((id, step));
    }
    end_follow_failed(f, tx)?;
    Ok(steps)
}

fn follow_failed_key(id: &str) -> String {
    format!("{FOLLOW_FAILED_KIND}:{id}")
}

fn open_follow_failed(
    f: &Features<'_>,
    tx: &Connection,
    id: &str,
    e: &anyhow::Error,
) -> Result<()> {
    let text =
        format!("fleetd cannot follow the feature {id}: {e:#}. fleetd tries again on each pass.");
    f.jobs.needs_you.open_item(
        tx,
        &NewItem {
            key: &follow_failed_key(id),
            kind: FOLLOW_FAILED_KIND,
            entity: Entity::Feature,
            entity_id: id,
            text: &text,
        },
    )
}

fn end_follow_failed(f: &Features<'_>, tx: &Connection) -> Result<()> {
    let live = live_features(tx)?;
    for key in needs_you::open_keys(tx, FOLLOW_FAILED_KIND)? {
        let Some(id) = key.strip_prefix(&format!("{FOLLOW_FAILED_KIND}:")) else {
            continue;
        };
        if !live.iter().any(|l| l == id) {
            f.jobs.needs_you.resolve_item(tx, &key)?;
        }
    }
    Ok(())
}

/// The `open` and `ready` features, by id.
fn live_features(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM features WHERE state IN (?1, ?2) ORDER BY id")?;
    let rows = stmt.query_map(
        [FeatureState::Open.as_str(), FeatureState::Ready.as_str()],
        |r| r.get(0),
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The tips that the feature is judged on.
pub(crate) struct Tips {
    pub(crate) repo: PathBuf,
    pub(crate) feature: String,
    pub(crate) primary: String,
}

fn follow_one(f: &Features<'_>, tx: &Connection, id: &str, clock: FleetSeconds) -> Result<Step> {
    let row = rows::feature(tx, id)?.context("no such feature in the ledger")?;
    let state =
        FeatureState::parse(&row.state).with_context(|| format!("state {:?}", row.state))?;
    if state.is_terminal() {
        return Ok(Step::Stayed);
    }
    let Some(run) = runs::feature_run(tx, id)? else {
        return Ok(Step::Waits("the feature has no run".into()));
    };
    let tips = match read_tips(f.git, tx, &run.project_root, &row)? {
        Ok(tips) => tips,
        Err(why) => return Ok(Step::Waits(why)),
    };
    let has_landed_work = has_landed_branch(tx, id)?;
    let facts = Facts {
        ideas_settled: ideas_settled(tx, id)?,
        has_landed_work,
        full_gate: full_gate(tx, id, &tips)?,
        changes_in_primary: has_landed_work
            && f.git
                .changes_in(&tips.repo, &tips.feature, &tips.primary, &tips.primary)?,
    };
    match rules::next(state, &facts) {
        Next::Stay => Ok(Step::Stayed),
        Next::Enter(to) => {
            let why = why_enter(to, &tips);
            transition(
                tx,
                f.jobs.guard,
                Entity::Feature,
                id,
                to.as_str(),
                by_fleetd(&why),
                f.jobs.needs_you,
            )?;
            Ok(Step::Entered(to))
        }
        // The full gate judges the feature tip over the fetched primary
        // tip, and the gate refuses a tip that does not contain it. So no
        // full gate is queued then: a queued gate holds the gate tree, and
        // the rebase that brings the primary tip in waits for the tree
        // (continuous rebase, FM-42).
        Next::RunFullGate if !f.git.contains(&tips.repo, &tips.feature, &tips.primary)? => {
            Ok(Step::Waits(WAITS_FOR_REBASE.into()))
        }
        Next::RunFullGate => {
            let tree = GateTreeRequest {
                feature_id: id,
                run_id: &run.run_id,
                repo: &tips.repo,
                base: row.branch.as_deref().unwrap_or_default(),
                tip: &tips.feature,
            };
            if !gate_tree::ready(f.jobs, tx, &tree, clock)? {
                return Ok(Step::Waits(GATE_TREE_NOT_MADE.into()));
            }
            let req = GateRequest {
                kind: Kind::FullGate,
                subject: id,
                feature_id: id,
                tip: &tips.feature,
                base_tip: &tips.primary,
                repo: &tips.repo,
            };
            Ok(match exec_gate::queue(f.jobs, tx, &req, clock)? {
                Some(_) => Step::FullGateQueued,
                None => Step::Waits(exec_gate::NO_GATE_SANDBOX.into()),
            })
        }
    }
}

/// The cause text of the move to `to`.
fn why_enter(to: FeatureState, tips: &Tips) -> String {
    match to {
        FeatureState::Ready => format!(
            "the full gate passed on {} over the primary tip {}",
            tips.feature, tips.primary
        ),
        FeatureState::Merged => format!(
            "git shows the changes of {} in the primary tip {}",
            tips.feature, tips.primary
        ),
        _ => format!(
            "no full gate pass on {} over the primary tip {}, or an idea is in flight",
            tips.feature, tips.primary
        ),
    }
}

/// The feature tip and the primary tip of `row` in the repository
/// `project_root`. The inner `Err` is why fleetd cannot read them yet.
pub(crate) fn read_tips(
    git: &dyn IntegrationGitGateway,
    conn: &Connection,
    project_root: &str,
    row: &rows::FeatureRow,
) -> Result<Result<Tips, String>> {
    let repo = PathBuf::from(project_root);
    let Some(branch) = row.branch.as_deref() else {
        return Ok(Err("the feature has no branch yet".into()));
    };
    let Some(feature) = git.tip(&repo, &format!("refs/heads/{branch}"))? else {
        return Ok(Err(format!("git has no branch {branch}")));
    };
    let settings = match project_settings(git, &repo, &feature) {
        Ok(s) => s,
        Err(e) => return Ok(Err(format!("{e:#}"))),
    };
    let primary_branch = &row.primary_branch;
    let primary = match settings.remote {
        Remote::None => git.tip(&repo, &format!("refs/heads/{primary_branch}"))?,
        Remote::Named(_) => rows::remote_tip(conn, project_root, primary_branch)?.map(|r| r.tip),
    };
    let Some(primary) = primary else {
        return Ok(Err(format!(
            "no tip of the primary branch {primary_branch}"
        )));
    };
    Ok(Ok(Tips {
        repo,
        feature,
        primary,
    }))
}

/// The project file at the feature tip `tip`, or the defaults when the
/// commit has none.
pub(crate) fn project_settings(
    git: &dyn IntegrationGitGateway,
    repo: &Path,
    tip: &str,
) -> Result<ProjectSettings> {
    match git.fleet_toml(repo, tip)? {
        Some(text) => project_file::parse(&text).with_context(|| format!("at {tip}")),
        None => Ok(ProjectSettings::default()),
    }
}

/// Whether the feature `id` has at least 1 idea, and each of its ideas is
/// `integrating`, `cancelled` or `parked`. A feature with no idea row is
/// not settled: a guard whose fact is unknown does not hold.
fn ideas_settled(conn: &Connection, id: &str) -> Result<bool> {
    let ideas = ideas_store::of_feature(conn, id)?;
    Ok(!ideas.is_empty()
        && ideas
            .iter()
            .all(|i| SETTLED_IDEA.contains(&i.state.as_str())))
}

/// Whether a task branch of the feature `id` is `landed`.
pub(crate) fn has_landed_branch(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM branches
             WHERE feature_id = ?1 AND task_id IS NOT NULL AND landing_state = ?2)",
        params![id, LandingState::Landed.as_str()],
        |r| r.get(0),
    )?)
}

/// The verdict of the full gate of the feature `id` on `tips`. A verdict
/// on other tips is stale and is not read.
fn full_gate(conn: &Connection, id: &str, tips: &Tips) -> Result<FullGate> {
    let run = rows::gate_run_of(conn, id, FULL_GATE, &tips.feature, &tips.primary)?;
    Ok(match run.and_then(|r| r.result).as_deref() {
        Some("pass") => FullGate::Passed,
        Some("fail") => FullGate::Failed,
        _ => FullGate::NotJudged,
    })
}
