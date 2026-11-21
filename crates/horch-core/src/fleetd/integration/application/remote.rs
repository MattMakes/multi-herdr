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

pub const PR_CLOSED_KIND: &str = "pr_closed";

pub fn pr_closed_key(feature: &str) -> String {
    format!("{PR_CLOSED_KIND}:{feature}")
}

pub fn fetch_key(repo: &str, remote: &str) -> String {
    format!("{repo}#{remote}")
}

pub fn remote_of_key(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(_, remote)| remote)
}

pub fn fetch_due(last: Option<&JobRow>, clock: FleetSeconds, period_s: u64) -> bool {
    let Some(last) = last else {
        return true;
    };
    if matches!(last.state, State::Queued | State::Running) {
        return false;
    }
    let Some(deadline) = last.deadline else {
        return true;
    };
    let limit = i64::try_from(last.time_limit_s).unwrap_or(i64::MAX);
    let period = i64::try_from(period_s).unwrap_or(i64::MAX);
    clock.0 >= deadline.0.saturating_sub(limit).saturating_add(period)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrRead {
    Open,
    Closed,
    Merged,
}

impl PrRead {
    pub fn parse(word: &str) -> Option<PrRead> {
        match word {
            "OPEN" => Some(PrRead::Open),
            "CLOSED" => Some(PrRead::Closed),
            "MERGED" => Some(PrRead::Merged),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fetched {
    pub tips: Vec<(String, String)>,
    pub prs: Vec<(String, PrRead)>,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

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

pub fn record_failed(tx: &Connection, guard: Option<Generation>, repo: &str) -> Result<()> {
    rows::set_remote_stale(tx, guard, repo, true)
}

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

pub const PUSH_FAILED_KIND: &str = "push_failed";

pub fn push_failed_key(feature: &str) -> String {
    format!("{PUSH_FAILED_KIND}:{feature}")
}

const NO_REMOTE_TIP: &str = "none";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushInput {
    pub tip: String,
    pub seen: Option<String>,
    pub remote: String,
}

impl PushInput {
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

pub fn push_key(remote: &str, tip: &str, seen: Option<&str>) -> String {
    format!("{tip} {} {remote}", seen.unwrap_or(NO_REMOTE_TIP))
}

pub fn push_due<'a>(
    feature: &FeatureRow,
    branch: &'a BranchRow,
    rebased: bool,
    clock: FleetSeconds,
) -> Option<&'a str> {
    let tip = branch.seen_tip.as_deref()?;
    let pr_missing = feature.state == "ready" && feature.pr.is_none();
    if branch.seen_remote_tip.as_deref() == Some(tip) && !pr_missing {
        return None;
    }
    let wanted = rebased || feature.state == "ready" || feature.pr_state == "open";
    let waits = feature.push_not_before.is_some_and(|end| clock.0 < end);
    (wanted && !waits).then_some(tip)
}

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
        let same = branch(Some("b1"), Some("b1"));
        let with_pr = FeatureRow {
            pr: Some("https://example.com/pull/1".into()),
            ..feature("ready", "open", None)
        };
        assert_eq!(push_due(&with_pr, &same, true, at), None);
        assert_eq!(
            push_due(&feature("open", "open", None), &same, true, at),
            None
        );
        assert_eq!(
            push_due(&feature("ready", "none", None), &same, false, at),
            Some("b1")
        );
        let none = branch(None, None);
        assert_eq!(
            push_due(&feature("ready", "open", None), &none, true, at),
            None
        );
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
