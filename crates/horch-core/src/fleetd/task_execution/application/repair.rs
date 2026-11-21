use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey};
use crate::fleetd::fleet_runtime::application::ledger::{created, record, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::integration::adapters::exec_bundle::Build;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::bundles::failed_gate;
use crate::fleetd::integration::application::landing;
use crate::fleetd::integration::domain::bundle::BundleState;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::adapters::ingested_store::ACCEPTED;
use crate::fleetd::task_execution::adapters::result_reader::{AttemptRef, RESULT_JSON};
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::application::dispatch::{Bound, BoundTree, DispatchCtx};
use crate::fleetd::task_execution::application::lease;
use crate::fleetd::task_execution::application::verifier::check_hash;
use crate::fleetd::task_execution::domain::repair::{
    next_repair, NextRepair, RepairCount, REPAIR_LIMIT_KIND,
};
use crate::fleetd::task_execution::domain::task::{attempt_id, AttemptRow, AttemptState};
use crate::fleetd::task_execution::domain::verification::input_key;
use crate::fleetd::worker_profiles::domain::profile::Retry;
use crate::fleetd::worktree_custody::adapters::sqlite as trees;
use crate::fleetd::worktree_custody::application::create::{create_worktree, CreateOutcome, Lease};
use crate::taskledger::finish;
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerKind};
use crate::taskledger::sql_words::WORKTREES_GONE;
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::workers::{self, NewWorker};
use crate::taskledger::{runs, util};

const REPAIR_KEY: &str = "repair";

const REPAIR_CAUSE: &str = "a resolver or fix attempt of a task branch";

const FAILED_CAUSE: &str = "a resolver or fix attempt failed";

const FAILED_AT_KEY: &str = "failed_at";

const OUTPUT_TAIL_LINES: usize = 60;

const OUTPUT_TAIL_BYTES: usize = 8 * 1024;

pub const REPAIR_START_KIND: &str = "repair_start";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairKind {
    Resolver,
    Fix,
}

impl RepairKind {
    pub fn of(state: LandingState) -> Option<RepairKind> {
        match state {
            LandingState::Conflicted => Some(RepairKind::Resolver),
            LandingState::GateFailed => Some(RepairKind::Fix),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RepairKind::Resolver => "resolver",
            RepairKind::Fix => "fix",
        }
    }

    fn parse(word: &str) -> Option<RepairKind> {
        [RepairKind::Resolver, RepairKind::Fix]
            .into_iter()
            .find(|k| k.as_str() == word)
    }

    fn state(self) -> LandingState {
        match self {
            RepairKind::Resolver => LandingState::Conflicted,
            RepairKind::Fix => LandingState::GateFailed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub branch: String,
    pub kind: RepairKind,
    pub bound: Bound,
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

struct Repairing {
    branch: String,
    task_id: String,
    kind: RepairKind,
}

fn repairing(conn: &Connection) -> Result<Vec<Repairing>> {
    let mut out = Vec::new();
    for kind in [RepairKind::Resolver, RepairKind::Fix] {
        for (branch, task_id) in rows::task_branches_in(conn, kind.state().as_str())? {
            if tasks::get(conn, &task_id)?.status == TaskStatus::Done.as_str() {
                out.push(Repairing {
                    branch,
                    task_id,
                    kind,
                });
            }
        }
    }
    out.sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(out)
}

pub fn has_branch_to_repair(conn: &Connection, task_id: &str) -> Result<bool> {
    let states = rows::landing_states_of_task(conn, task_id)?;
    Ok(states
        .iter()
        .flatten()
        .any(|s| LandingState::parse(s).and_then(RepairKind::of).is_some()))
}

fn has_open_attempt(conn: &Connection, task_id: &str) -> Result<bool> {
    let open = conn
        .query_row(
            "SELECT 1 FROM attempts WHERE task_id = ?1 AND state <> ?2 LIMIT 1",
            params![task_id, AttemptState::Ended.as_str()],
            |_| Ok(()),
        )
        .optional()?;
    Ok(open.is_some())
}

#[derive(Debug, PartialEq, Eq)]
struct RepairFact {
    kind: RepairKind,
    branch: String,
    entered: Option<String>,
}

fn entered(conn: &Connection, branch: &str, kind: RepairKind) -> Result<Option<String>> {
    door::entered_at(conn, Entity::TaskBranch, branch, &[kind.state().as_str()])
}

fn newest_repair(conn: &Connection, r: &Repairing) -> Result<Option<String>> {
    let newest: Option<String> = conn
        .query_row(
            "SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [&r.task_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(id) = newest else {
        return Ok(None);
    };
    let Some(fact) = repair_fact(conn, &id)? else {
        return Ok(None);
    };
    let current = fact.kind == r.kind
        && fact.branch == r.branch
        && fact.entered == entered(conn, &r.branch, r.kind)?;
    Ok(current.then_some(id))
}

fn repair_fact(conn: &Connection, attempt_id: &str) -> Result<Option<RepairFact>> {
    let data = door::fact_data(
        conn,
        Entity::Attempt,
        attempt_id,
        EventKind::Note,
        REPAIR_CAUSE,
    )?;
    data.as_deref().map_or(Ok(None), parse_fact)
}

fn parse_fact(data: &str) -> Result<Option<RepairFact>> {
    let v: serde_json::Value = serde_json::from_str(data)?;
    let kind = v[REPAIR_KEY].as_str().and_then(RepairKind::parse);
    let branch = v["branch"].as_str();
    Ok(kind.zip(branch).map(|(kind, branch)| RepairFact {
        kind,
        branch: branch.to_string(),
        entered: v["entered"].as_str().map(str::to_string),
    }))
}

pub fn repair_kind(conn: &Connection, attempt_id: &str) -> Result<Option<RepairKind>> {
    Ok(repair_fact(conn, attempt_id)?.map(|f| f.kind))
}

pub fn start_due(tx: &Connection, ctx: &DispatchCtx<'_>, budget: usize) -> Result<Vec<Started>> {
    if tx.is_autocommit() {
        bail!("the repair start writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Vec::new();
    let mut at_limit = BTreeSet::new();
    let mut waiting = BTreeSet::new();
    for r in repairing(tx)? {
        waiting.insert(start_key(&r.branch));
        let failed_at = match newest_repair(tx, &r)? {
            Some(attempt) => match failed_at(tx, &attempt)? {
                Some(at) => Some(at),
                None => continue,
            },
            None => None,
        };
        let count = RepairCount {
            failed: repair_attempts(tx, &r.branch)?,
            max_attempts: runs::get(tx, &tasks::get(tx, &r.task_id)?.run_id)?.max_attempts,
            failed_at,
        };
        match next_repair(&count, &Retry::default().backoff_s, ctx.clock.0) {
            NextRepair::Wait { .. } => continue,
            NextRepair::Limit => {
                open_limit_item(tx, &r, &count)?;
                at_limit.insert(limit_key(&r.branch));
                continue;
            }
            NextRepair::Start => {}
        }
        if out.len() >= budget || has_open_attempt(tx, &r.task_id)? {
            continue;
        }
        tx.execute_batch("SAVEPOINT repair_bind")?;
        match bind(tx, ctx, &r) {
            Ok(s) => {
                tx.execute_batch("RELEASE repair_bind")?;
                waiting.remove(&start_key(&r.branch));
                out.push(s);
            }
            Err(e) => {
                tx.execute_batch("ROLLBACK TO repair_bind; RELEASE repair_bind")?;
                open_start_item(tx, &r, &e)?;
            }
        }
    }
    for key in needs_you::open_keys(tx, REPAIR_LIMIT_KIND)? {
        if !at_limit.contains(&key) {
            needs_you::resolve(tx, &key)?;
        }
    }
    for key in needs_you::open_keys(tx, REPAIR_START_KIND)? {
        if !waiting.contains(&key) {
            needs_you::resolve(tx, &key)?;
        }
    }
    Ok(out)
}

fn repair_attempts(conn: &Connection, branch: &str) -> Result<i64> {
    Ok(rows::branch(conn, branch)?.map_or(0, |b| b.repair_attempts))
}

fn start_key(branch: &str) -> String {
    format!("{REPAIR_START_KIND}:{branch}")
}

fn open_start_item(tx: &Connection, r: &Repairing, e: &anyhow::Error) -> Result<()> {
    let text = format!(
        "The {} attempt of the task branch {} cannot start: {e:#}. fleetd tries again on \
         each pass. Run `horch branch abandon {}` to end the branch.",
        r.kind.as_str(),
        r.branch,
        r.branch
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &start_key(&r.branch),
            kind: REPAIR_START_KIND,
            entity: Entity::TaskBranch.as_str(),
            entity_id: &r.branch,
            text: &text,
        },
    )
}

fn limit_key(branch: &str) -> String {
    format!("{REPAIR_LIMIT_KIND}:{branch}")
}

fn open_limit_item(tx: &Connection, r: &Repairing, count: &RepairCount) -> Result<()> {
    let task = tasks::get(tx, &r.task_id)?;
    let text = format!(
        "The task branch {} of {} is {}. {} of {} {} attempts failed, and {} is the \
         max_attempts of the task. Run `horch task requeue {}` for 1 more attempt, or run \
         `horch branch abandon {}`.",
        r.branch,
        task.short(),
        r.kind.state().as_str(),
        count.failed,
        count.max_attempts,
        r.kind.as_str(),
        count.max_attempts,
        task.short(),
        r.branch
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &limit_key(&r.branch),
            kind: REPAIR_LIMIT_KIND,
            entity: Entity::TaskBranch.as_str(),
            entity_id: &r.branch,
            text: &text,
        },
    )
}

fn failed_at(conn: &Connection, attempt_id: &str) -> Result<Option<i64>> {
    let data = door::fact_data(
        conn,
        Entity::Attempt,
        attempt_id,
        EventKind::Note,
        FAILED_CAUSE,
    )?;
    let Some(data) = data else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_str(&data)?;
    v[FAILED_AT_KEY]
        .as_i64()
        .map(Some)
        .with_context(|| format!("the failure event of {attempt_id} has no {FAILED_AT_KEY}"))
}

fn count_failure(
    tx: &Connection,
    guard: Option<Generation>,
    clock: FleetSeconds,
    b: &Repairing,
    attempt: &str,
) -> Result<()> {
    if failed_at(tx, attempt)?.is_some() {
        return Ok(());
    }
    let n = repair_attempts(tx, &b.branch)? + 1;
    let text = format!("the {} attempt {attempt} failed", b.kind.as_str());
    rows::set_branch_fact(
        tx,
        guard,
        &b.branch,
        BranchFact::RepairAttempts(n),
        by_fleetd(&text),
    )?;
    let data = json!({ FAILED_AT_KEY: clock.0, "repair_attempts": n, "branch": b.branch });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Attempt,
        attempt,
        by_fleetd(FAILED_CAUSE),
        &data,
    )?;
    Ok(())
}

fn attempt_ended(conn: &Connection, attempt_id: &str) -> Result<bool> {
    let state: String = conn.query_row(
        "SELECT state FROM attempts WHERE id = ?1",
        [attempt_id],
        |r| r.get(0),
    )?;
    Ok(state == AttemptState::Ended.as_str())
}

pub fn requeue(
    tx: &Connection,
    guard: Option<Generation>,
    task: &Task,
    reason: &str,
) -> Result<Task> {
    let mine: Vec<Repairing> = repairing(tx)?
        .into_iter()
        .filter(|r| r.task_id == task.task_id)
        .collect();
    if mine.is_empty() {
        bail!(
            "{} is done, and no branch of it waits for a resolver or fix attempt; only a held \
             task is requeued",
            task.short()
        );
    }
    let text = format!("horch task requeue: {reason}");
    for r in &mine {
        let cause = Cause {
            actor: Actor::Operator,
            text: &text,
        };
        rows::set_branch_fact(tx, guard, &r.branch, BranchFact::RepairAttempts(0), cause)?;
    }
    tasks::get(tx, &task.task_id)
}

fn persona_of(t: &Task) -> String {
    t.claimable_by
        .clone()
        .or(t.primary_persona.clone())
        .unwrap_or_default()
}

fn bind(tx: &Connection, ctx: &DispatchCtx<'_>, r: &Repairing) -> Result<Started> {
    let guard = ctx.jobs.guard;
    let task = tasks::get(tx, &r.task_id)?;
    let run = runs::get(tx, &task.run_id)?;
    let n = task.attempts + 1;
    let attempt = attempt_id(&task.task_id, n);
    let persona = persona_of(&task);
    let worker_id = workers::insert(
        tx,
        &NewWorker {
            run_id: task.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: persona.clone(),
            target_task_id: Some(task.task_id.clone()),
            workspace_id: run.workspace_id.clone(),
            ..NewWorker::default()
        },
    )?;
    tx.execute(
        "UPDATE workers SET task_id = ?2, attempt = ?3 WHERE worker_id = ?1",
        params![worker_id, task.task_id, n],
    )?;
    let moved = tx.execute(
        "UPDATE tasks SET attempts = ?2, attempt_floor = attempt_floor + 1,
            current_worker_id = ?3, updated_at = ?4
         WHERE task_id = ?1 AND status = ?5",
        params![
            task.task_id,
            n,
            worker_id,
            util::now(),
            TaskStatus::Done.as_str()
        ],
    )?;
    if moved != 1 {
        bail!("{} is {}, not done", task.short(), task.status);
    }
    let row = AttemptRow {
        id: attempt.clone(),
        task_id: Some(task.task_id.clone()),
        worker_id: worker_id.clone(),
        state: AttemptState::Launching,
        process: None,
        pane: None,
    };
    store::insert_attempt(tx, &row, &util::now())?;
    tx.execute("UPDATE attempts SET via = 'push' WHERE id = ?1", [&attempt])?;
    let text = format!(
        "fleetd binds the {} attempt {attempt} of {}: its branch {} is {}",
        r.kind.as_str(),
        task.short(),
        r.branch,
        r.kind.state().as_str()
    );
    created(tx, guard, Entity::Attempt, &attempt, by_fleetd(&text))?;
    let data = json!({
        REPAIR_KEY: r.kind.as_str(),
        "branch": r.branch,
        "entered": entered(tx, &r.branch, r.kind)?,
    });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Attempt,
        &attempt,
        by_fleetd(REPAIR_CAUSE),
        &data,
    )?;
    let tree = take_tree(tx, ctx, &task, &worker_id, &attempt)?;
    lease::take(
        tx,
        ctx.clock,
        &task.task_id,
        &attempt,
        lease::claim_lease_s(),
    )?;
    Ok(Started {
        branch: r.branch.clone(),
        kind: r.kind,
        bound: Bound {
            task_id: task.task_id,
            attempt_id: attempt,
            attempt: n,
            worker_id,
            persona,
            worktree: Some(tree),
        },
    })
}

fn take_tree(
    tx: &Connection,
    ctx: &DispatchCtx<'_>,
    task: &Task,
    worker_id: &str,
    attempt: &str,
) -> Result<BoundTree> {
    let row = trees::of_task(tx, &task.task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status));
    if row.is_none() {
        bail!(
            "{} has no worktree: its branch is repaired in its own tree",
            task.short()
        );
    }
    let lease = Lease {
        task_id: Some(&task.task_id),
        worker_id,
        attempt: Some(attempt),
        from: None,
        new: None,
    };
    let (row, add_job) = match create_worktree(&ctx.jobs, tx, &lease, ctx.clock)? {
        CreateOutcome::Attached(row) => (row, None),
        CreateOutcome::Reserved { row, job } => (row, Some(job)),
        CreateOutcome::NeedsRescue(row) => bail!(
            "the worktree {} of {} is orphaned: its rescue comes first",
            row.worktree_id,
            task.short()
        ),
    };
    Ok(BoundTree {
        worktree_id: row.worktree_id,
        path: row.path,
        branch: row.branch,
        add_job,
    })
}

pub fn start_section(
    conn: &Connection,
    fleet_root: &Path,
    attempt_id: &str,
) -> Result<Option<String>> {
    match repair_fact(conn, attempt_id)? {
        Some(fact) => Ok(Some(section_of(conn, fleet_root, attempt_id, fact)?)),
        None => Ok(None),
    }
}

pub fn start_sections<'a>(
    conn: &Connection,
    fleet_root: &Path,
    attempt_ids: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let data = door::fact_data_of_ids(
        conn,
        Entity::Attempt,
        attempt_ids,
        EventKind::Note,
        REPAIR_CAUSE,
    )?;
    let mut out = std::collections::BTreeMap::new();
    for (attempt_id, data) in data {
        if let Some(fact) = parse_fact(&data)? {
            let section = section_of(conn, fleet_root, &attempt_id, fact)?;
            out.insert(attempt_id, section);
        }
    }
    Ok(out)
}

fn section_of(
    conn: &Connection,
    fleet_root: &Path,
    attempt_id: &str,
    RepairFact { kind, branch, .. }: RepairFact,
) -> Result<String> {
    let task_id: String = conn
        .query_row(
            "SELECT task_id FROM attempts WHERE id = ?1",
            [attempt_id],
            |r| r.get(0),
        )
        .with_context(|| format!("no attempt {attempt_id}"))?;
    let task = tasks::get(conn, &task_id)?;
    let check = check_of(conn, &task)?;
    let check = check.map_or_else(
        || "the task has no check command".to_string(),
        |c| format!("`{c}`"),
    );
    let feature = match rows::branch(conn, &branch)? {
        Some(row) => rows::feature(conn, &row.feature_id)?.and_then(|f| f.branch),
        None => None,
    };
    let feature = feature.with_context(|| format!("the feature of {branch} has no branch"))?;
    let section = match kind {
        RepairKind::Resolver => {
            let files = conflict_files(conn, &branch)?;
            resolver_section(&branch, &feature, &files, &check)
        }
        RepairKind::Fix => {
            let output = gate_output(conn, fleet_root, &branch)?;
            fix_section(&branch, &output, &check)
        }
    };
    Ok(section)
}

fn check_of(conn: &Connection, task: &Task) -> Result<Option<String>> {
    Ok(task
        .land_check
        .clone()
        .or(finish::default_check(conn, &task.run_id)?)
        .filter(|c| !c.trim().is_empty()))
}

fn conflict_files(conn: &Connection, branch: &str) -> Result<Vec<String>> {
    for (_, id) in rows::bundle_jobs_of_branch(conn, branch, BundleState::Dropped.as_str())? {
        let Some(job) = jobs_store::get(conn, id)? else {
            continue;
        };
        let build = jobs::result_data(&job).and_then(|d| serde_json::from_value::<Build>(d).ok());
        if let Some(Build::Conflict { member, files }) = build {
            if member == branch {
                return Ok(files);
            }
        }
    }
    Ok(Vec::new())
}

struct GateOutput {
    text: String,
    path: String,
    tail: Option<String>,
}

fn gate_output(conn: &Connection, fleet_root: &Path, branch: &str) -> Result<GateOutput> {
    let Some(gate) = failed_gate(conn, branch)? else {
        return Ok(GateOutput {
            text: "no failed gate of the branch is in the ledger".into(),
            path: String::new(),
            tail: None,
        });
    };
    let job = jobs_store::get(conn, gate.job_id)?;
    let text = job
        .as_ref()
        .and_then(jobs::outcome)
        .map_or_else(String::new, |o| o.text);
    let path = fleet_root
        .join("gate-output")
        .join(format!("{}.log", gate.job_id));
    let tail = std::fs::read(&path).ok().map(|bytes| tail_of(&bytes));
    Ok(GateOutput {
        text,
        path: path.display().to_string(),
        tail,
    })
}

fn tail_of(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(OUTPUT_TAIL_BYTES);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(OUTPUT_TAIL_LINES);
    lines[from..].join("\n")
}

fn resolver_section(branch: &str, feature: &str, files: &[String], check: &str) -> String {
    let files = if files.is_empty() {
        "- The build of the bundle named no file. `git merge` names them.\n".to_string()
    } else {
        files.iter().map(|f| format!("- `{f}`\n")).collect()
    };
    format!(
        "## Resolver attempt\n\n\
         Your task is done. Its branch `{branch}` conflicts with the feature branch \
         `{feature}`. This attempt resolves the conflict on the task branch.\n\n\
         The conflict files:\n\n\
         {files}\n\
         1. In your worktree, run `git merge {feature}`. The repository sets \
         `merge.conflictStyle=zdiff3`, so each conflict shows in zdiff3 form: \
         your side, the base, and the feature side.\n\
         2. Resolve each conflict. Keep the requirements of both sides.\n\
         3. Run the tests of both sides: the check of this task, {check}, and the \
         fast gate of the feature in `fleet.toml`.\n\
         4. Commit the merge, then run `horch result`.\n"
    )
}

fn fix_section(branch: &str, output: &GateOutput, check: &str) -> String {
    let mut out = format!(
        "## Fix attempt\n\n\
         Your task is done. The fast gate of the feature failed on its branch \
         `{branch}`. This attempt fixes the failure on the task branch.\n\n\
         The gate: {}\n",
        output.text
    );
    if !output.path.is_empty() {
        out.push_str(&format!("\nThe gate output is in `{}`.\n", output.path));
    }
    if let Some(tail) = &output.tail {
        out.push_str(&format!(
            "\nThe end of the gate output:\n\n~~~text\n{tail}\n~~~\n"
        ));
    }
    out.push_str(&format!(
        "\n1. Find the cause of the failure in the gate output.\n\
         2. Fix it in your worktree.\n\
         3. Run the check of this task, {check}, and the fast gate of the feature in \
         `fleet.toml`.\n\
         4. Commit the fix, then run `horch result`.\n"
    ));
    out
}

pub struct Results<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub clock: FleetSeconds,
    pub tips: &'a BTreeMap<String, String>,
    pub fleet_root: &'a Path,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub queued: Vec<String>,
    pub ready: Vec<String>,
    pub failed: Vec<String>,
}

enum Checked {
    Waits,
    Passed,
    Failed,
}

pub fn observe(r: &Results<'_>, tx: &Connection) -> Result<Report> {
    if tx.is_autocommit() {
        bail!("the repair result writes its rows in 1 open transaction of the ledger");
    }
    let mut report = Report::default();
    for b in repairing(tx)? {
        let Some(attempt) = newest_repair(tx, &b)? else {
            continue;
        };
        if !has_result(tx, r.fleet_root, &b.task_id, &attempt)? {
            if attempt_ended(tx, &attempt)? {
                count_failure(tx, r.jobs.guard, r.clock, &b, &attempt)?;
                report.failed.push(b.branch);
            }
            continue;
        }
        let Some(tip) = r.tips.get(&b.task_id) else {
            continue;
        };
        match check(r, tx, &b.task_id, tip)? {
            Checked::Waits => report.queued.push(b.branch),
            Checked::Failed => {
                count_failure(tx, r.jobs.guard, r.clock, &b, &attempt)?;
                report.failed.push(b.branch);
            }
            Checked::Passed => {
                ready(r, tx, &b, &attempt)?;
                report.ready.push(b.branch);
            }
        }
    }
    Ok(report)
}

fn has_result(conn: &Connection, fleet_root: &Path, task_id: &str, attempt: &str) -> Result<bool> {
    let a = AttemptRef {
        task_id: task_id.to_string(),
        attempt_id: attempt.to_string(),
    };
    let path = a.outbox(fleet_root).join(RESULT_JSON);
    let found = conn
        .query_row(
            "SELECT 1 FROM ingested WHERE path = ?1 AND outcome = ?2",
            params![path.display().to_string(), ACCEPTED],
            |_| Ok(()),
        )
        .optional()?;
    Ok(found.is_some())
}

fn check(r: &Results<'_>, tx: &Connection, task_id: &str, tip: &str) -> Result<Checked> {
    let task = tasks::get(tx, task_id)?;
    let Some(check) = check_of(tx, &task)? else {
        return Ok(if task.check_none {
            Checked::Passed
        } else {
            Checked::Failed
        });
    };
    let key = input_key(tip, &check_hash(Some(&check)));
    let last = jobs_store::latest(tx, Kind::TaskCheck, task_id, &key)?;
    match last.as_ref().map(|j: &JobRow| j.state) {
        Some(State::Passed) => return Ok(Checked::Passed),
        Some(State::Failed) => return Ok(Checked::Failed),
        Some(State::Queued | State::Running) => return Ok(Checked::Waits),
        Some(State::Abandoned) | None => {}
    }
    let project = runs::get(tx, &task.run_id)?.project_root;
    let job = JobKey {
        kind: Kind::TaskCheck,
        subject: task_id,
        input_key: &key,
    };
    jobs::queue(r.jobs, tx, &job, Some(&project), r.clock)?;
    Ok(Checked::Waits)
}

fn ready(r: &Results<'_>, tx: &Connection, b: &Repairing, attempt: &str) -> Result<()> {
    let why = format!(
        "the check of the result of the {} attempt {attempt} passed",
        b.kind.as_str()
    );
    let path: &[LandingState] = match b.kind {
        RepairKind::Resolver => &[LandingState::Ready],
        RepairKind::Fix => &[LandingState::Verifying, LandingState::Ready],
    };
    for to in path {
        landing::enter(r.jobs, tx, &b.branch, *to, r.clock, &why)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_repair_state_has_its_kind() {
        assert_eq!(
            RepairKind::of(LandingState::Conflicted),
            Some(RepairKind::Resolver)
        );
        assert_eq!(
            RepairKind::of(LandingState::GateFailed),
            Some(RepairKind::Fix)
        );
        for s in [
            LandingState::Building,
            LandingState::Verifying,
            LandingState::Ready,
            LandingState::InBundle,
            LandingState::Landed,
            LandingState::Abandoned,
        ] {
            assert_eq!(RepairKind::of(s), None, "{s:?}");
        }
        for k in [RepairKind::Resolver, RepairKind::Fix] {
            assert_eq!(RepairKind::parse(k.as_str()), Some(k));
            assert_eq!(RepairKind::of(k.state()), Some(k));
        }
    }

    #[test]
    fn tail_of_keeps_the_last_lines() {
        let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let tail = tail_of(text.as_bytes());
        assert!(tail.starts_with("line 41\n"), "{tail}");
        assert!(tail.ends_with("line 100"), "{tail}");
        let long = "x".repeat(OUTPUT_TAIL_BYTES * 2);
        assert_eq!(tail_of(long.as_bytes()).len(), OUTPUT_TAIL_BYTES);
    }

    #[test]
    fn resolver_section_names_each_file_and_the_merge() {
        let s = resolver_section("fleet/r1-T1-a", "fleet/r1", &["a.txt".into()], "`ct`");
        assert!(s.starts_with("## Resolver attempt\n"), "{s}");
        assert!(s.contains("- `a.txt`\n"), "{s}");
        assert!(s.contains("`git merge fleet/r1`"), "{s}");
        assert!(s.contains("zdiff3"), "{s}");
        assert!(s.contains("`ct`"), "{s}");
    }

    #[test]
    fn fix_section_shows_the_output_in_a_fence() {
        let output = GateOutput {
            text: "exit 101".into(),
            path: "/f/gate-output/7.log".into(),
            tail: Some("test a ... FAILED".into()),
        };
        let s = fix_section("fleet/r1-T1-a", &output, "`ct`");
        assert!(s.starts_with("## Fix attempt\n"), "{s}");
        assert!(s.contains("~~~text\ntest a ... FAILED\n~~~"), "{s}");
        assert!(s.contains("`/f/gate-output/7.log`"), "{s}");
    }
}
