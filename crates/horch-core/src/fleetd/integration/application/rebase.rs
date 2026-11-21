use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{self, result_data, JobCtx, JobKey};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::features::{self, Tips};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::application::remote;
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use crate::fleetd::worktree_custody::application::gate_tree::{self, GateTreeRequest};
use crate::taskledger::model::Actor;
use crate::taskledger::runs::{self, Run};

pub trait RebaseRefs {
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool>;

    fn rescue(&self, repo: &Path, branch: &str, tip: &str) -> Result<String>;

    fn merge_base(&self, repo: &Path, a: &str, b: &str) -> Result<Option<String>>;
}

pub struct Rebases<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
    pub refs: &'a dyn RebaseRefs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Rescue {
        repo: PathBuf,
        branch: String,
        tip: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub branch: String,
    pub rescue_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rebased {
    Moved { tip: String },
    Conflict { files: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebaseKey<'a> {
    pub tip: &'a str,
    pub onto: &'a str,
    pub cut: Option<&'a str>,
}

pub fn rebase_key(tip: &str, onto: &str) -> String {
    format!("{tip} {onto}")
}

pub fn rebase_key_cut(tip: &str, onto: &str, cut: &str) -> String {
    format!("{tip} {onto} {cut}")
}

pub fn read_key(key: &str) -> Option<RebaseKey<'_>> {
    let parts: Vec<&str> = key.split(' ').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    match parts[..] {
        [tip, onto] => Some(RebaseKey {
            tip,
            onto,
            cut: None,
        }),
        [tip, onto, cut] => Some(RebaseKey {
            tip,
            onto,
            cut: Some(cut),
        }),
        _ => None,
    }
}

pub fn is_on(
    refs: &dyn RebaseRefs,
    repo: &Path,
    tip: &str,
    onto: &str,
    cut: Option<&str>,
) -> Result<bool> {
    if !refs.contains(repo, tip, onto)? {
        return Ok(false);
    }
    let Some(cut) = cut else {
        return Ok(true);
    };
    match refs.merge_base(repo, tip, cut)? {
        Some(base) => refs.contains(repo, onto, &base),
        None => Ok(true),
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

pub fn gate_tree_busy(conn: &Connection, feature_id: &str) -> Result<bool> {
    let bundles: i64 = conn.query_row(
        "SELECT COUNT(*) FROM bundles WHERE feature_id = ?1 AND state IN ('gating', 'passed')",
        params![feature_id],
        |r| r.get(0),
    )?;
    Ok(bundles > 0 || jobs_store::active(conn, Kind::FullGate, feature_id)?.is_some())
}

pub fn follow(r: &Rebases<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Vec<Effect>> {
    follow_in(r, tx, clock, None)
}

pub fn follow_in(
    r: &Rebases<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    features: Option<&[String]>,
) -> Result<Vec<Effect>> {
    let mut effects = Vec::new();
    for (feature_id, branch, _) in rows::live_feature_branches(tx)? {
        if features.is_some_and(|ids| !ids.contains(&feature_id)) {
            continue;
        }
        if let Some(effect) = follow_one(r, tx, clock, &feature_id, &branch)? {
            effects.push(effect);
        }
    }
    Ok(effects)
}

fn follow_one(
    r: &Rebases<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    feature_id: &str,
    branch: &str,
) -> Result<Option<Effect>> {
    let Some(run) = runs::feature_run(tx, feature_id)? else {
        return Ok(None);
    };
    let row = rows::feature(tx, feature_id)?.context("no such feature in the ledger")?;
    let Ok(tips) = features::read_tips(r.git, tx, &run.project_root, &row)? else {
        return Ok(None);
    };
    let last = jobs_store::last(tx, Kind::Rebase, branch)?;
    record_moved(r, tx, clock, feature_id, branch, &tips, last.as_ref())?;
    if let Some(lower) = row.stacked_on.as_deref() {
        if !lower_on_primary(r, tx, &tips, lower)? {
            return Ok(None);
        }
    }
    if gate_tree_busy(tx, feature_id)? {
        return Ok(None);
    }
    if let Some(last) = last.as_ref().filter(|l| l.state == State::Abandoned) {
        if read_key(&last.input_key).is_some_and(|k| k.onto == tips.primary) {
            if gate_tree_ready(r, tx, clock, &run, feature_id, branch, &tips)? {
                queue(r, tx, clock, branch, &tips.repo, &last.input_key)?;
            }
            return Ok(None);
        }
    }
    if r.refs.contains(&tips.repo, &tips.feature, &tips.primary)? {
        return Ok(None);
    }
    if !gate_tree_ready(r, tx, clock, &run, feature_id, branch, &tips)? {
        return Ok(None);
    }
    let key = rebase_key(&tips.feature, &tips.primary);
    let ended = jobs_store::latest(tx, Kind::Rebase, branch, &key)?
        .is_some_and(|l| matches!(l.state, State::Passed | State::Failed));
    if ended {
        return Ok(None);
    }
    if !rescued(r, tx, branch, &tips)? {
        return Ok(Some(Effect::Rescue {
            repo: tips.repo.clone(),
            branch: branch.to_string(),
            tip: tips.feature.clone(),
        }));
    }
    queue(r, tx, clock, branch, &tips.repo, &key)?;
    Ok(None)
}

fn lower_on_primary(r: &Rebases<'_>, tx: &Connection, tips: &Tips, lower: &str) -> Result<bool> {
    let Some(row) = rows::feature(tx, lower)? else {
        return Ok(false);
    };
    let Some(branch) = row
        .branch
        .filter(|_| matches!(row.state.as_str(), "open" | "ready"))
    else {
        return Ok(false);
    };
    match r.git.tip(&tips.repo, &format!("refs/heads/{branch}"))? {
        Some(tip) => r.refs.contains(&tips.repo, &tip, &tips.primary),
        None => Ok(false),
    }
}

pub(crate) fn rescued(r: &Rebases<'_>, tx: &Connection, branch: &str, tips: &Tips) -> Result<bool> {
    let Some(name) = rows::branch(tx, branch)?.and_then(|b| b.rescue_ref) else {
        return Ok(false);
    };
    Ok(r.git.tip(&tips.repo, &name)?.as_deref() == Some(tips.feature.as_str()))
}

pub(crate) fn gate_tree_ready(
    r: &Rebases<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    run: &Run,
    feature_id: &str,
    branch: &str,
    tips: &Tips,
) -> Result<bool> {
    if let Some(tree) = gate_tree::of_feature(tx, &tips.repo, feature_id)? {
        if tree.state != WorktreeState::Reserved {
            return Ok(tree.state == WorktreeState::Created && Path::new(&tree.path).is_dir());
        }
    }
    let req = GateTreeRequest {
        feature_id,
        run_id: &run.run_id,
        repo: &tips.repo,
        base: branch,
        tip: &tips.feature,
    };
    gate_tree::reserve(r.jobs, tx, &req, clock)?;
    Ok(false)
}

pub(crate) fn queue(
    r: &Rebases<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    branch: &str,
    repo: &Path,
    key: &str,
) -> Result<()> {
    let key = JobKey {
        kind: Kind::Rebase,
        subject: branch,
        input_key: key,
    };
    jobs::queue(r.jobs, tx, &key, Some(&repo.to_string_lossy()), clock)?;
    Ok(())
}

fn record_moved(
    r: &Rebases<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    feature_id: &str,
    branch: &str,
    tips: &Tips,
    last: Option<&JobRow>,
) -> Result<()> {
    let Some(last) = last.filter(|l| l.state == State::Passed) else {
        return Ok(());
    };
    let moved = result_data(last).and_then(|d| serde_json::from_value::<Rebased>(d).ok());
    let Some(Rebased::Moved { tip }) = moved else {
        return Ok(());
    };
    if tip != tips.feature {
        return Ok(());
    }
    let row = rows::branch(tx, branch)?.with_context(|| format!("no branch {branch}"))?;
    if row.seen_tip.as_deref() == Some(tip.as_str()) {
        return Ok(());
    }
    let why = format!("the rebase job {} moved the branch to {tip}", last.id);
    rows::set_branch_fact(
        tx,
        r.jobs.guard,
        branch,
        BranchFact::SeenTip(&tip),
        by_fleetd(&why),
    )?;
    let settings =
        features::project_settings(r.git, &tips.repo, &tip)?.map_err(anyhow::Error::msg)?;
    let repo = tips.repo.to_string_lossy();
    remote::queue_push(r.jobs, tx, &repo, feature_id, &settings.remote, true, clock)?;
    Ok(())
}

pub fn run(effect: &Effect, refs: &dyn RebaseRefs) -> Result<Rescued> {
    let Effect::Rescue { repo, branch, tip } = effect;
    Ok(Rescued {
        branch: branch.clone(),
        rescue_ref: refs.rescue(repo, branch, tip)?,
    })
}

pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, done: &Rescued) -> Result<()> {
    let Some(row) = rows::branch(tx, &done.branch)? else {
        return Ok(());
    };
    if row.rescue_ref.as_deref() == Some(done.rescue_ref.as_str()) {
        return Ok(());
    }
    let why = format!(
        "the rescue ref {} is written before the rebase",
        done.rescue_ref
    );
    rows::set_branch_fact(
        tx,
        ctx.guard,
        &done.branch,
        BranchFact::RescueRef(&done.rescue_ref),
        by_fleetd(&why),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_holds_the_tip_and_the_tip_to_move_onto() {
        let key = rebase_key("a1", "b2");
        let read = read_key(&key).unwrap();
        assert_eq!((read.tip, read.onto, read.cut), ("a1", "b2", None));
        let key = rebase_key_cut("a1", "b2", "c3");
        let read = read_key(&key).unwrap();
        assert_eq!((read.tip, read.onto, read.cut), ("a1", "b2", Some("c3")));
        assert_eq!(read_key("a1"), None);
        assert_eq!(read_key(" b2"), None);
        assert_eq!(read_key("a1 b2 "), None);
        assert_eq!(read_key("a1 b2 c3 d4"), None);
    }

    #[test]
    fn a_moved_result_reads_back() {
        let data = serde_json::to_value(Rebased::Moved { tip: "t".into() }).unwrap();
        assert_eq!(
            serde_json::from_value::<Rebased>(data).unwrap(),
            Rebased::Moved { tip: "t".into() }
        );
    }
}
