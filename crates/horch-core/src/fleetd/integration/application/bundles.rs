//! The use cases of bundles and their bisect (tickets 29 and 30).
//!
//! `docs/specs/fleetd/design.md`, "The bundle", "The bundle state machine"
//! (`building`, `gating`), "A moved tip drops a bundle", FM-36 and I12:
//! - [`advance`] is the pass part. When a feature has no open bundle and 1
//!   or more `ready` task branches, it writes 1 `building` bundle on the
//!   feature tip with 1 member for each branch in queue order, moves each
//!   member to `in_bundle` and queues the `bundle_build` job, all in the
//!   transaction of the pass.
//! - Each later pass reads the row of each `building` and `gating` bundle
//!   and the row of its job. It never waits for a job. A passed build moves
//!   the bundle to `gating` and queues the fast gate. A conflict sends the
//!   member to `conflicted`; another failure or a moved feature tip drops
//!   the bundle, and its members go back to `ready`. An abandoned run of
//!   either job runs again.
//! - The verdict of the fast gate (ticket 30): a pass moves the bundle to
//!   `passed`, due after `bundle_passed_limit_s`. A failure moves it to
//!   `failed`, and the bisect writes 1 child bundle with `parent_id` for
//!   each half, built and gated in the same way. A failed bundle of 1
//!   member sends its branch to `gate_failed`; [`failed_gate`] names the
//!   gate job whose result names the gate output.
//! - Each step reads the rows again, so after a restart each bundle goes on
//!   from its row: no bundle is built 2 times, and no member stays
//!   `in_bundle` for ever (FM-36).
//! - A bundle past the time limit of its state has 1 needs-you item, and
//!   the item ends when the bundle is no longer past it.

use std::path::PathBuf;

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
use crate::fleetd::worktree_custody::application::gate_tree::{self, GateTreeRequest};
use crate::taskledger::model::Actor;
use crate::taskledger::runs;

/// The kind of the needs-you item of a bundle past its time limit.
pub const OVERDUE_KIND: &str = "bundle_overdue";

/// The key of the needs-you item of the bundle `id` past its limit.
pub fn overdue_key(id: i64) -> String {
    format!("{OVERDUE_KIND}:{id}")
}

/// What [`advance`] works with: the job context of the pass, and the git
/// gateway that reads the tips of the feature branch and of the members.
pub struct Bundles<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

/// The time limits of the bundle states in `config`.
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

/// The repository, the branch and the run of a feature.
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

/// The pass part of bundles, in the transaction `tx` of part 2 at the fleet
/// clock `clock`: form the new bundles, step each `building` and `gating`
/// bundle, and keep 1 needs-you item for each bundle past its limit.
pub fn advance(b: &Bundles<'_>, tx: &Connection, clock: FleetSeconds) -> Result<()> {
    advance_in(b, tx, clock, None, None)
}

/// [`advance`] for the work budget of 1 pass: form bundles only for the
/// features of `features`, and step only the bundles of `bundles`. `None`
/// is each row.
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

/// The build job of a bundle is a separate process: a pass can drop the
/// bundle while the job runs, and the job can still write the candidate
/// ref after the drop. The ids of the dropped bundles whose build job has
/// not ended, or passed with a candidate that the row does not record yet.
/// A pass follows them, so [`advance_in`] records each such candidate.
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

/// Record the candidate of the passed build job of each dropped bundle of
/// [`late_builds`] on its row, so the deletion of candidate refs deletes
/// the ref. A recorded bundle has its `candidate_tip`, so it is recorded 1
/// time.
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

/// The repository and the branch of `feature_id`, or `None` when the
/// feature has no branch or no run.
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

/// Write 1 `building` bundle of the `ready` task branches of `feature_id`
/// on the tip of its branch `branch`, when the feature has no open bundle.
/// A branch that git does not have waits; its `ready` limit names it.
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
        if let Some(tip) = head_tip(b, &t, &name)? {
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

/// The rows of 1 new bundle: its members in queue order and the tip of
/// each, on `base_tip`; `parent_id` is the failed bundle of a bisect part.
struct Part<'p> {
    feature_id: &'p str,
    base_tip: &'p str,
    parent_id: Option<i64>,
    members: &'p [String],
    tips: &'p [String],
}

/// Write the `building` bundle `part` with its members and queue its
/// `bundle_build` job, in `tx`. Gives the id of the bundle.
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

/// Record the run of `queued` as the job of the bundle `id`, when it is a
/// new run.
fn set_job(ctx: &JobCtx<'_>, tx: &Connection, id: i64, queued: &Queued, why: &str) -> Result<()> {
    if let Queued::New(job) = queued {
        rows::set_bundle_fact(tx, ctx.guard, id, BundleFact::JobId(*job), by_fleetd(why))?;
    }
    Ok(())
}

/// 1 step of the `building` or `gating` bundle `id`.
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

/// What the run `job` of the build says. A bundle with no run has no
/// candidate to wait for, so it is a failed build.
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

/// Queue the next run of the abandoned build `job` of the bundle `id`, with
/// the same input key, after its backoff.
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

/// Queue the fast gate of the `gating` bundle `bundle` on its candidate
/// over its base tip. A pass calls it on each step: a run that is queued,
/// running or has a verdict adds nothing, and an abandoned run runs again.
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
        // The gates of the feature hold for want of a sandbox.
        None => Ok(()),
    }
}

/// The word of the fast gate in `gate_runs`.
const FAST_GATE: &str = "fast";

/// Act on the verdict of the fast gate of the `gating` bundle `bundle`
/// (ticket 30): no verdict queues the gate, a pass moves the bundle to
/// `passed`, and a failure moves it to `failed` and bisects it.
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

/// The verdict of the fast gate of `bundle` on its candidate over its base
/// tip, from its `gate_runs` row. A gate is pure, so the newest row of the
/// same 2 tips is the verdict.
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

/// Write 1 child bundle of the failed `parent` with the members `half`, on
/// the base tip of the parent. Each member stays `in_bundle`, with the due
/// time of a new bundle. A member that git no longer has goes back to
/// `ready`.
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

/// The failed fast gate that sent a task branch to `gate_failed`: the
/// bundle of 1 member that failed, and its gate job. The result of the job
/// names the output of the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailedGate {
    pub bundle_id: i64,
    pub job_id: i64,
}

/// The newest failed gate of `branch` (ticket 33 gives its output to the
/// fix attempt), or `None` when no failed bundle of 1 member holds it.
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

/// Move the bundle `id` to `to` through `transition()`, and write the due
/// time of `to` at `clock`.
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

/// Drop `bundle` after `step`, and move each member to its state.
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

/// Open 1 needs-you item for each bundle past the limit of its state, and
/// end the item of each bundle that is no longer past it.
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

/// Each `open` or `ready` feature with a branch, as (id, branch), by id.
fn live_features(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, branch FROM features
         WHERE state IN ('open', 'ready') AND branch IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The SQL list of the words of `states`, for example `('building')`. The
/// words are the fixed words of [`BundleState`].
fn sql_list(states: impl IntoIterator<Item = BundleState>) -> String {
    let words: Vec<String> = states
        .into_iter()
        .map(|s| format!("'{}'", s.as_str()))
        .collect();
    format!("({})", words.join(", "))
}

/// The ids of the bundles in 1 of `states`, by id.
fn bundles_in(conn: &Connection, states: &[BundleState]) -> Result<Vec<i64>> {
    let sql = format!(
        "SELECT id FROM bundles WHERE state IN {} ORDER BY id",
        sql_list(states.iter().copied())
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// Whether `feature_id` has a bundle that is not ended.
fn has_open_bundle(conn: &Connection, feature_id: &str) -> Result<bool> {
    let open = sql_list(BundleState::ALL.into_iter().filter(|s| s.is_open()));
    Ok(conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM bundles WHERE feature_id = ?1 AND state IN {open})"),
        [feature_id],
        |r| r.get(0),
    )?)
}

/// The `ready` task branches of `feature_id`, in queue order: by
/// `queue_seq`, then in the order of their rows.
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
