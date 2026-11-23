use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, NewItem, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::adapters::exec_bundle::{self, Build};
use crate::fleetd::integration::adapters::exec_gate::{self, GateRequest};
use crate::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BundleFact, BundleRow, NewBundle,
};
use crate::fleetd::integration::application::landing;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::domain::bisector::{self, Decision, Verdict};
use crate::fleetd::integration::domain::bundle::{
    self as rules, BuildFact, BundleState, DropCause, Limits, Step,
};
use crate::fleetd::integration::domain::task_branch::{self, LandingState};
use crate::fleetd::worktree_custody::adapters::sqlite as trees;
use crate::fleetd::worktree_custody::application::gate_tree::{self, GateTreeRequest};
use crate::taskledger::model::Actor;
use crate::taskledger::runs;

pub const OVERDUE_KIND: &str = "bundle_overdue";

pub fn overdue_key(id: i64) -> String {
    format!("{OVERDUE_KIND}:{id}")
}

pub struct Bundles<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

pub fn limits(config: &FleetConfig) -> Limits {
    let jobs = &config.job_limit_s;
    Limits {
        building_s: jobs
            .bundle_build
            .saturating_add(config.landing_job_margin_s),
        gating_s: jobs.fast_gate.saturating_add(config.landing_job_margin_s),
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

struct Target {
    repo: PathBuf,
    branch: String,
    run_id: String,
}

impl Target {
    fn repo_text(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }
}

pub fn advance(b: &Bundles<'_>, tx: &Connection, clock: FleetSeconds) -> Result<()> {
    advance_in(b, tx, clock, None, None)
}

pub fn advance_in(
    b: &Bundles<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    features: Option<&[String]>,
    bundles: Option<&[i64]>,
) -> Result<()> {
    for (feature, branch) in live_features(tx)? {
        if features.is_none_or(|f| f.contains(&feature)) {
            form(b, tx, clock, &feature, &branch)?;
        }
    }
    for id in bundles_in(tx, &[BundleState::Building, BundleState::Gating])? {
        if bundles.is_none_or(|ids| ids.contains(&id)) {
            step(b, tx, clock, id)?;
        }
    }
    late_candidates(b.jobs, tx, bundles)?;
    overdue_items(b.jobs, tx, clock)
}

pub fn late_builds(conn: &Connection) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    for (id, job) in rows::dropped_without_candidate(conn)? {
        let job = jobs_store::get(conn, job)?;
        let late = job.as_ref().is_some_and(|j| j.kind == Kind::BundleBuild)
            && matches!(
                build_fact(job.as_ref()),
                BuildFact::Pending | BuildFact::Built { .. }
            );
        if late {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn late_candidates(ctx: &JobCtx<'_>, tx: &Connection, bundles: Option<&[i64]>) -> Result<()> {
    for id in late_builds(tx)? {
        if bundles.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
        let job = match bundle.job_id {
            Some(job) => jobs_store::get(tx, job)?,
            None => None,
        };
        let BuildFact::Built {
            candidate_ref,
            candidate_tip,
        } = build_fact(job.as_ref())
        else {
            continue;
        };
        let why = "its build job wrote the candidate after the drop";
        let facts = [
            BundleFact::CandidateRef(Some(&candidate_ref)),
            BundleFact::CandidateTip(&candidate_tip),
        ];
        for fact in facts {
            rows::set_bundle_fact(tx, ctx.guard, id, fact, by_fleetd(why))?;
        }
    }
    Ok(())
}

fn target(tx: &Connection, feature_id: &str) -> Result<Option<Target>> {
    let Some(branch) = rows::feature(tx, feature_id)?.and_then(|f| f.branch) else {
        return Ok(None);
    };
    Ok(runs::feature_run(tx, feature_id)?.map(|run| Target {
        repo: PathBuf::from(run.project_root),
        branch,
        run_id: run.run_id,
    }))
}

fn head_tip(b: &Bundles<'_>, t: &Target, branch: &str) -> Result<Option<String>> {
    b.git.tip(&t.repo, &format!("refs/heads/{branch}"))
}

fn form(
    b: &Bundles<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    feature_id: &str,
    branch: &str,
) -> Result<()> {
    if has_open_bundle(tx, feature_id)? {
        return Ok(());
    }
    let ready = ready_branches(tx, feature_id)?;
    if ready.is_empty() {
        return Ok(());
    }
    let Some(t) = target(tx, feature_id)? else {
        return Ok(());
    };
    let Some(base) = head_tip(b, &t, branch)? else {
        return Ok(());
    };
    let mut members = Vec::new();
    let mut tips = Vec::new();
    for name in ready {
        if let Some(tip) = member_tip(b, tx, &t, &base, &name)? {
            members.push(name);
            tips.push(tip);
        }
    }
    if members.is_empty() {
        return Ok(());
    }
    let ctx = b.jobs;
    let why = format!("{} ready task branches of {feature_id}", members.len());
    let part = Part {
        feature_id,
        base_tip: &base,
        parent_id: None,
        members: &members,
        tips: &tips,
    };
    let id = insert(ctx, tx, clock, &t, &part, &why)?;
    let why = format!("it is a member of bundle {id}");
    for name in &members {
        landing::enter(ctx, tx, name, LandingState::InBundle, clock, &why)?;
    }
    Ok(())
}

pub const NO_TIP_KIND: &str = "branch_tip_missing";

pub fn no_tip_key(branch: &str) -> String {
    format!("{NO_TIP_KIND}:{branch}")
}

fn member_tip(
    b: &Bundles<'_>,
    tx: &Connection,
    t: &Target,
    base: &str,
    name: &str,
) -> Result<Option<String>> {
    let ctx = b.jobs;
    let key = no_tip_key(name);
    if let Some(tip) = head_tip(b, t, name)? {
        ctx.needs_you.resolve_item(tx, &key)?;
        return Ok(Some(tip));
    }
    if let Some(tip) = recorded_tip(b.git, tx, &t.repo, name)? {
        if b.git.is_ancestor(&t.repo, &tip, base)? {
            ctx.needs_you.resolve_item(tx, &key)?;
            return Ok(Some(tip));
        }
    }
    let text = format!(
        "fleetd cannot land the ready task branch {name}: git has no branch {name}, and \
         the ledger records no tip of it that is in {}. Restore the branch, or run \
         horch branch abandon {name}.",
        t.branch
    );
    ctx.needs_you.open_item(
        tx,
        &NewItem {
            key: &key,
            kind: NO_TIP_KIND,
            entity: Entity::TaskBranch,
            entity_id: name,
            text: &text,
        },
    )?;
    Ok(None)
}

pub(crate) fn recorded_tip(
    git: &dyn IntegrationGitGateway,
    tx: &Connection,
    repo: &Path,
    name: &str,
) -> Result<Option<String>> {
    let row = rows::branch(tx, name)?.with_context(|| format!("no task branch {name}"))?;
    let rescued = match row.rescue_ref.as_deref() {
        Some(r) => git.tip(repo, r)?,
        None => None,
    };
    match row.seen_tip.or(rescued) {
        Some(tip) => Ok(Some(tip)),
        None => trees::recorded_tip(tx, name),
    }
}

struct Part<'p> {
    feature_id: &'p str,
    base_tip: &'p str,
    parent_id: Option<i64>,
    members: &'p [String],
    tips: &'p [String],
}

fn insert(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    t: &Target,
    part: &Part<'_>,
    why: &str,
) -> Result<i64> {
    let member_names: Vec<&str> = part.members.iter().map(String::as_str).collect();
    let due_at = rules::due_at(BundleState::Building, clock.0, &limits(ctx.config))
        .context("building has a time limit")?;
    let new = NewBundle {
        feature_id: part.feature_id,
        base_tip: part.base_tip,
        parent_id: part.parent_id,
        members: &member_names,
        due_at,
    };
    let id = rows::insert_bundle(tx, ctx.guard, &new, by_fleetd(why))?;
    let tips: Vec<&str> = part.tips.iter().map(String::as_str).collect();
    let input_key = exec_bundle::input_key(part.base_tip, &tips);
    let subject = id.to_string();
    let key = JobKey {
        kind: Kind::BundleBuild,
        subject: &subject,
        input_key: &input_key,
    };
    let queued = jobs::queue(ctx, tx, &key, Some(&t.repo_text()), clock)?;
    set_job(ctx, tx, id, &queued, "the build of its candidate is queued")?;
    Ok(id)
}

fn set_job(ctx: &JobCtx<'_>, tx: &Connection, id: i64, queued: &Queued, why: &str) -> Result<()> {
    if let Queued::New(job) = queued {
        rows::set_bundle_fact(tx, ctx.guard, id, BundleFact::JobId(*job), by_fleetd(why))?;
    }
    Ok(())
}

fn step(b: &Bundles<'_>, tx: &Connection, clock: FleetSeconds, id: i64) -> Result<()> {
    let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
    let Some(t) = target(tx, &bundle.feature_id)? else {
        return Ok(());
    };
    let tip = head_tip(b, &t, &t.branch)?;
    let moved = tip.as_deref() != Some(bundle.base_tip.as_str());
    let state = BundleState::parse(&bundle.state)
        .with_context(|| format!("{:?} is not a bundle state", bundle.state))?;
    let job = match bundle.job_id {
        Some(job) => jobs_store::get(tx, job)?,
        None => None,
    };
    let next = match state {
        BundleState::Building => rules::building(moved, build_fact(job.as_ref())),
        BundleState::Gating => rules::gating(moved),
        _ => Step::Stay,
    };
    let ctx = b.jobs;
    match &next {
        Step::Stay if state == BundleState::Gating => judge(b, tx, clock, &bundle, &t),
        Step::Stay => Ok(()),
        Step::Rebuild => rebuild(ctx, tx, clock, id, job.as_ref()),
        Step::Gate {
            candidate_ref,
            candidate_tip,
        } => {
            let why = "its build job passed";
            let facts = [
                BundleFact::CandidateRef(Some(candidate_ref)),
                BundleFact::CandidateTip(candidate_tip),
            ];
            for fact in facts {
                rows::set_bundle_fact(tx, ctx.guard, id, fact, by_fleetd(why))?;
            }
            enter(ctx, tx, clock, id, BundleState::Gating, why)?;
            let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
            gate(ctx, tx, clock, &bundle, &t)
        }
        Step::Conflict { member } => {
            let why = format!("the merge of {member} conflicts in its build");
            leave(ctx, tx, clock, &bundle, &next, &why)
        }
        Step::Drop(cause) => {
            let why = match cause {
                DropCause::TipMoved => format!(
                    "the tip of {} is not its base tip {}",
                    t.branch, bundle.base_tip
                ),
                DropCause::BuildFailed => "its build job failed".to_string(),
            };
            leave(ctx, tx, clock, &bundle, &next, &why)
        }
    }
}

fn build_fact(job: Option<&JobRow>) -> BuildFact {
    let Some(job) = job else {
        return BuildFact::Failed;
    };
    let data = || jobs::result_data(job).and_then(|d| serde_json::from_value::<Build>(d).ok());
    match job.state {
        State::Queued | State::Running => BuildFact::Pending,
        State::Abandoned => BuildFact::Abandoned,
        State::Passed => match data() {
            Some(Build::Candidate {
                candidate_ref,
                candidate_tip,
            }) => BuildFact::Built {
                candidate_ref,
                candidate_tip,
            },
            _ => BuildFact::Failed,
        },
        State::Failed => match data() {
            Some(Build::Conflict { member, .. }) => BuildFact::Conflict { member },
            _ => BuildFact::Failed,
        },
    }
}

fn rebuild(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    id: i64,
    job: Option<&JobRow>,
) -> Result<()> {
    let job = job.with_context(|| format!("bundle {id} has no build job to run again"))?;
    let key = JobKey {
        kind: Kind::BundleBuild,
        subject: &job.subject,
        input_key: &job.input_key,
    };
    let queued = jobs::queue(ctx, tx, &key, job.repo.as_deref(), clock)?;
    set_job(ctx, tx, id, &queued, "its build job ended abandoned")
}

fn gate(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
    t: &Target,
) -> Result<()> {
    let tip = bundle
        .candidate_tip
        .as_deref()
        .with_context(|| format!("bundle {} is gating with no candidate tip", bundle.id))?;
    let tree = GateTreeRequest {
        feature_id: &bundle.feature_id,
        run_id: &t.run_id,
        repo: &t.repo,
        base: &t.branch,
        tip: &bundle.base_tip,
    };
    if !gate_tree::ready(ctx, tx, &tree, clock)? {
        return Ok(());
    }
    let subject = bundle.id.to_string();
    let req = GateRequest {
        kind: Kind::FastGate,
        subject: &subject,
        feature_id: &bundle.feature_id,
        tip,
        base_tip: &bundle.base_tip,
        repo: &t.repo,
    };
    match exec_gate::queue(ctx, tx, &req, clock)? {
        Some(queued) => set_job(ctx, tx, bundle.id, &queued, "its fast gate is queued"),
        None => Ok(()),
    }
}

const FAST_GATE: &str = "fast";

fn judge(
    b: &Bundles<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
    t: &Target,
) -> Result<()> {
    let ctx = b.jobs;
    match bisector::decide(verdict(tx, bundle)?, &bundle.members) {
        Decision::Wait => gate(ctx, tx, clock, bundle, t),
        Decision::Pass => {
            let why = "its fast gate passed";
            enter(ctx, tx, clock, bundle.id, BundleState::Passed, why)?;
            let due = clock.0.saturating_add(
                i64::try_from(ctx.config.bundle_passed_limit_s).unwrap_or(i64::MAX),
            );
            rows::set_bundle_fact(
                tx,
                ctx.guard,
                bundle.id,
                BundleFact::DueAt(due),
                by_fleetd(why),
            )
        }
        Decision::GateFailed { member } => {
            enter(
                ctx,
                tx,
                clock,
                bundle.id,
                BundleState::Failed,
                "its fast gate failed",
            )?;
            let why = format!(
                "it is the failing branch of bundle {}: the output of its fast gate is the result of job {}",
                bundle.id,
                bundle.job_id.unwrap_or_default()
            );
            landing::enter(ctx, tx, &member, LandingState::GateFailed, clock, &why)?;
            Ok(())
        }
        Decision::Split { left, right } => {
            enter(
                ctx,
                tx,
                clock,
                bundle.id,
                BundleState::Failed,
                "its fast gate failed",
            )?;
            for half in [left, right] {
                bisect_part(b, tx, clock, bundle, t, &half)?;
            }
            Ok(())
        }
    }
}

fn verdict(tx: &Connection, bundle: &BundleRow) -> Result<Verdict> {
    let Some(tip) = bundle.candidate_tip.as_deref() else {
        return Ok(Verdict::Pending);
    };
    let run = rows::gate_run_of(tx, &bundle.feature_id, FAST_GATE, tip, &bundle.base_tip)?;
    Ok(match run.and_then(|r| r.result).as_deref() {
        Some("pass") => Verdict::Passed,
        Some("fail") => Verdict::Failed,
        _ => Verdict::Pending,
    })
}

fn bisect_part(
    b: &Bundles<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    parent: &BundleRow,
    t: &Target,
    half: &[String],
) -> Result<()> {
    let ctx = b.jobs;
    let mut members = Vec::new();
    let mut tips = Vec::new();
    for name in half {
        match head_tip(b, t, name)? {
            Some(tip) => {
                members.push(name.clone());
                tips.push(tip);
            }
            None => {
                let why = format!("bundle {} failed, and git has no branch {name}", parent.id);
                landing::enter(ctx, tx, name, LandingState::Ready, clock, &why)?;
            }
        }
    }
    if members.is_empty() {
        return Ok(());
    }
    let why = format!("a bisect part of the failed bundle {}", parent.id);
    let part = Part {
        feature_id: &parent.feature_id,
        base_tip: &parent.base_tip,
        parent_id: Some(parent.id),
        members: &members,
        tips: &tips,
    };
    let id = insert(ctx, tx, clock, t, &part, &why)?;
    let due = task_branch::due_at(
        LandingState::InBundle,
        clock.0,
        &landing::limits(ctx.config),
    );
    if let Some(due) = due {
        let why = format!(
            "it is a member of bundle {id}, a bisect part of bundle {}",
            parent.id
        );
        for name in &members {
            rows::set_branch_fact(tx, ctx.guard, name, BranchFact::DueAt(due), by_fleetd(&why))?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailedGate {
    pub bundle_id: i64,
    pub job_id: i64,
}

pub fn failed_gate(conn: &Connection, branch: &str) -> Result<Option<FailedGate>> {
    let found = conn
        .query_row(
            "SELECT b.id, b.job_id FROM bundles b JOIN bundle_members m ON m.bundle_id = b.id
             WHERE m.branch = ?1 AND b.state = ?2 AND b.job_id IS NOT NULL
               AND (SELECT COUNT(*) FROM bundle_members n WHERE n.bundle_id = b.id) = 1
             ORDER BY b.id DESC LIMIT 1",
            params![branch, BundleState::Failed.as_str()],
            |r| {
                Ok(FailedGate {
                    bundle_id: r.get(0)?,
                    job_id: r.get(1)?,
                })
            },
        )
        .optional()?;
    Ok(found)
}

fn enter(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    id: i64,
    to: BundleState,
    why: &str,
) -> Result<()> {
    let key = id.to_string();
    transition(
        tx,
        ctx.guard,
        Entity::Bundle,
        &key,
        to.as_str(),
        by_fleetd(why),
        ctx.needs_you,
    )?;
    if let Some(due) = rules::due_at(to, clock.0, &limits(ctx.config)) {
        rows::set_bundle_fact(tx, ctx.guard, id, BundleFact::DueAt(due), by_fleetd(why))?;
    }
    Ok(())
}

fn leave(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
    step: &Step,
    why: &str,
) -> Result<()> {
    enter(ctx, tx, clock, bundle.id, BundleState::Dropped, why)?;
    let why = format!("bundle {} is dropped: {why}", bundle.id);
    for (member, to) in rules::member_states(step, &bundle.members) {
        landing::enter(ctx, tx, &member, to, clock, &why)?;
    }
    Ok(())
}

fn overdue_items(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<()> {
    let limits = limits(ctx.config);
    let overdue = |bundle: &BundleRow| {
        BundleState::parse(&bundle.state)
            .is_some_and(|s| rules::overdue(s, bundle.due_at, clock.0, &limits))
    };
    for key in needs_you::open_keys(tx, OVERDUE_KIND)? {
        let id = key
            .strip_prefix(&format!("{OVERDUE_KIND}:"))
            .and_then(|id| id.parse::<i64>().ok());
        let still = match id {
            Some(id) => rows::bundle(tx, id)?.is_some_and(|b| overdue(&b)),
            None => false,
        };
        if !still {
            ctx.needs_you.resolve_item(tx, &key)?;
        }
    }
    for id in bundles_in(tx, &[BundleState::Building, BundleState::Gating])? {
        let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
        if !overdue(&bundle) {
            continue;
        }
        let key = overdue_key(id);
        let text = format!(
            "Bundle {id} of {} is {} past its time limit. Read the events of the bundle and of its job.",
            bundle.feature_id, bundle.state
        );
        ctx.needs_you.open_item(
            tx,
            &NewItem {
                key: &key,
                kind: OVERDUE_KIND,
                entity: Entity::Bundle,
                entity_id: &id.to_string(),
                text: &text,
            },
        )?;
    }
    Ok(())
}

fn live_features(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, branch FROM features
         WHERE state IN ('open', 'ready') AND branch IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn sql_list(states: impl IntoIterator<Item = BundleState>) -> String {
    let words: Vec<String> = states
        .into_iter()
        .map(|s| format!("'{}'", s.as_str()))
        .collect();
    format!("({})", words.join(", "))
}

fn bundles_in(conn: &Connection, states: &[BundleState]) -> Result<Vec<i64>> {
    let sql = format!(
        "SELECT id FROM bundles WHERE state IN {} ORDER BY id",
        sql_list(states.iter().copied())
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

fn has_open_bundle(conn: &Connection, feature_id: &str) -> Result<bool> {
    let open = sql_list(BundleState::ALL.into_iter().filter(|s| s.is_open()));
    Ok(conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM bundles WHERE feature_id = ?1 AND state IN {open})"),
        [feature_id],
        |r| r.get(0),
    )?)
}

fn ready_branches(conn: &Connection, feature_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM branches
         WHERE feature_id = ?1 AND task_id IS NOT NULL AND landing_state = ?2
         ORDER BY queue_seq IS NULL, queue_seq, rowid",
    )?;
    let names = stmt.query_map(params![feature_id, LandingState::Ready.as_str()], |r| {
        r.get(0)
    })?;
    Ok(names.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_the_job_limit_plus_the_margin() {
        let mut config = FleetConfig::default();
        config.job_limit_s.bundle_build = 100;
        config.job_limit_s.fast_gate = 200;
        config.landing_job_margin_s = 7;
        assert_eq!(
            limits(&config),
            Limits {
                building_s: 107,
                gating_s: 207,
            }
        );
    }

    #[test]
    fn overdue_key_names_the_bundle() {
        assert_eq!(overdue_key(7), "bundle_overdue:7");
    }
}
