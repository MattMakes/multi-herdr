use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::Kind as JobKind;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::integration::application::commands::{self as integration, RefTips, Rescued};
use crate::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeMode, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{
    self, create_worktree, CreateOutcome, Lease,
};
use crate::fleetd::worktree_custody::application::{queries, remove, rescue_refs};
use crate::fleetd::worktree_custody::domain::rescue::REF_PREFIX;
use crate::fleetd::worktree_custody::domain::rescue_class;
use crate::taskledger::model::{Actor, TriageDecision, WorktreeStatus};
use crate::taskledger::triage::{apply as triage_apply, refs, reply::Decided, Row};
use crate::taskledger::{finish, runs, util, workers, worktree_ops};

pub struct Custody {
    pub guard: Generation,
    pub config: FleetConfig,
    pub refs: Box<dyn RefTips + Send + Sync>,
}

impl Custody {
    fn jobs(&self) -> JobCtx<'_> {
        JobCtx {
            guard: Some(self.guard),
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }
}

pub fn register(handlers: &mut Handlers, custody: Arc<Custody>) {
    let with = |f: fn(&Custody, &Connection, &Value) -> Result<Value>| {
        let custody = Arc::clone(&custody);
        move |tx: &rusqlite::Transaction<'_>, payload: &Value| f(&custody, tx, payload)
    };
    handlers.insert(Kind::JobRequeue, with(job_requeue));
    handlers.insert(Kind::WorktreeScratch, with(worktree_scratch));
    handlers.insert(Kind::WorktreeDrop, with(worktree_drop));
    handlers.insert(Kind::WorktreeRestore, with(worktree_restore));
    handlers.insert(Kind::WorktreeTriage, with(worktree_triage));
    handlers.insert(Kind::BranchAbandon, with(branch_abandon));
    handlers.insert(Kind::FeatureAbandon, with(feature_abandon));
    handlers.insert(Kind::RescueDrop, with(rescue_drop));
}

fn payload<T: DeserializeOwned>(kind: &str, payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).with_context(|| format!("a bad {kind} payload"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRequeue {
    job: i64,
}

fn job_requeue(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let JobRequeue { job } = payload("job_requeue", p)?;
    let row = jobs_store::get(tx, job)?.with_context(|| format!("no job {job}"))?;
    let key = JobKey {
        kind: row.kind,
        subject: &row.subject,
        input_key: &row.input_key,
    };
    match jobs::requeue(&c.jobs(), tx, &key)? {
        Queued::New(id) => {
            let new = jobs_store::get(tx, id)?.with_context(|| format!("no job {id}"))?;
            Ok(json!({
                "job": id,
                "run": new.run,
                "kind": new.kind.to_string(),
                "subject": new.subject,
            }))
        }
        Queued::Active(open) => bail!(
            "job {} of the same kind and subject is {}: no new run",
            open.id,
            open.state.as_str()
        ),
        Queued::Reused(r) | Queued::Held(r) => bail!("job {}: no new run", r.id),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeScratch {
    worker_id: String,
    name: String,
    base: String,
    base_commit: String,
    path_exists: bool,
    branch_exists: bool,
}

fn worktree_scratch(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let s: WorktreeScratch = payload("worktree_scratch", p)?;
    worktree_ops::valid_name(&s.name)?;
    let worker = workers::get(tx, &s.worker_id)?;
    let run = runs::get(tx, worker.run_id.as_deref().context("worker has no run")?)?;
    let base = run
        .run_branch
        .as_deref()
        .or(run.base_branch.as_deref())
        .context("run has no git base")?;
    if base != s.base {
        bail!("the base of the run is {base}, not {}", s.base);
    }
    let (path, branch) = worktree_ops::tree_names(&run, None, &s.name);
    if s.path_exists || s.branch_exists {
        bail!("worktree path or branch already exists; pick another name");
    }
    let new = NewWorktree {
        worktree_id: util::new_id(),
        task_id: None,
        worker_id: Some(s.worker_id.clone()),
        repo_root: run.project_root.clone(),
        path: path.to_string_lossy().into_owned(),
        branch,
        base: base.to_string(),
        base_commit: Some(s.base_commit),
        status: WorktreeStatus::Open,
        mode: Some(WorktreeMode {
            run_id: run.run_id.clone(),
            name: s.name,
            scratch: true,
        }),
    };
    let attempt = finish::live_attempt(tx, &s.worker_id)?;
    let lease = Lease {
        task_id: None,
        worker_id: &s.worker_id,
        attempt: attempt.as_deref(),
        from: None,
        new: Some(&new),
    };
    let (row, job) = reserve(c, tx, &lease)?;
    Ok(json!({
        "worktree_id": row.worktree_id,
        "path": row.path,
        "branch": row.branch,
        "job": job,
    }))
}

fn reserve(c: &Custody, tx: &Connection, lease: &Lease<'_>) -> Result<(trees::WorktreeRow, i64)> {
    let clock = create::fleet_clock(tx)?;
    match create_worktree(&c.jobs(), tx, lease, clock)? {
        CreateOutcome::Reserved { row, job } => Ok((row, job)),
        CreateOutcome::Attached(row) | CreateOutcome::NeedsRescue(row) => {
            bail!("worktree {} exists already", row.path)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeDrop {
    run_id: String,
    name: String,
    reason: String,
    keep_branch: bool,
    facts: DropFacts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DropFacts {
    path_exists: bool,
    dirty: Option<bool>,
    head_branch: Option<String>,
    head_is_recorded: Option<bool>,
    stash: bool,
    unique_commits: bool,
    head: String,
}

fn worktree_drop(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let d: WorktreeDrop = payload("worktree_drop", p)?;
    if d.reason.trim().is_empty() {
        bail!("give a nonempty --reason");
    }
    let wt = worktree_ops::find_in(tx, &d.run_id, &d.name)?;
    refuse_drop(&d, &wt.branch)?;
    if d.facts.path_exists {
        refuse_moved_head(c, Path::new(&wt.path), &d.facts.head)?;
    }
    let job = if d.facts.path_exists {
        Some(queue_drop(
            c,
            tx,
            &wt.worktree_id,
            &d.facts.head,
            d.keep_branch,
        )?)
    } else {
        None
    };
    worktree_ops::record_drop(tx, &wt.worktree_id, &d.reason)?;
    Ok(json!({
        "worktree_id": wt.worktree_id,
        "path": wt.path,
        "reason": d.reason,
        "job": job,
    }))
}

fn refuse_drop(d: &WorktreeDrop, recorded: &str) -> Result<()> {
    let f = &d.facts;
    if f.path_exists && fact(f.dirty, "dirty")? {
        bail!("worktree has uncommitted or untracked changes; nothing removed");
    }
    if f.stash {
        bail!("worktree branch has a stash; nothing removed");
    }
    if f.path_exists {
        let Some(branch) = &f.head_branch else {
            bail!("worktree HEAD is detached or unreadable; reattach the recorded branch {recorded} before dropping it; nothing removed");
        };
        if branch != recorded {
            bail!("worktree is on {branch}, not its recorded branch {recorded}; nothing removed (including with --keep-branch)");
        }
        if !fact(f.head_is_recorded, "head_is_recorded")? {
            bail!("worktree HEAD differs from its recorded branch {recorded}; nothing removed");
        }
    }
    if !d.keep_branch && f.unique_commits {
        bail!("worktree has unpublished commits that exist nowhere else; use --keep-branch; nothing removed");
    }
    Ok(())
}

fn refuse_moved_head(c: &Custody, path: &Path, checked: &str) -> Result<()> {
    match c.refs.tip(path, "HEAD")? {
        Some(head) if head == checked => Ok(()),
        Some(_) => bail!("worktree HEAD changed during the drop checks; nothing removed"),
        None => bail!("worktree HEAD is unreadable; nothing removed"),
    }
}

fn fact(value: Option<bool>, name: &str) -> Result<bool> {
    value.with_context(|| format!("the fact {name} is unknown; nothing removed"))
}

fn queue_drop(
    c: &Custody,
    tx: &Connection,
    id: &str,
    head: &str,
    keep_branch: bool,
) -> Result<i64> {
    let ctx = c.jobs();
    let clock = create::fleet_clock(tx)?;
    let row = trees::get(tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    if matches!(row.state, WorktreeState::Landed | WorktreeState::Rescued) {
        return match remove::queue_remove(&ctx, tx, &row, "dropped", keep_branch, clock)? {
            Ok(q) => queued_id(q, "worktree_remove", &row.path),
            Err(why) => bail!("the worktree stays: {why}; nothing removed"),
        };
    }
    if !matches!(row.state, WorktreeState::Landing | WorktreeState::Orphaned) {
        let cause = Cause {
            actor: Actor::Worker,
            text: "dropped",
        };
        transition(
            tx,
            ctx.guard,
            Entity::Worktree,
            id,
            "orphaned",
            cause,
            ctx.needs_you,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "the worktree stays: worktree {id} is {}: {e}; nothing removed",
                row.state
            )
        })?;
        trees::set_owner(tx, id, None)?;
    }
    let input_key = rescue_key(&row.path, head);
    let key = JobKey {
        kind: JobKind::Rescue,
        subject: id,
        input_key: &input_key,
    };
    let q = jobs::queue(&ctx, tx, &key, Some(&row.repo_root), clock)?;
    let job = queued_id(q, "rescue", &row.path)?;
    let ended = remove::OwnerEnded {
        keep_branch,
        rescue_job: Some(job),
    };
    remove::record_owner_end(&ctx, tx, id, Actor::Worker, &ended)?;
    Ok(job)
}

fn queued_id(q: Queued, kind: &str, path: &str) -> Result<i64> {
    match q {
        Queued::New(id) => Ok(id),
        Queued::Active(r) | Queued::Reused(r) => Ok(r.id),
        Queued::Held(r) => bail!(
            "the {kind} job of {path} ended abandoned {} times; run horch job requeue {} after you read why",
            r.abandoned_runs,
            r.id
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeRestore {
    name: String,
    run_id: Option<String>,
    root: String,
    facts: RestoreFacts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreFacts {
    path_exists: bool,
    branch_exists: bool,
    branch_commit: Option<String>,
    rescue_commit: Option<String>,
    archive: Option<RefAt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefAt {
    name: String,
    commit: String,
}

fn worktree_restore(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let r: WorktreeRestore = payload("worktree_restore", p)?;
    match queries::rescue_source(tx, r.run_id.as_deref(), &r.name)? {
        Some(source) => restore_rescue(c, tx, &r.facts, &source),
        None => restore_archive(c, tx, &r),
    }
}

fn restore_rescue(
    c: &Custody,
    tx: &Connection,
    f: &RestoreFacts,
    source: &queries::RescueSource,
) -> Result<Value> {
    let repo = PathBuf::from(&source.row.repo_root);
    let slug = format!("T{}-{}", source.seq, source.n);
    let branch = format!("rescue/{slug}");
    let path = repo.join(".worktrees").join(format!("rescue-{slug}"));
    if f.branch_exists {
        bail!("the branch {branch} exists; restore changed nothing");
    }
    if f.path_exists {
        bail!("{} exists; restore changed nothing", path.display());
    }
    let commit = f
        .rescue_commit
        .clone()
        .context("the fact rescue_commit is unknown; restore changed nothing")?;
    let new = NewWorktree {
        worktree_id: util::new_id(),
        task_id: None,
        worker_id: None,
        repo_root: source.row.repo_root.clone(),
        path: path.to_string_lossy().into_owned(),
        branch: branch.clone(),
        base: source.rescue_ref.clone(),
        base_commit: Some(commit),
        status: WorktreeStatus::Open,
        mode: None,
    };
    let (row, job) = reserve(c, tx, &operator_lease(&new))?;
    Ok(json!({
        "worktree_id": row.worktree_id,
        "job": job,
        "line": format!("restored {} on {branch} at {}", source.rescue_ref, path.display()),
    }))
}

fn operator_lease(new: &NewWorktree) -> Lease<'_> {
    Lease {
        task_id: None,
        worker_id: "",
        attempt: None,
        from: None,
        new: Some(new),
    }
}

fn restore_archive(c: &Custody, tx: &Connection, r: &WorktreeRestore) -> Result<Value> {
    let f = &r.facts;
    let name = &r.name;
    let root = Path::new(&r.root);
    let Some(at) = &f.archive else {
        bail!("no {} and no trash ref of {name}", refs::archive_ref(name));
    };
    let path = root.join(".worktrees").join(name);
    if f.path_exists {
        bail!("{} exists already", path.display());
    }
    let old = match triage_apply::archived_row(tx, root, &at.name)? {
        Some(id) => trees::get(tx, &id)?,
        None => None,
    };
    let branch = old
        .as_ref()
        .map(|o| o.branch.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| format!("restored/{name}"));
    let path_text = path.to_string_lossy().into_owned();
    if f.branch_exists {
        let tip = f
            .branch_commit
            .as_deref()
            .context("the fact branch_commit is unknown; restore changed nothing")?;
        if tip != at.commit {
            bail!("the branch {branch} moved since {}; check it out by hand: git worktree add {path_text} {branch}", at.name);
        }
    }
    let new = NewWorktree {
        worktree_id: util::new_id(),
        task_id: old.as_ref().and_then(|o| o.task_id.clone()),
        worker_id: None,
        repo_root: r.root.clone(),
        path: path_text.clone(),
        branch: branch.clone(),
        base: old
            .as_ref()
            .map_or_else(|| branch.clone(), |o| o.base.clone()),
        base_commit: Some(at.commit.clone()),
        status: WorktreeStatus::Open,
        mode: old.and_then(|o| o.mode),
    };
    let (row, job) = reserve(c, tx, &operator_lease(&new))?;
    triage_apply::note_restored(tx, &row.worktree_id, &at.name)?;
    Ok(json!({
        "worktree_id": row.worktree_id,
        "job": job,
        "line": format!(
            "restored {path_text} on {branch} at {} from {}",
            at.commit.get(..7).unwrap_or(&at.commit),
            at.name
        ),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeTriage {
    root: String,
    primary: String,
    decisions: Vec<TriageItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriageItem {
    row: Row,
    decision: String,
    direction: Option<String>,
    line: usize,
    ticket: Option<TicketFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketFile {
    path: String,
    sha256: String,
}

fn worktree_triage(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let t: WorktreeTriage = payload("worktree_triage", p)?;
    let root = Path::new(&t.root);
    let mut decided = Vec::new();
    let mut for_job = Vec::new();
    for item in &t.decisions {
        let decision = TriageDecision::parse(&item.decision)?;
        if triage_apply::JOB_DECISIONS.contains(&decision) {
            for_job.push((item.row.clone(), decision, item.direction.clone()));
            continue;
        }
        let ticket = match &item.ticket {
            Some(file) => Some(checked_ticket(c, root, file)?),
            None => None,
        };
        let d = Decided {
            id: item.row.id.clone(),
            name: item.row.name.clone(),
            decision,
            direction: item.direction.clone(),
            line: item.line,
        };
        decided.push((item, d, ticket));
    }
    let mut applied = Vec::new();
    for (item, d, ticket) in &decided {
        let a =
            triage_apply::apply_in_ledger(tx, root, &t.primary, &item.row, d, ticket.as_deref())?;
        applied.push(json!({
            "id": a.id,
            "name": a.name,
            "decision": a.decision.as_str(),
            "ok": a.ok,
            "text": a.text,
            "line": a.line(),
            "queued": a.queued.as_ref().map(|q| json!({
                "run_id": q.run_id,
                "task_id": q.task_id,
                "task": q.task,
            })),
        }));
    }
    let failed = applied.iter().filter(|a| a["ok"] == false).count();
    if for_job.is_empty() {
        return Ok(json!({ "applied": applied, "failed": failed, "job": null, "queued": [] }));
    }
    let job = triage_apply::queue_job(&c.jobs(), tx, root, &for_job)?;
    let queued: Vec<String> = for_job
        .iter()
        .map(|(row, d, _)| {
            format!(
                "{} {} {d}: queued in the triage_apply job {job}; horch worktree triage shows the result",
                row.id, row.name
            )
        })
        .collect();
    Ok(json!({ "applied": applied, "failed": failed, "job": job, "queued": queued }))
}

fn checked_ticket(c: &Custody, root: &Path, file: &TicketFile) -> Result<String> {
    let full = root.join(&file.path);
    let limit = c.config.request_max_bytes;
    let size = std::fs::metadata(&full)
        .with_context(|| format!("reading the brief {}", full.display()))?
        .len();
    if size > limit {
        bail!(
            "the brief {} has {size} bytes, above {limit}",
            full.display()
        );
    }
    let bytes =
        std::fs::read(&full).with_context(|| format!("reading the brief {}", full.display()))?;
    if sha256_bytes(&bytes).to_string() != file.sha256 {
        bail!("the brief {} differs from its sha256", full.display());
    }
    Ok(file.path.clone())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchAbandon {
    branch: String,
    rescue_ref: Option<String>,
    tip: Option<String>,
}

fn branch_abandon(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let BranchAbandon {
        branch,
        rescue_ref,
        tip,
    } = payload("branch_abandon", p)?;
    let rescued = Rescued { branch, rescue_ref };
    let repo = integration::repo_of(tx, &rescued.branch)?;
    integration::verify(c.refs.as_ref(), &repo, &rescued, tip.as_deref())?;
    let done = integration::record(&c.jobs(), tx, &rescued, "the operator abandons it")?;
    Ok(json!({
        "branch": done.branch,
        "rescue_ref": done.rescue_ref,
        "already": done.already,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureAbandon {
    feature: String,
    branches: Vec<BranchAbandon>,
}

fn feature_abandon(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let FeatureAbandon { feature, branches } = payload("feature_abandon", p)?;
    let repo = integration::feature_repo(tx, &feature)?;
    let mut rescued = Vec::with_capacity(branches.len());
    for BranchAbandon {
        branch,
        rescue_ref,
        tip,
    } in branches
    {
        let r = Rescued { branch, rescue_ref };
        integration::verify(c.refs.as_ref(), &repo, &r, tip.as_deref())?;
        rescued.push(r);
    }
    let why = "the operator abandons the feature";
    let done = integration::record_feature(&c.jobs(), tx, &feature, &rescued, why)?;
    let branches: Vec<Value> = done
        .branches
        .iter()
        .map(|b| json!({ "branch": b.branch, "rescue_ref": b.rescue_ref }))
        .collect();
    Ok(json!({
        "feature": done.feature,
        "already": done.already,
        "branches": branches,
        "to_stop": done.to_stop(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RescueDrop {
    repo: PathBuf,
    name: String,
    tip: String,
    class: String,
    bytes: Option<u64>,
}

fn rescue_drop(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let RescueDrop {
        repo,
        name,
        tip,
        class,
        bytes,
    } = payload("rescue_drop", p)?;
    let repo_text = repo.to_string_lossy().into_owned();
    if !repo.is_absolute() || runs::of_project(tx, &repo)?.is_empty() {
        bail!("{repo_text} is not the root of a project of the ledger");
    }
    if rescue_class::parse(&name).is_none() {
        bail!("{name} is not a rescue ref: horch rescue drop deletes only a ref {REF_PREFIX}<owner>/<n>");
    }
    let class = match class.as_str() {
        "unique" => rescue_class::Class::Unique,
        "superseded" => rescue_class::Class::Superseded,
        other => bail!("the class {other:?} is not unique or superseded"),
    };
    match c.refs.tip(&repo, &name)? {
        Some(at) if at == tip => {}
        Some(at) => bail!("the rescue ref {name} is at {at}, not at {tip}"),
        None => bail!("the rescue ref {name} does not exist in {repo_text}"),
    }
    let plan = rescue_refs::DropPlan {
        deletion: rescue_refs::Deletion {
            repo: repo_text,
            name,
            tip,
        },
        class,
        bytes,
    };
    let recorded = rescue_refs::record_drop(tx, &plan)?;
    Ok(json!({ "ref": plan.deletion.name, "tip": plan.deletion.tip, "recorded": recorded }))
}
