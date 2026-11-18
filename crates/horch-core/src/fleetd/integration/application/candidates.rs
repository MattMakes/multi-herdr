//! The deletion of candidate refs (ticket 32).
//!
//! `docs/specs/fleetd/design.md`, "fleetd owns the candidate refs" and "The
//! bundle state machine": fleetd deletes the ref `refs/horch/bundle/<id>`
//! of a bundle in `landed`, `failed` or `dropped` when no child bundle of
//! it is in `building`, `gating` or `passed`.
//! - [`advance`] is the pass part. For each such bundle whose row still has
//!   its `candidate_ref`, it reads the ref store. When the ref is gone, the
//!   deletion is verified, and it clears `candidate_ref`. When the ref is
//!   there, it gives 1 [`Deletion`] to run after the commit.
//! - [`run`] is the effect after the commit: `git update-ref -d <ref>
//!   <candidate tip>`. It records nothing: the next pass reads the ref
//!   store again. So a failed deletion or a crash at any point is done
//!   again on the next pass, and a ref at a tip that fleetd did not write
//!   is never deleted.

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

/// What [`advance`] works with: the job context of the pass, and the git
/// gateway that reads the ref store.
pub struct Candidates<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

/// 1 deletion of a candidate ref, to run after the commit of its pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    /// The ended bundle.
    pub bundle: i64,
    pub repo: PathBuf,
    /// The full name of the ref, `refs/horch/bundle/<id>`.
    pub name: String,
    /// The candidate tip of the bundle: git deletes the ref only at it.
    pub tip: String,
}

/// What [`run`] did with 1 [`Deletion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// The ref was at the candidate tip, and git deleted it.
    Deleted,
    /// The ref was not at the candidate tip, and nothing was deleted.
    /// `actual` is the tip that git has, or `None` when the ref is gone.
    Mismatch { actual: Option<String> },
}

/// Whether the ref of a bundle in `state` is due for deletion: the bundle
/// is ended, and no child bundle of it is open.
fn due(state: BundleState, open_child: bool) -> bool {
    !state.is_open() && !open_child
}

/// The pass part of the deletion, in the transaction `tx` of the pass:
/// clear `candidate_ref` of each due bundle whose ref is gone, and give 1
/// deletion for each due bundle whose ref is there, by bundle id.
pub fn advance(c: &Candidates<'_>, tx: &Connection) -> Result<Vec<Deletion>> {
    let mut deletions = Vec::new();
    for id in with_candidate_ref(tx)? {
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

/// 1 step of the due bundle `bundle`.
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

/// Run `d` after the commit of its pass: delete the ref only at the
/// candidate tip. A failure of git is an error, and the next pass gives the
/// deletion again.
pub fn run(git: &dyn IntegrationGitGateway, d: &Deletion) -> Result<Ran> {
    Ok(match git.delete_ref(&d.repo, &d.name, &d.tip)? {
        RefMove::Moved => Ran::Deleted,
        RefMove::Mismatch { actual } => Ran::Mismatch { actual },
    })
}

/// The ids of the bundles whose row has a candidate ref, by id.
fn with_candidate_ref(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM bundles WHERE candidate_ref IS NOT NULL ORDER BY id")?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// Whether a child bundle of the bundle `id` is open.
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
