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
use crate::fleetd::ideas::domain::plan::{Kind, Plan, PlannedTask};
use crate::fleetd::ideas::domain::plan_validator::{
    PlanFacts, Problem, Requirement, RosterProfile,
};
use crate::fleetd::ideas::domain::{plan_rules_model_guide, plan_rules_profiles};
use crate::fleetd::integration::application::new_features::{
    create_ref, name_feature_branch, name_from_title, BranchRef, NameSource, Named,
};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::worker_profiles::application::health;
use crate::fsx;
use crate::roster::teammate::reserved_tier;
use crate::taskledger::map::ticket_facts;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::{runs, util, Store};

pub const SOURCE: &str = "horch idea add --direct";

pub const TICKETS: &str = "tickets";

pub const REQUIREMENT: &str = "R1";

pub const PLAN_REFUSED_KIND: &str = "direct_plan_refused";

const TICKETING: &str = "ticketing";

const PLANNING: [&str; 3] = ["clarifying", "specifying", "ticketing"];

const FILE_MODE: u32 = 0o600;

pub fn plan_refused_key(idea: &str) -> String {
    format!("{PLAN_REFUSED_KIND}:{idea}")
}

pub struct Direct<'a> {
    pub roster: &'a [RosterProfile],
    pub git: &'a dyn IntegrationGitGateway,
    pub primary_tip: &'a str,
    pub clock: FleetSeconds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectAdded {
    pub idea: Added,
    pub ticket: String,
    pub ticket_text: String,
    pub plan: String,
    pub approval: ApprovalFile,
    pub branch: Option<BranchRef>,
}

impl DirectAdded {
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
        write_judge_ticket(dir)?;
        self.approval.write(&folder)?;
        if let Some(branch) = &self.branch {
            create_ref(git, branch)?;
        }
        Ok(())
    }
}

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

struct WorkTask<'a> {
    key: String,
    title: String,
    ticket: String,
    kind: &'a str,
    profile: String,
    check: Option<String>,
    first_own: Option<String>,
}

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
            "ticket": judge_ticket_rel(&work.key),
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

fn judge_ticket_rel(key: &str) -> String {
    format!("{TICKETS}/02-{key}-judge.md")
}

pub fn write_judge_ticket(dir: &Path) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(dir.join(PLAN_YAML)) else {
        return Ok(());
    };
    let plan: serde_json::Value = serde_yaml::from_str(&text)?;
    let tasks = plan["tasks"].as_array().cloned().unwrap_or_default();
    let field = |t: &serde_json::Value, k: &str| t[k].as_str().unwrap_or("").to_string();
    let Some(judge) = tasks
        .iter()
        .find(|t| field(t, "kind") == Kind::Judge.as_str())
    else {
        return Ok(());
    };
    let rel = judge_ticket_rel(&field(judge, "judges"));
    if field(judge, "ticket") != rel {
        return Ok(());
    }
    let verdict = judge["outputs"][0].as_str().unwrap_or("");
    let ticket = judge_ticket(
        &field(judge, "title"),
        &field(judge, "judges"),
        &field(judge, "profile"),
        verdict,
    );
    fsx::ensure_private_dir(&dir.join(TICKETS))?;
    fsx::create_immutable(&dir.join(rel), ticket.as_bytes(), FILE_MODE)?;
    Ok(())
}

fn judge_ticket(title: &str, judged: &str, profile: &str, verdict: &str) -> String {
    format!(
        "# 02: {title}

<!-- horch-ticket: 1 -->
**Persona:** primary `{profile}`
**Kind:** report
**Check:** none
**Own:** `{verdict}`

## Goal

Judge the output of the task `{judged}` against its ticket. Write the verdict to `{verdict}`.

## Steps

1. Read the ticket of the task `{judged}` and its output.
2. Check the output against each requirement of that ticket. Do not change the output.
3. Write `{verdict}` in the verdict format below.

## Done when

`{verdict}` exists and has these 2 sections:

- `## Verdict`: exactly 1 word, `pass` or `critical_open`. Write `critical_open` when the list of findings has 1 or more `[CRITICAL]` items, else `pass`.
- `## Findings`: 1 list item for each finding. Each item starts with its rank: `[CRITICAL]`, `[HIGH]`, `[MEDIUM]` or `[LOW]`. With no finding, the section has no list item.
"
    )
}

fn verdict_file(work: &WorkTask<'_>) -> String {
    match &work.first_own {
        Some(own) => Path::new(own)
            .with_extension("review.md")
            .to_string_lossy()
            .into_owned(),
        None => format!("{}.review.md", work.key),
    }
}

fn judge_profile(
    author: &str,
    roster: &[RosterProfile],
    unhealthy: &BTreeMap<String, String>,
) -> String {
    judge_candidates(author, roster)
        .find(|p| cannot_judge(p, roster, unhealthy).is_none())
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

fn cannot_judge(
    p: &RosterProfile,
    roster: &[RosterProfile],
    unhealthy: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(cause) = unhealthy.get(&p.name) {
        return Some(format!("unhealthy: {cause}"));
    }
    if let Some((tier, _)) = reserved_tier(&p.model) {
        return Some(format!(
            "its model {} is of the reserved tier {tier}",
            p.model
        ));
    }
    let plan = Plan {
        tasks: vec![PlannedTask {
            key: "judge".into(),
            kind: Some(Kind::Judge),
            profile: p.name.clone(),
            ..PlannedTask::default()
        }],
        ..Plan::default()
    };
    let facts = PlanFacts {
        roster: roster.to_vec(),
        ..PlanFacts::default()
    };
    let refused: Vec<String> = plan_rules_profiles::check(&plan, &facts)
        .into_iter()
        .chain(plan_rules_model_guide::check(&plan, &facts))
        .map(|problem| problem.message)
        .collect();
    (!refused.is_empty()).then(|| refused.join("; "))
}

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
        .filter_map(|p| cannot_judge(p, ctx.roster, &unhealthy).map(|c| format!("{}: {c}", p.name)))
        .collect();
    if named.is_empty() {
        return Ok(None);
    }
    Ok(Some(Problem::task(
        "profile",
        &field(judge, "key"),
        format!(
            "no profile of another provider with a sandbox on this OS can judge {judged}: {}",
            named.join("; ")
        ),
    )))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    Committed(Verdict),
    Refused(Vec<Problem>),
    Waiting,
}

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
    let folder = IdeaFolder::new(ctx.ideas, idea, ctx.owner.clone())?;
    write_judge_ticket(folder.dir())?;
    match validate_plan(store, ctx, idea)? {
        Verdict::Refused { mut problems } => {
            problems.extend(judge_gap(store.conn(), ctx, idea)?);
            store.immediate(|tx| open_refused(tx, idea, &problems))?;
            Ok(Started::Refused(problems))
        }
        committed => Ok(Started::Committed(committed)),
    }
}

fn planning(tx: &Connection, feature: &str, idea: &str) -> Result<bool> {
    let n: i64 = tx.query_row(
        "SELECT count(*) FROM ideas WHERE feature_id = ?1 AND id <> ?2 AND state IN (?3, ?4, ?5)",
        rusqlite::params![feature, idea, PLANNING[0], PLANNING[1], PLANNING[2]],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

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

pub fn is_direct(conn: &Connection, idea: &str) -> Result<bool> {
    let Some(data) = fact_data(conn, Entity::Idea, idea, EventKind::Added, ADDED_CAUSE)? else {
        return Ok(false);
    };
    let v: serde_json::Value = serde_json::from_str(&data)
        .with_context(|| format!("the added event of {idea} is not JSON"))?;
    Ok(v["source"].as_str() == Some(SOURCE))
}

pub fn files_written(dir: &Path) -> bool {
    dir.join(PLAN_YAML).exists() && dir.join(APPROVAL_JSON).exists()
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
