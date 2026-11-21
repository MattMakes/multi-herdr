use std::path::{Component, Path};

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::adapters::plan_yaml;
use crate::fleetd::ideas::application::direct::direct_requirement;
use crate::fleetd::ideas::domain::idea_folder::FILE_LIMIT;
use crate::fleetd::ideas::domain::plan::{Acceptance, Plan, PlannedTask};
use crate::fleetd::ideas::domain::plan_validator::{
    DeclaredGates, PlanFacts, PlanValidator, Problem, RosterProfile,
};
use crate::fleetd::task_execution::application::planned_tasks::{self, add_planned_tasks};
use crate::fsx;
use crate::taskledger::map::{ticket_facts, ticket_problems};
use crate::taskledger::model::{Actor, EventKind, WorkKind};
use crate::taskledger::{runs, util, Store};

pub const PLAN_YAML: &str = "plan.yaml";

pub const VERDICT_FILE: &str = "plan.verdict.json";

const TICKETING: &str = "ticketing";

const IN_PROGRESS: &str = "in_progress";

const READ: &str = "read";

const VERDICT_MODE: u32 = 0o600;

pub struct PlanCommit<'a> {
    pub ideas: &'a Path,
    pub owner: &'a User,
    pub roster: &'a [RosterProfile],
    pub gates: &'a DeclaredGates,
    pub guard: Option<Generation>,
    pub refusals: &'a dyn RecordRefusal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Committed {
        plan_sha256: String,
        tasks: Vec<(String, String)>,
    },
    Refused {
        problems: Vec<Problem>,
    },
}

struct ReadPlan {
    plan: Plan,
    sha256: String,
    tickets: Vec<Option<String>>,
}

pub fn validate_plan(store: &mut Store, ctx: &PlanCommit<'_>, idea_id: &str) -> Result<Verdict> {
    let idea = ideas_store::idea(store.conn(), idea_id)?
        .with_context(|| format!("the ledger has no idea {idea_id}"))?;
    if idea.state != TICKETING {
        bail!(
            "idea {idea_id} is {}, not {TICKETING}: it has no plan to validate",
            idea.state
        );
    }
    let folder = IdeaFolder::new(ctx.ideas, idea_id, ctx.owner.clone())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (read, mut problems) = read_plan(folder.dir(), ctx.owner, &idea);
    let facts = match plan_facts(store.conn(), &folder, ctx, idea_id) {
        Ok(facts) => facts,
        Err(p) => {
            problems.push(p);
            PlanFacts::default()
        }
    };
    let Some(read) = read.filter(|_| problems.is_empty()) else {
        return refuse(store, ctx, folder.dir(), idea_id, problems);
    };
    let problems = PlanValidator::validate(&read.plan, &facts);
    if !problems.is_empty() {
        return refuse(store, ctx, folder.dir(), idea_id, problems);
    }
    let tasks = store.immediate(|tx| commit(tx, ctx, &idea, &read, &facts, folder.dir()))?;
    let verdict = json!({
        "idea": idea_id,
        "valid": true,
        "plan_sha256": read.sha256,
        "problems": [],
        "tasks": tasks.iter().map(|(key, id)| json!({ "key": key, "task": id })).collect::<Vec<_>>(),
    });
    write_verdict(folder.dir(), &verdict)?;
    Ok(Verdict::Committed {
        plan_sha256: read.sha256,
        tasks,
    })
}

fn read_plan(dir: &Path, owner: &User, idea: &IdeaRow) -> (Option<ReadPlan>, Vec<Problem>) {
    let mut problems = Vec::new();
    let bytes = match read_file(dir, owner, PLAN_YAML) {
        Ok(bytes) => bytes,
        Err(reason) => {
            problems.push(Problem::plan(READ, format!("{PLAN_YAML}: {reason}")));
            return (None, problems);
        }
    };
    let sha256 = sha256_bytes(&bytes).hex();
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            problems.push(Problem::plan(
                READ,
                format!("{PLAN_YAML} is not UTF-8 text"),
            ));
            return (None, problems);
        }
    };
    let mut plan = match plan_yaml::parse(&text) {
        Ok(plan) => plan,
        Err(e) => {
            problems.push(Problem::plan(READ, format!("{PLAN_YAML}: {e:#}")));
            return (None, problems);
        }
    };
    if plan.idea != idea.id {
        problems.push(Problem::plan(
            READ,
            format!("{PLAN_YAML} names the idea {}, not {}", plan.idea, idea.id),
        ));
    }
    if plan.feature != idea.feature_id {
        problems.push(Problem::plan(
            READ,
            format!(
                "{PLAN_YAML} names the feature {}, not {}, the feature of {}",
                plan.feature, idea.feature_id, idea.id
            ),
        ));
    }
    let tickets = plan
        .tasks
        .iter_mut()
        .map(|task| {
            let ticket = task.ticket.as_mut()?;
            match read_ticket(dir, owner, &ticket.path) {
                Ok(text) => {
                    ticket.problems = ticket_problems(&text)
                        .into_iter()
                        .map(|(line, p)| format!("line {line}: {p}"))
                        .collect();
                    Some(text)
                }
                Err(reason) => {
                    ticket.problems = vec![reason];
                    None
                }
            }
        })
        .collect();
    let read = ReadPlan {
        plan,
        sha256,
        tickets,
    };
    (Some(read), problems)
}

pub(crate) fn read_ticket(dir: &Path, owner: &User, rel: &str) -> Result<String, String> {
    let inside = !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !inside {
        return Err("the path is not a relative path inside the idea folder".into());
    }
    let bytes = read_file(dir, owner, rel)?;
    String::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text".into())
}

pub(crate) fn read_file(dir: &Path, owner: &User, rel: &str) -> Result<Vec<u8>, String> {
    let path = dir.join(rel);
    if let Err(e) = std::fs::symlink_metadata(&path) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Err("the file does not exist".into());
        }
    }
    let rule = Rule {
        root: dir,
        owner,
        limit: FILE_LIMIT,
    };
    read_protected(&path, &rule).map_err(|r| r.reason)
}

pub(crate) fn plan_facts(
    conn: &Connection,
    folder: &IdeaFolder,
    ctx: &PlanCommit<'_>,
    idea_id: &str,
) -> Result<PlanFacts, Problem> {
    let read = |e: FolderError| Problem::plan(READ, e.to_string());
    let mut approved_assumptions = match folder.read_assumptions() {
        Ok(a) => a.ids(),
        Err(FolderError::Missing { .. }) => Vec::new(),
        Err(e) => return Err(read(e)),
    };
    let requirements = match folder.read_requirements() {
        Err(e @ FolderError::Missing { .. }) => {
            let direct = direct_requirement(conn, ctx.guard, folder, idea_id)
                .map_err(|e| Problem::plan(READ, format!("{e:#}")))?;
            let req = direct.ok_or_else(|| read(e))?;
            approved_assumptions.extend(req.cites.iter().cloned());
            vec![req]
        }
        other => other.map_err(read)?,
    };
    Ok(PlanFacts {
        requirements,
        approved_assumptions,
        answers: folder.read_answers().map_err(read)?,
        roster: ctx.roster.to_vec(),
        gates: ctx.gates.clone(),
    })
}

fn refuse(
    store: &mut Store,
    ctx: &PlanCommit<'_>,
    dir: &Path,
    idea_id: &str,
    problems: Vec<Problem>,
) -> Result<Verdict> {
    let listed: Vec<serde_json::Value> = problems
        .iter()
        .map(|p| json!({ "rule": p.rule, "task": p.task, "message": p.message }))
        .collect();
    store.immediate(|tx| {
        let changed = tx.execute(
            "UPDATE ideas SET phase_attempts = phase_attempts + 1, updated_at = ?2
             WHERE id = ?1 AND state = ?3",
            params![idea_id, util::now(), TICKETING],
        )?;
        if changed != 1 {
            bail!("idea {idea_id} left {TICKETING} before its plan was refused");
        }
        let text = format!("plan.yaml refused with {} problems", problems.len());
        let data = json!({ "decision": "refused", "reasons": listed });
        record(
            tx,
            ctx.guard,
            EventKind::Verdict,
            Entity::Idea,
            idea_id,
            by_fleetd(&text),
            &data,
        )?;
        Ok(())
    })?;
    let verdict = json!({
        "idea": idea_id,
        "valid": false,
        "plan_sha256": null,
        "problems": listed,
        "tasks": [],
    });
    write_verdict(dir, &verdict)?;
    Ok(Verdict::Refused { problems })
}

fn commit(
    tx: &Connection,
    ctx: &PlanCommit<'_>,
    idea: &IdeaRow,
    read: &ReadPlan,
    facts: &PlanFacts,
    dir: &Path,
) -> Result<Vec<(String, String)>> {
    let cause = by_fleetd("the plan is valid");
    let reqs: Vec<ideas_store::Requirement<'_>> = facts
        .requirements
        .iter()
        .map(|r| ideas_store::Requirement {
            req_id: &r.id,
            text: &r.text,
        })
        .collect();
    ideas_store::insert_requirements(tx, ctx.guard, &idea.id, &reqs, cause)?;
    let run = runs::of_feature(tx, &idea.feature_id)?.with_context(|| {
        format!(
            "the feature {} of {} has no internal run",
            idea.feature_id, idea.id
        )
    })?;
    let planned: Vec<planned_tasks::PlannedTask> = read
        .plan
        .tasks
        .iter()
        .zip(&read.tickets)
        .map(|(task, ticket)| planned_task(task, ticket.as_deref(), dir))
        .collect();
    let ids = add_planned_tasks(tx, ctx.guard, &run, &idea.id, &planned, cause)?;
    for (task, (_, id)) in read.plan.tasks.iter().zip(&ids) {
        for req in &task.covers {
            tx.execute(
                "INSERT INTO task_covers (task_id, idea_id, req_id) VALUES (?1, ?2, ?3)",
                params![id, idea.id, req],
            )?;
        }
    }
    tx.execute(
        "UPDATE ideas SET plan_sha256 = ?2, updated_at = ?3 WHERE id = ?1",
        params![idea.id, read.sha256, util::now()],
    )?;
    let task_ids: Vec<&str> = ids.iter().map(|(_, id)| id.as_str()).collect();
    let data = json!({ "decision": "committed", "plan_sha256": read.sha256, "task_ids": task_ids });
    record(
        tx,
        ctx.guard,
        EventKind::Verdict,
        Entity::Idea,
        &idea.id,
        cause,
        &data,
    )?;
    transition(
        tx,
        ctx.guard,
        Entity::Idea,
        &idea.id,
        IN_PROGRESS,
        cause,
        ctx.refusals,
    )?;
    Ok(ids)
}

pub(crate) fn planned_task(
    task: &PlannedTask,
    ticket: Option<&str>,
    dir: &Path,
) -> planned_tasks::PlannedTask {
    let task_file = match &task.ticket {
        Some(t) => dir.join(&t.path),
        None => dir.join(PLAN_YAML),
    };
    let runs: Vec<&str> = task
        .acceptance
        .iter()
        .filter_map(|a| match a {
            Acceptance::Run(cmd) => Some(cmd.as_str()),
            Acceptance::Sections { .. } => None,
        })
        .collect();
    planned_tasks::PlannedTask {
        key: task.key.clone(),
        title: task.title.clone(),
        task_file: task_file.to_string_lossy().into_owned(),
        ticket_snapshot: ticket.map(str::to_string),
        work_kind: WorkKind::parse(&task.kind_word).ok(),
        profile: task.profile.clone(),
        land_check: (!runs.is_empty()).then(|| runs.join(" && ")),
        spec_json: spec_json(task).to_string(),
        after: task.after.clone(),
        judges: task.judges.clone(),
        own: own_paths(task, ticket),
    }
}

fn own_paths(task: &PlannedTask, ticket: Option<&str>) -> Vec<String> {
    let mut own = task.outputs.clone();
    for path in ticket.map(|t| ticket_facts(t).own).unwrap_or_default() {
        if !own.contains(&path) {
            own.push(path);
        }
    }
    own
}

fn spec_json(task: &PlannedTask) -> serde_json::Value {
    let acceptance: Vec<serde_json::Value> = task
        .acceptance
        .iter()
        .map(|a| match a {
            Acceptance::Run(cmd) => json!({ "run": cmd }),
            Acceptance::Sections { file, sections } => {
                let mut items = vec![file.clone()];
                items.extend(sections.iter().cloned());
                json!({ "sections": items })
            }
        })
        .collect();
    json!({
        "key": task.key,
        "title": task.title,
        "ticket": task.ticket.as_ref().map(|t| &t.path),
        "kind": task.kind_word,
        "profile": task.profile,
        "covers": task.covers,
        "inputs": task.inputs,
        "outputs": task.outputs,
        "acceptance": acceptance,
        "after": task.after,
        "judges": task.judges,
        "budget": task.budget.as_ref().map(|b| json!({ "max_minutes": b.max_minutes })),
        "reason": task.reason,
        "no_decisions": task.no_decisions,
    })
}

fn write_verdict(dir: &Path, verdict: &serde_json::Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(verdict)?;
    text.push('\n');
    fsx::write_atomic(&dir.join(VERDICT_FILE), text.as_bytes(), VERDICT_MODE)
        .with_context(|| format!("writing {VERDICT_FILE} of {}", dir.display()))
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
