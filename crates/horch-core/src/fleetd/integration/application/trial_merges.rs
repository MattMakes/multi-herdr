use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::integration::adapters::trial_merges_store as store;
use crate::fleetd::integration::domain::trial_merge::{self, InFlight, Pair, Side, Verdict};

pub trait TrialMergeGit {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;

    fn changed_files(&self, repo: &Path, base: &str, tip: &str) -> Result<BTreeSet<String>>;
}

#[derive(Debug, Clone, Copy)]
pub struct Primary<'a> {
    pub repo: &'a Path,
    pub branch: &'a str,
    pub tip: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialMergeData {
    pub conflict_files: Vec<String>,
}

pub fn queue_due(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    git: &dyn TrialMergeGit,
    primary: &Primary<'_>,
    budget: usize,
    clock: FleetSeconds,
) -> Result<Vec<i64>> {
    let main = Side {
        name: primary.branch.to_string(),
        ref_name: format!("refs/heads/{}", primary.branch),
        tip: primary.tip.to_string(),
    };
    let features = in_flight(tx, git, primary)?;
    let repo = primary.repo.to_string_lossy();
    let mut queued = Vec::new();
    for pair in trial_merge::pairs(&main, &features) {
        if queued.len() >= budget {
            break;
        }
        let row = store::get(tx, &pair.left.tip, &pair.right.tip)?;
        if row.is_some_and(|r| r.clean.is_some()) {
            continue;
        }
        let (subject, input_key) = (pair.subject(), pair.input_key());
        let key = JobKey {
            kind: Kind::TrialMerge,
            subject: &subject,
            input_key: &input_key,
        };
        match jobs::queue(ctx, tx, &key, Some(&repo), clock)? {
            Queued::New(id) => {
                store::put_queued(tx, ctx.guard, &pair, id)?;
                queued.push(id);
            }
            Queued::Reused(job) => reuse(ctx, tx, &pair, &job)?,
            Queued::Active(_) | Queued::Held(_) => {}
        }
    }
    Ok(queued)
}

fn in_flight(
    tx: &Connection,
    git: &dyn TrialMergeGit,
    primary: &Primary<'_>,
) -> Result<Vec<InFlight>> {
    let mut out = Vec::new();
    for (id, branch) in store::in_flight_features(tx, primary.branch)? {
        let ref_name = format!("refs/heads/{branch}");
        let Some(tip) = git.tip(primary.repo, &ref_name)? else {
            continue;
        };
        let files = git.changed_files(primary.repo, primary.tip, &tip)?;
        out.push(InFlight {
            side: Side {
                name: id,
                ref_name,
                tip,
            },
            files,
        });
    }
    Ok(out)
}

fn reuse(ctx: &JobCtx<'_>, tx: &Connection, pair: &Pair, job: &JobRow) -> Result<()> {
    if let Some(verdict) = verdict(job) {
        store::put_queued(tx, ctx.guard, pair, job.id)?;
        store::set_verdict(
            tx,
            ctx.guard,
            &pair.left.tip,
            &pair.right.tip,
            job.id,
            &verdict,
        )?;
    }
    Ok(())
}

pub fn record_ended(ctx: &JobCtx<'_>, tx: &Connection) -> Result<usize> {
    let mut written = 0;
    for row in store::waiting(tx)? {
        let Some(job) = jobs_store::get(tx, row.job_id)? else {
            continue;
        };
        let (left, right) = (row.left_tip.as_str(), row.right_tip.as_str());
        if job.state == State::Abandoned {
            let next = jobs_store::latest(tx, Kind::TrialMerge, &job.subject, &job.input_key)?;
            if let Some(next) = next.filter(|n| n.id != job.id) {
                store::set_job(tx, ctx.guard, left, right, next.id)?;
            }
        } else if let Some(verdict) = verdict(&job) {
            store::set_verdict(tx, ctx.guard, left, right, job.id, &verdict)?;
            written += 1;
        }
    }
    Ok(written)
}

fn verdict(job: &JobRow) -> Option<Verdict> {
    if !matches!(job.state, State::Passed | State::Failed) {
        return None;
    }
    let outcome = jobs::outcome(job).filter(|o| o.verdict)?;
    if outcome.passed {
        return Some(Verdict::Clean);
    }
    let data: TrialMergeData = serde_json::from_value(jobs::result_data(job)?).ok()?;
    Some(Verdict::Conflict(data.conflict_files))
}
