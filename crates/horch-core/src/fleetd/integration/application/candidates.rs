use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BundleFact, BundleRow};
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::integration::domain::bundle::BundleState;
use crate::taskledger::model::Actor;
use crate::taskledger::runs;

pub struct Candidates<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub bundle: i64,
    pub repo: PathBuf,
    pub name: String,
    pub tip: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    Deleted,
    Mismatch { actual: Option<String> },
}

fn due(state: BundleState, open_child: bool) -> bool {
    !state.is_open() && !open_child
}

pub fn advance(c: &Candidates<'_>, tx: &Connection) -> Result<Vec<Deletion>> {
    advance_in(c, tx, None)
}

pub fn advance_in(
    c: &Candidates<'_>,
    tx: &Connection,
    bundles: Option<&[i64]>,
) -> Result<Vec<Deletion>> {
    let mut deletions = Vec::new();
    for id in with_candidate_ref(tx)? {
        if bundles.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
        let state = BundleState::parse(&bundle.state)
            .with_context(|| format!("{:?} is not a bundle state", bundle.state))?;
        if !due(state, has_open_child(tx, id)?) {
            continue;
        }
        if let Some(d) = step(c, tx, &bundle).with_context(|| format!("bundle {id}"))? {
            deletions.push(d);
        }
    }
    Ok(deletions)
}

fn step(c: &Candidates<'_>, tx: &Connection, bundle: &BundleRow) -> Result<Option<Deletion>> {
    let Some(run) = runs::feature_run(tx, &bundle.feature_id)? else {
        return Ok(None);
    };
    let repo = PathBuf::from(run.project_root);
    let name = bundle
        .candidate_ref
        .clone()
        .context("the bundle has no candidate ref")?;
    if c.git.tip(&repo, &name)?.is_none() {
        rows::set_bundle_fact(
            tx,
            c.jobs.guard,
            bundle.id,
            BundleFact::CandidateRef(None),
            Cause {
                actor: Actor::Horch,
                text: "fleetd verified the deletion of its candidate ref",
            },
        )?;
        return Ok(None);
    }
    let tip = bundle
        .candidate_tip
        .clone()
        .context("the bundle has a candidate ref and no candidate tip")?;
    Ok(Some(Deletion {
        bundle: bundle.id,
        repo,
        name,
        tip,
    }))
}

pub fn run(git: &dyn IntegrationGitGateway, d: &Deletion) -> Result<Ran> {
    Ok(match git.delete_ref(&d.repo, &d.name, &d.tip)? {
        RefMove::Moved => Ran::Deleted,
        RefMove::Mismatch { actual } => Ran::Mismatch { actual },
    })
}

fn with_candidate_ref(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM bundles WHERE candidate_ref IS NOT NULL ORDER BY id")?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

fn has_open_child(conn: &Connection, id: i64) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT state FROM bundles WHERE parent_id = ?1")?;
    let states = stmt.query_map([id], |r| r.get::<_, String>(0))?;
    for state in states {
        if BundleState::parse(&state?).is_none_or(BundleState::is_open) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ended_bundle_with_no_open_child_is_due() {
        for state in BundleState::ALL {
            let ended = matches!(
                state,
                BundleState::Landed | BundleState::Failed | BundleState::Dropped
            );
            assert_eq!(due(state, false), ended, "{state:?}");
            assert!(!due(state, true), "{state:?} with an open child");
        }
    }
}
