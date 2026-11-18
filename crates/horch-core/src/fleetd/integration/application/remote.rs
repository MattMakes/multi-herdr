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
//!
//! "Push and pull request results are facts on the feature row": 1 `push`
//! job pushes the feature branch. Its input key is the feature tip and
//! `seen_remote_tip` ([`push_key`]), and the push is a compare-and-set on
//! `seen_remote_tip` (I9, `--force-with-lease`). [`queue_push`] is the rule
//! of when to push (MAP.md of fleetd-p4, Q21). A passed run records
//! [`record_push_passed`], a failed run [`record_push_failed`]: a backoff
//! on the fleet clock, and at `push_failure_limit` failures in a row 1
//! needs-you item (FM-43).

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, JobCtx, JobKey, NeedsYouItems, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{self as job_rules, Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BranchRow, FeatureFact, FeatureRow,
};
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

// ─── push ───────────────────────────────────────────────────────────────

/// The kind of the needs-you item of a push that failed
/// `push_failure_limit` times in a row.
pub const PUSH_FAILED_KIND: &str = "push_failed";

/// The key of the needs-you item of the failed pushes of `feature`.
pub fn push_failed_key(feature: &str) -> String {
    format!("{PUSH_FAILED_KIND}:{feature}")
}

/// The word of the input key for "no `seen_remote_tip`": the remote has no
/// branch that fleetd saw.
const NO_REMOTE_TIP: &str = "none";

/// The input of 1 push: the tip to push, the remote tip that fleetd last
/// saw (the lease), and the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushInput {
    pub tip: String,
    /// `None`: the remote must not have the branch.
    pub seen: Option<String>,
    pub remote: String,
}

impl PushInput {
    /// The input of the push input key `key`, as [`push_key`] writes it.
    pub fn parse(key: &str) -> Option<PushInput> {
        let mut parts = key.splitn(3, ' ');
        let (tip, seen, remote) = (parts.next()?, parts.next()?, parts.next()?);
        if tip.is_empty() || seen.is_empty() || remote.is_empty() {
            return None;
        }
        Some(PushInput {
            tip: tip.to_string(),
            seen: (seen != NO_REMOTE_TIP).then(|| seen.to_string()),
            remote: remote.to_string(),
        })
    }
}

/// The input key of the push of `tip` to `remote` with the lease `seen`
/// ("The kinds of job": the feature tip and `seen_remote_tip`).
pub fn push_key(remote: &str, tip: &str, seen: Option<&str>) -> String {
    format!("{tip} {} {remote}", seen.unwrap_or(NO_REMOTE_TIP))
}

/// The tip to push of the feature `feature` with the branch row `branch`
/// at `clock`, or `None`. The local tip (`seen_tip`) is pushed when it is
/// not `seen_remote_tip`, the feature is `ready`, has an open pull request
/// or was just rebased (Q21), and its push backoff has ended.
pub fn push_due<'a>(
    feature: &FeatureRow,
    branch: &'a BranchRow,
    rebased: bool,
    clock: FleetSeconds,
) -> Option<&'a str> {
    let tip = branch.seen_tip.as_deref()?;
    if branch.seen_remote_tip.as_deref() == Some(tip) {
        return None;
    }
    let wanted = rebased || feature.state == "ready" || feature.pr_state == "open";
    let waits = feature.push_not_before.is_some_and(|end| clock.0 < end);
    (wanted && !waits).then_some(tip)
}

/// The push rule: queue a run of the push of `feature` of the repository
/// `repo` when [`push_due`] gives a tip at `clock`, in `tx`. `remote` is
/// the remote of the project file, and `rebased` is `true` after a rebase
/// of the feature branch (ticket 41). `None` when nothing is due, and
/// always with `remote = none`.
pub fn queue_push(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    repo: &str,
    feature: &str,
    remote: &Remote,
    rebased: bool,
    clock: FleetSeconds,
) -> Result<Option<Queued>> {
    let Remote::Named(name) = remote else {
        return Ok(None);
    };
    let Some(f) = rows::feature(tx, feature)? else {
        return Ok(None);
    };
    let Some(b) = f
        .branch
        .as_deref()
        .map(|n| rows::branch(tx, n))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let Some(tip) = push_due(&f, &b, rebased, clock) else {
        return Ok(None);
    };
    let input_key = push_key(name, tip, b.seen_remote_tip.as_deref());
    let key = JobKey {
        kind: Kind::Push,
        subject: feature,
        input_key: &input_key,
    };
    jobs::queue(ctx, tx, &key, Some(repo), clock).map(Some)
}

/// Record the passed push of `tip` of `feature`, in `tx`: `seen_remote_tip`
/// is `tip`, `push_state` is `ok`, the failures and the backoff end, and so
/// does the needs-you item of the failed pushes.
pub fn record_push_passed(
    tx: &Connection,
    guard: Option<Generation>,
    needs_you: &dyn NeedsYouItems,
    feature: &str,
    tip: &str,
) -> Result<()> {
    let f = feature_row(tx, feature)?;
    if let Some(branch) = f.branch.as_deref() {
        let why = format!("the push job pushed {tip}");
        rows::set_branch_fact(
            tx,
            guard,
            branch,
            BranchFact::SeenRemoteTip(tip),
            cause(&why),
        )?;
    }
    let why = "the push job passed";
    if f.push_state.as_deref() != Some("ok") {
        rows::set_feature_fact(tx, guard, feature, FeatureFact::PushState("ok"), cause(why))?;
    }
    if f.push_failures != 0 {
        rows::set_feature_fact(tx, guard, feature, FeatureFact::PushFailures(0), cause(why))?;
    }
    if f.push_not_before.is_some() {
        rows::set_feature_fact(
            tx,
            guard,
            feature,
            FeatureFact::PushNotBefore(None),
            cause(why),
        )?;
    }
    needs_you.resolve_item(tx, &push_failed_key(feature))?;
    Ok(())
}

/// Record the failed push of `feature` at `clock`, in `tx`, for the reason
/// `why`: `push_state` is `failed`, `push_failures` grows by 1, and
/// `push_not_before` is the end of its backoff (`job_backoff_s`). At
/// `push_failure_limit` failures in a row, 1 needs-you item is open.
/// `seen_remote_tip` stays: a remote that moved is not overwritten later.
pub fn record_push_failed(
    tx: &Connection,
    guard: Option<Generation>,
    needs_you: &dyn NeedsYouItems,
    config: &FleetConfig,
    feature: &str,
    clock: FleetSeconds,
    why: &str,
) -> Result<()> {
    let f = feature_row(tx, feature)?;
    let failures = f.push_failures.saturating_add(1);
    let n = u64::try_from(failures).unwrap_or(u64::MAX);
    let end = job_rules::deadline(clock, job_rules::backoff_s(&config.job_backoff_s, n));
    let text = format!("the push job failed: {why}");
    if f.push_state.as_deref() != Some("failed") {
        rows::set_feature_fact(
            tx,
            guard,
            feature,
            FeatureFact::PushState("failed"),
            cause(&text),
        )?;
    }
    for fact in [
        FeatureFact::PushFailures(failures),
        FeatureFact::PushNotBefore(Some(end.0)),
    ] {
        rows::set_feature_fact(tx, guard, feature, fact, cause(&text))?;
    }
    if n >= config.push_failure_limit {
        let text = format!(
            "The push of feature {feature} failed {failures} times in a row. \
             The last failure: {why}. fleetd tries again after each backoff."
        );
        needs_you.open_item(
            tx,
            &NewItem {
                key: &push_failed_key(feature),
                kind: PUSH_FAILED_KIND,
                entity: Entity::Feature,
                entity_id: feature,
                text: &text,
            },
        )?;
    }
    Ok(())
}

fn feature_row(tx: &Connection, feature: &str) -> Result<FeatureRow> {
    rows::feature(tx, feature)?.ok_or_else(|| anyhow::anyhow!("no feature {feature} in the ledger"))
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

    fn feature(state: &str, pr_state: &str, not_before: Option<i64>) -> FeatureRow {
        FeatureRow {
            id: "F-1".into(),
            name: None,
            branch: Some("b".into()),
            primary_branch: "main".into(),
            stacked_on: None,
            state: state.into(),
            pr: None,
            push_state: None,
            push_failures: 0,
            push_not_before: not_before,
            pr_state: pr_state.into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    fn branch(seen_tip: Option<&str>, seen_remote_tip: Option<&str>) -> BranchRow {
        BranchRow {
            name: "b".into(),
            feature_id: "F-1".into(),
            task_id: None,
            landing_state: None,
            owner: "fleetd".into(),
            due_at: 0,
            queue_seq: None,
            repair_attempts: 0,
            rescue_ref: None,
            seen_tip: seen_tip.map(Into::into),
            seen_remote_tip: seen_remote_tip.map(Into::into),
            orphan: false,
        }
    }

    #[test]
    fn a_push_is_due_for_a_new_tip_of_a_ready_feature_an_open_pr_or_a_rebase() {
        let at = FleetSeconds(100);
        let new = branch(Some("b2"), Some("b1"));
        assert_eq!(
            push_due(&feature("ready", "none", None), &new, false, at),
            Some("b2")
        );
        assert_eq!(
            push_due(&feature("open", "open", None), &new, false, at),
            Some("b2")
        );
        assert_eq!(
            push_due(&feature("open", "none", None), &new, true, at),
            Some("b2")
        );
        assert_eq!(
            push_due(&feature("open", "none", None), &new, false, at),
            None
        );
        // The remote has the tip, or fleetd saw no local tip.
        let same = branch(Some("b1"), Some("b1"));
        assert_eq!(
            push_due(&feature("ready", "open", None), &same, true, at),
            None
        );
        let none = branch(None, None);
        assert_eq!(
            push_due(&feature("ready", "open", None), &none, true, at),
            None
        );
        // The backoff of a failed push.
        assert_eq!(
            push_due(&feature("ready", "none", Some(101)), &new, false, at),
            None
        );
        assert_eq!(
            push_due(&feature("ready", "none", Some(100)), &new, false, at),
            Some("b2")
        );
    }

    #[test]
    fn a_push_key_names_the_tip_the_lease_and_the_remote() {
        let key = push_key("origin", "t", Some("s"));
        let input = PushInput::parse(&key).unwrap();
        assert_eq!(
            (input.tip.as_str(), input.seen.as_deref()),
            ("t", Some("s"))
        );
        assert_eq!(input.remote, "origin");
        let first = PushInput::parse(&push_key("up", "t", None)).unwrap();
        assert_eq!(first.seen, None);
        assert_eq!(PushInput::parse("t s"), None);
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
