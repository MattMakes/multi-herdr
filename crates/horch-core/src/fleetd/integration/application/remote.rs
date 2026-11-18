//! The use cases of the fetch and push jobs (tickets 37 and 38).
//!
//! `docs/specs/fleetd/design.md`, "Between features": remote git state is
//! level-triggered. 1 `fetch` job runs for each repository on the period
//! `fetch_period_s`, and each period is a new run, also after a failed
//! run. A passed run writes the fetched tip of each branch in
//! `remote_tips` and clears `stale_since`. A failed run sets `stale_since`
//! once. With `pr = gh`, the fetch reads the state of the pull request of
//! each feature: a pull request that is closed and not merged puts 1 item
//! on the needs-you list. With `remote = none`, no fetch runs.
//!
//! [`queue_fetch`] is the period rule (part 2 of a pass). The executor of
//! the job (`adapters/exec_fetch.rs`) does the network work outside each
//! transaction, then records what it saw with [`record_passed`] or
//! [`record_failed`].

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, JobCtx, JobKey, NeedsYouItems, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::integration::adapters::sqlite::{self as rows, FeatureFact};
use crate::fleetd::integration::domain::project::Remote;
use crate::taskledger::model::Actor;

/// The kind of the needs-you item of a closed pull request.
pub const PR_CLOSED_KIND: &str = "pr_closed";

/// The key of the needs-you item of the closed pull request of `feature`.
pub fn pr_closed_key(feature: &str) -> String {
    format!("{PR_CLOSED_KIND}:{feature}")
}

/// The input key of the fetch of `repo` from `remote`: the repository and
/// its remote ("The kinds of job").
pub fn fetch_key(repo: &str, remote: &str) -> String {
    format!("{repo}#{remote}")
}

/// The remote of the fetch input key `key`.
pub fn remote_of_key(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(_, remote)| remote)
}

/// Whether a new fetch run is due at `clock`, after the last run `last` of
/// the repository: no run yet, or the last run is not active and started at
/// least `period_s` ago on the fleet clock. A run that never started is due
/// at once; the job queue adds the backoff of an `abandoned` run.
pub fn fetch_due(last: Option<&JobRow>, clock: FleetSeconds, period_s: u64) -> bool {
    let Some(last) = last else {
        return true;
    };
    if matches!(last.state, State::Queued | State::Running) {
        return false;
    }
    // The deadline of a run is its start plus its time limit.
    let Some(deadline) = last.deadline else {
        return true;
    };
    let limit = i64::try_from(last.time_limit_s).unwrap_or(i64::MAX);
    let period = i64::try_from(period_s).unwrap_or(i64::MAX);
    clock.0 >= deadline.0.saturating_sub(limit).saturating_add(period)
}

/// The period rule: queue the next run of the fetch of `repo` when it is
/// due at `clock`, in `tx`. `remote` is the remote of the project file.
/// `None` when nothing is due, and always with `remote = none`.
pub fn queue_fetch(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    repo: &str,
    remote: &Remote,
    clock: FleetSeconds,
) -> Result<Option<Queued>> {
    let Remote::Named(name) = remote else {
        return Ok(None);
    };
    let last = jobs_store::last(tx, Kind::Fetch, repo)?;
    if !fetch_due(last.as_ref(), clock, ctx.config.fetch_period_s) {
        return Ok(None);
    }
    let input_key = fetch_key(repo, name);
    let key = JobKey {
        kind: Kind::Fetch,
        subject: repo,
        input_key: &input_key,
    };
    jobs::queue(ctx, tx, &key, Some(repo), clock).map(Some)
}

/// The state of a pull request, as `gh pr view --json state` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrRead {
    Open,
    /// Closed and not merged.
    Closed,
    /// Merged. fleetd reads a merge from git, not from the pull request.
    Merged,
}

impl PrRead {
    /// The value of `state` of `gh`: `OPEN`, `CLOSED` or `MERGED`.
    pub fn parse(word: &str) -> Option<PrRead> {
        match word {
            "OPEN" => Some(PrRead::Open),
            "CLOSED" => Some(PrRead::Closed),
            "MERGED" => Some(PrRead::Merged),
            _ => None,
        }
    }
}

/// What a passed fetch of 1 repository saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fetched {
    /// Each fetched branch on the remote and its tip.
    pub tips: Vec<(String, String)>,
    /// The pull request state of each feature that the fetch read.
    pub prs: Vec<(String, PrRead)>,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Record the passed fetch of `repo`, in `tx`: each fetched tip, the end of
/// `stale_since`, and the pull request state of each feature.
pub fn record_passed(
    tx: &Connection,
    guard: Option<Generation>,
    needs_you: &dyn NeedsYouItems,
    repo: &str,
    fetched: &Fetched,
) -> Result<()> {
    for (branch, tip) in &fetched.tips {
        rows::put_remote_tip(tx, guard, repo, branch, tip)?;
    }
    rows::set_remote_stale(tx, guard, repo, false)?;
    for (feature, read) in &fetched.prs {
        record_pr(tx, guard, needs_you, feature, *read)?;
    }
    Ok(())
}

/// Record the failed fetch of `repo`, in `tx`: `stale_since` of each of
/// its tips, when it is not set yet.
pub fn record_failed(tx: &Connection, guard: Option<Generation>, repo: &str) -> Result<()> {
    rows::set_remote_stale(tx, guard, repo, true)
}

/// Write the pull request state `read` of `feature`. A closed pull request
/// opens 1 needs-you item; an open one ends it.
fn record_pr(
    tx: &Connection,
    guard: Option<Generation>,
    needs_you: &dyn NeedsYouItems,
    feature: &str,
    read: PrRead,
) -> Result<()> {
    let key = pr_closed_key(feature);
    let state = match read {
        PrRead::Merged => return Ok(()),
        PrRead::Open => {
            needs_you.resolve_item(tx, &key)?;
            "open"
        }
        PrRead::Closed => {
            let text = format!(
                "The pull request of feature {feature} is closed and not merged: \
                 open it again, or run `horch feature abandon {feature}`."
            );
            needs_you.open_item(
                tx,
                &NewItem {
                    key: &key,
                    kind: PR_CLOSED_KIND,
                    entity: Entity::Feature,
                    entity_id: feature,
                    text: &text,
                },
            )?;
            "closed"
        }
    };
    let now = rows::feature(tx, feature)?.map(|f| f.pr_state);
    if now.as_deref() != Some(state) {
        let why = format!("the fetch read the pull request as {state}");
        rows::set_feature_fact(tx, guard, feature, FeatureFact::PrState(state), cause(&why))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::domain::jobs::State;

    fn run(state: State, deadline: Option<i64>) -> JobRow {
        JobRow {
            id: 1,
            kind: Kind::Fetch,
            subject: "/r".into(),
            repo: Some("/r".into()),
            input_key: fetch_key("/r", "origin"),
            run: 1,
            abandoned_runs: 0,
            not_before: None,
            state,
            pid: None,
            pid_start: None,
            time_limit_s: 180,
            deadline: deadline.map(FleetSeconds),
            result_json: None,
            created_at: "now".into(),
            started_at: None,
            ended_at: None,
        }
    }

    #[test]
    fn a_fetch_is_due_1_period_after_the_start_of_the_last_run() {
        // Started at 100: the deadline is 100 + 180.
        let last = run(State::Passed, Some(280));
        assert!(!fetch_due(Some(&last), FleetSeconds(159), 60));
        assert!(fetch_due(Some(&last), FleetSeconds(160), 60));
        let failed = run(State::Failed, Some(280));
        assert!(fetch_due(Some(&failed), FleetSeconds(160), 60));
    }

    #[test]
    fn a_fetch_is_due_with_no_run_and_never_beside_an_active_run() {
        assert!(fetch_due(None, FleetSeconds(0), 60));
        for state in [State::Queued, State::Running] {
            assert!(!fetch_due(
                Some(&run(state, Some(0))),
                FleetSeconds(9999),
                60
            ));
        }
        assert!(fetch_due(
            Some(&run(State::Abandoned, None)),
            FleetSeconds(0),
            60
        ));
    }

    #[test]
    fn the_input_key_names_the_repository_and_its_remote() {
        let key = fetch_key("/a#b/repo", "origin");
        assert_eq!(remote_of_key(&key), "origin");
    }

    #[test]
    fn gh_states_parse() {
        assert_eq!(PrRead::parse("OPEN"), Some(PrRead::Open));
        assert_eq!(PrRead::parse("CLOSED"), Some(PrRead::Closed));
        assert_eq!(PrRead::parse("MERGED"), Some(PrRead::Merged));
        assert_eq!(PrRead::parse("closed"), None);
    }
}
