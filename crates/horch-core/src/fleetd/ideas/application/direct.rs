//! horch idea add --direct: a plan of 1 work task, plus its judge task for
//! a judgment kind (ticket 17 of fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "Everything is an idea, with a fast
//! lane"; FR-10, FR-12; MAP.md of fleetd-p6, Q6 and Q8. A ticket that you
//! wrote skips research and the interview:
//!
//! - [`add_direct_idea`] (`AddDirectIdea`) writes, in the caller's write
//!   transaction, the idea in `captured` (`AddIdea`, with the ticket as its
//!   text), the branch name of a new feature from the ticket title
//!   (`NameFeatureBranch`), and your approval: `approved_sha256` is the
//!   sha256 of the copied ticket, because the ticket stands for your
//!   approval. The caller commits, then writes the files of
//!   [`DirectAdded`]: idea.md, `tickets/01-<name>.md`, plan.yaml,
//!   approval.json and the feature branch.
//! - plan.yaml has 1 work task: its profile from the Persona line and its
//!   check from the Check line. For a judgment kind fleetd adds the judge
//!   task: kind `judge`, `judges:` the work task, and the first healthy
//!   profile of another provider of the roster with a sandbox on this OS.
//! - [`start_direct`] moves the idea to `ticketing` under the rule of the
//!   feature, then validates its plan (`ValidatePlan`). A refused plan
//!   opens 1 needs-you item with the reason, and the idea stays in
//!   `ticketing`.
//! - [`direct_requirement`] gives the plan validator the 1 requirement of a
//!   `--direct` idea: the approved ticket, which cites itself.
//! - [`is_direct`] reads from the ledger whether an idea is a `--direct`
//!   idea, and [`files_written`] tells whether the effects after the
//!   commit wrote its files. The dispatcher decides from both (ticket 58
//!   of fleetd-build): plan.yaml alone is an effect after the commit (A24),
//!   so a pass before the effects saw no plan.yaml and moved the idea to
//!   `clarifying`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::sqlite::fact_data;
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::approval::{
    record_approval, trusted_approval, ApprovalFile,
};
use crate::fleetd::ideas::application::intake::{
    add_idea, write_idea_file, Added, Target, ADDED_CAUSE,
};
use crate::fleetd::ideas::application::needs_you::open_item;
use crate::fleetd::ideas::application::plan_commit::{
    validate_plan, PlanCommit, Verdict, PLAN_YAML,
};
use crate::fleetd::ideas::domain::approval::{Approval, APPROVAL_JSON, BY_OPERATOR};
use crate::fleetd::ideas::domain::idea::FIRST;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::ideas::domain::plan::Kind;
use crate::fleetd::ideas::domain::plan_validator::{Problem, Requirement, RosterProfile};
use crate::fleetd::integration::application::new_features::{
    create_ref, name_feature_branch, name_from_title, BranchRef, NameSource, Named,
};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::worker_profiles::application::health;
use crate::fsx;
use crate::taskledger::map::ticket_facts;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::{runs, util, Store};

/// The source of the idea.md of a `--direct` idea.
pub const SOURCE: &str = "horch idea add --direct";

/// The folder of the copied ticket, in the idea folder.
pub const TICKETS: &str = "tickets";

/// The id of the 1 requirement of a `--direct` idea.
pub const REQUIREMENT: &str = "R1";

/// The kind of the needs-you item of a refused `--direct` plan.
pub const PLAN_REFUSED_KIND: &str = "direct_plan_refused";

/// The state of an idea with a plan to validate.
const TICKETING: &str = "ticketing";

/// The states in which an idea of a feature plans: while 1 idea of the
/// feature is in one of them, another waits in `captured`.
const PLANNING: [&str; 3] = ["clarifying", "specifying", "ticketing"];

/// The mode of the tickets folder, and of each file that fleetd writes.
const FILE_MODE: u32 = 0o600;

/// The key of the needs-you item of the refused plan of `idea`.
pub fn plan_refused_key(idea: &str) -> String {
    format!("{PLAN_REFUSED_KIND}:{idea}")
}

/// What [`add_direct_idea`] needs besides the ledger.
pub struct Direct<'a> {
    /// The profiles of the roster: the judge task gets 1 of them.
    pub roster: &'a [RosterProfile],
    /// The git of the project, to name a new feature branch.
    pub git: &'a dyn IntegrationGitGateway,
    /// The fetched tip of the primary branch: a new feature branch starts
    /// there.
    pub primary_tip: &'a str,
    /// The fleet clock.
    pub clock: FleetSeconds,
}

/// What [`add_direct_idea`] wrote, and the files to write after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectAdded {
    /// The idea, its feature and its idea.md.
    pub idea: Added,
    /// The copied ticket, a path in the idea folder.
    pub ticket: String,
    /// The text of the copied ticket.
    pub ticket_text: String,
    /// The text of plan.yaml.
    pub plan: String,
    /// approval.json.
    pub approval: ApprovalFile,
    /// The branch of a new feature, to create.
    pub branch: Option<BranchRef>,
}

impl DirectAdded {
    /// The effects after the commit: the idea folder with idea.md, the
    /// copied ticket, plan.yaml and approval.json, then the branch of a new
    /// feature. Each write gives the same result when it runs again.
    pub fn write(&self, ideas: &Path, owner: User, git: &dyn IntegrationGitGateway) -> Result<()> {
        write_idea_file(ideas, owner.clone(), &self.idea.file)?;
        let folder = IdeaFolder::new(ideas, &self.idea.file.id, owner)?;
        let dir = folder.dir();
        fsx::ensure_private_dir(&dir.join(TICKETS))?;
        fsx::create_immutable(
            &dir.join(&self.ticket),
            self.ticket_text.as_bytes(),
            FILE_MODE,
        )?;
        fsx::write_atomic(&dir.join(PLAN_YAML), self.plan.as_bytes(), FILE_MODE)?;
        self.approval.write(&folder)?;
        if let Some(branch) = &self.branch {
            create_ref(git, branch)?;
        }
        Ok(())
    }
}

/// `AddDirectIdea`: the ticket `ticket` that you wrote becomes an idea in
/// `captured` in the feature of `target`, with its plan and your approval,
/// in `tx`. A ticket with no `# NN: Title` heading is refused, and writes
/// nothing. The caller commits, then runs [`DirectAdded::write`].
pub fn add_direct_idea(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    direct: &Direct<'_>,
    ticket: &str,
    target: Target<'_>,
    cause: Cause<'_>,
) -> Result<DirectAdded> {
    let facts = ticket_facts(ticket);
    let Some(title) = facts.title.clone().filter(|t| !t.trim().is_empty()) else {
        bail!("the ticket has no '# NN: Title' heading: fleetd names the feature and the task from it");
    };
    let idea = add_idea(tx, ctx, ticket, SOURCE, target, cause)?;
    let branch = match target {
        Target::New(_) => Some(name_branch(tx, ctx, direct, &idea.feature, &title, cause)?),
        Target::Join(_) => None,
    };
    let run = runs::of_feature(tx, &idea.feature)?
        .with_context(|| format!("the feature {} has no internal run", idea.feature))?;
    let name = name_from_title(&title);
    let rel = format!("{TICKETS}/01-{name}.md");
    let work = WorkTask {
        key: name.clone(),
        title,
        ticket: rel.clone(),
        kind: facts.kind.map(|k| k.as_str()).unwrap_or(""),
        profile: facts.primary.clone().unwrap_or_default(),
        check: facts.check.clone(),
        first_own: facts.own.first().cloned(),
    };
    let plan = plan_yaml(
        &idea,
        &run.project_root,
        run.run_branch.as_deref().unwrap_or(""),
        &work,
        direct.roster,
        &health::unhealthy_profiles(tx)?,
    )?;
    let sha256 = sha256_bytes(ticket.as_bytes()).hex();
    let approval = Approval {
        idea: idea.file.id.clone(),
        file: rel.clone(),
        sha256: sha256.clone(),
        approved_at: util::now(),
        by: BY_OPERATOR.to_string(),
    };
    let approval = record_approval(tx, ctx.guard, &approval, &sha256, cause)?;
    Ok(DirectAdded {
        idea,
        ticket: rel,
        ticket_text: ticket.to_string(),
        plan,
        approval,
        branch,
    })
}

/// The branch of the new feature `feature`, named from the ticket title.
fn name_branch(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    direct: &Direct<'_>,
    feature: &str,
    title: &str,
    cause: Cause<'_>,
) -> Result<BranchRef> {
    let named = name_feature_branch(
        tx,
        ctx,
        direct.git,
        feature,
        NameSource::Direct { title },
        direct.primary_tip,
        direct.clock,
        cause,
    )?;
    match named {
        Named::Accepted { effect, .. } => Ok(effect),
        Named::Refused { reason } => bail!("the feature of the ticket gets no branch: {reason}"),
    }
}

/// The work task of a `--direct` plan, from its ticket.
struct WorkTask<'a> {
    key: String,
    title: String,
    ticket: String,
    kind: &'a str,
    profile: String,
    check: Option<String>,
    /// The first `**Own:**` path of the ticket.
    first_own: Option<String>,
}

/// The text of plan.yaml: the work task, and its judge task when its kind
/// is a judgment kind. The plan validator reports each problem of it.
fn plan_yaml(
    idea: &Added,
    repo: &str,
    branch: &str,
    work: &WorkTask<'_>,
    roster: &[RosterProfile],
    unhealthy: &BTreeMap<String, String>,
) -> Result<String> {
    let acceptance: Vec<_> = work.check.iter().map(|c| json!({ "run": c })).collect();
    let mut tasks = vec![json!({
        "key": work.key,
        "title": work.title,
        "ticket": work.ticket,
        "kind": work.kind,
        "profile": work.profile,
        "covers": [REQUIREMENT],
        "acceptance": acceptance,
    })];
    if Kind::parse(work.kind).is_some_and(Kind::is_judgment) {
        tasks.push(json!({
            "key": format!("{}-judge", work.key),
            "title": format!("Judge: {}", work.title),
            "kind": Kind::Judge.as_str(),
            "judges": work.key,
            "profile": judge_profile(&work.profile, roster, unhealthy),
            "outputs": [verdict_file(work)],
        }));
    }
    let plan = json!({
        "idea": idea.file.id,
        "repo": repo,
        "feature": idea.feature,
        "base_branch": branch,
        "tasks": tasks,
    });
    Ok(serde_yaml::to_string(&plan)?)
}

/// The verdict file of the judge of `work`, its 1 output: the first Own
/// path of the ticket with the extension `review.md` (`docs/sync.md` gives
/// `docs/sync.review.md`, as in ticket 39 of fleetd-p6), else
/// `<task key>.review.md`. The design names no file name (MAP.md, Q18).
fn verdict_file(work: &WorkTask<'_>) -> String {
    match &work.first_own {
        Some(own) => Path::new(own)
            .with_extension("review.md")
            .to_string_lossy()
            .into_owned(),
        None => format!("{}.review.md", work.key),
    }
}

/// The profile of the judge of a task of the profile `author` (MAP.md, Q8):
/// the first healthy profile of the roster of another provider with a
/// sandbox on this OS, at the plan time. An empty name when the roster has
/// none, so the plan validator refuses the plan, and [`start_direct`] names
/// the cause.
fn judge_profile(
    author: &str,
    roster: &[RosterProfile],
    unhealthy: &BTreeMap<String, String>,
) -> String {
    judge_candidates(author, roster)
        .find(|p| !unhealthy.contains_key(&p.name))
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

/// The profiles of the roster that can judge a task of the profile
/// `author`: another provider, with a sandbox on this OS.
fn judge_candidates<'a>(
    author: &str,
    roster: &'a [RosterProfile],
) -> impl Iterator<Item = &'a RosterProfile> {
    let provider = roster
        .iter()
        .find(|p| p.name == author)
        .map(|p| p.provider.clone());
    roster.iter().filter(move |p| {
        provider
            .as_ref()
            .is_some_and(|provider| &p.provider != provider && p.sandbox_here)
    })
}

/// The problem of a judge task of the plan of `idea` that has no profile
/// because each profile that can judge is unhealthy: it names each of them
/// and its cause. `None` for each other plan.
fn judge_gap(conn: &Connection, ctx: &PlanCommit<'_>, idea: &str) -> Result<Option<Problem>> {
    let Ok(text) = std::fs::read_to_string(ctx.ideas.join(idea).join(PLAN_YAML)) else {
        return Ok(None);
    };
    let Ok(plan) = serde_yaml::from_str::<serde_json::Value>(&text) else {
        return Ok(None);
    };
    let tasks = plan["tasks"].as_array().cloned().unwrap_or_default();
    let field = |t: &serde_json::Value, k: &str| t[k].as_str().unwrap_or("").to_string();
    let Some(judge) = tasks
        .iter()
        .find(|t| field(t, "kind") == Kind::Judge.as_str() && field(t, "profile").is_empty())
    else {
        return Ok(None);
    };
    let judged = field(judge, "judges");
    let Some(author) = tasks.iter().find(|t| field(t, "key") == judged) else {
        return Ok(None);
    };
    let unhealthy = health::unhealthy_profiles(conn)?;
    let named: Vec<String> = judge_candidates(&field(author, "profile"), ctx.roster)
        .filter_map(|p| unhealthy.get(&p.name).map(|c| format!("{}: {c}", p.name)))
        .collect();
    if named.is_empty() {
        return Ok(None);
    }
    Ok(Some(Problem::task(
        "profile",
        &field(judge, "key"),
        format!(
            "no healthy profile of another provider with a sandbox on this OS can judge {judged}; unhealthy: {}",
            named.join("; ")
        ),
    )))
}

/// What [`start_direct`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// The plan is in the ledger, and the idea is in `in_progress`.
    Committed(Verdict),
    /// The plan is refused: the idea stays in `ticketing`, and 1 needs-you
    /// item gives the reason.
    Refused(Vec<Problem>),
    /// Another idea of the feature plans: the idea waits in `captured`.
    Waiting,
}

/// Move the `--direct` idea `idea` from `captured` to `ticketing` when no
/// other idea of its feature is in `clarifying`, `specifying` or
/// `ticketing`, then validate its plan. An idea already in `ticketing` is
/// validated again. An idea in another state is an error that writes
/// nothing.
pub fn start_direct(store: &mut Store, ctx: &PlanCommit<'_>, idea: &str) -> Result<Started> {
    let moved = store.immediate(|tx| {
        let row = ideas_store::idea(tx, idea)?
            .with_context(|| format!("the ledger has no idea {idea}"))?;
        if row.state == TICKETING {
            return Ok(true);
        }
        if row.state != FIRST {
            bail!(
                "idea {idea} is {}, not {FIRST}: it is not a new --direct idea",
                row.state
            );
        }
        if planning(tx, &row.feature_id, idea)? {
            return Ok(false);
        }
        let cause = by_fleetd("the ticket of a --direct idea is its plan");
        transition(
            tx,
            ctx.guard,
            Entity::Idea,
            idea,
            TICKETING,
            cause,
            ctx.refusals,
        )?;
        Ok(true)
    })?;
    if !moved {
        return Ok(Started::Waiting);
    }
    match validate_plan(store, ctx, idea)? {
        Verdict::Refused { mut problems } => {
            problems.extend(judge_gap(store.conn(), ctx, idea)?);
            store.immediate(|tx| open_refused(tx, idea, &problems))?;
            Ok(Started::Refused(problems))
        }
        committed => Ok(Started::Committed(committed)),
    }
}

/// Whether an idea of `feature` other than `idea` plans now.
fn planning(tx: &Connection, feature: &str, idea: &str) -> Result<bool> {
    let n: i64 = tx.query_row(
        "SELECT count(*) FROM ideas WHERE feature_id = ?1 AND id <> ?2 AND state IN (?3, ?4, ?5)",
        rusqlite::params![feature, idea, PLANNING[0], PLANNING[1], PLANNING[2]],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Open the needs-you item of the refused plan of `idea`, with each problem.
fn open_refused(tx: &Connection, idea: &str, problems: &[Problem]) -> Result<()> {
    let reasons: Vec<String> = problems
        .iter()
        .map(|p| match &p.task {
            Some(task) => format!("{} ({task}): {}", p.rule, p.message),
            None => format!("{}: {}", p.rule, p.message),
        })
        .collect();
    let text = format!(
        "The plan of the --direct idea {idea} is refused: {}",
        reasons.join("; ")
    );
    open_item(
        tx,
        &Write {
            key: &plan_refused_key(idea),
            kind: PLAN_REFUSED_KIND,
            entity: Entity::Idea.as_str(),
            entity_id: idea,
            text: &text,
        },
    )
}

/// The 1 requirement of the `--direct` idea `idea`, for the plan validator:
/// the approved ticket, which cites itself (MAP.md, Q6). `None` for an idea
/// of the normal flow: it has assumptions.md, or its approval.json that
/// fleetd wrote names no ticket of [`TICKETS`].
pub fn direct_requirement(
    conn: &Connection,
    guard: Option<Generation>,
    folder: &IdeaFolder,
    idea: &str,
) -> Result<Option<Requirement>> {
    match folder.read_assumptions() {
        Err(FolderError::Missing { .. }) => {}
        _ => return Ok(None),
    }
    let Some(approval) = trusted_approval(conn, guard, folder, idea)? else {
        return Ok(None);
    };
    if !approval.file.starts_with(&format!("{TICKETS}/")) {
        return Ok(None);
    }
    Ok(Some(Requirement {
        id: REQUIREMENT.to_string(),
        text: format!("The work of the ticket {}", approval.file),
        cites: vec![approval.file],
    }))
}

/// Whether `idea` is a `--direct` idea: its `added` event, which the
/// request commits with the idea row, has the source [`SOURCE`]. An idea
/// with no `added` event (a backfilled idea) is not one.
pub fn is_direct(conn: &Connection, idea: &str) -> Result<bool> {
    let Some(data) = fact_data(conn, Entity::Idea, idea, EventKind::Added, ADDED_CAUSE)? else {
        return Ok(false);
    };
    let v: serde_json::Value = serde_json::from_str(&data)
        .with_context(|| format!("the added event of {idea} is not JSON"))?;
    Ok(v["source"].as_str() == Some(SOURCE))
}

/// Whether the effects after the commit wrote the files of the `--direct`
/// idea in its folder `dir`: plan.yaml, and approval.json, which the
/// effects write after the ticket and plan.yaml.
pub fn files_written(dir: &Path) -> bool {
    dir.join(PLAN_YAML).exists() && dir.join(APPROVAL_JSON).exists()
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
