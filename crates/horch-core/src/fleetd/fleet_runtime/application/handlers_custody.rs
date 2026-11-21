//! The handlers of the custody, job and integration request kinds:
//! `job_requeue`, `worktree_scratch`, `worktree_drop`, `worktree_restore`,
//! `worktree_triage`, `feature_abandon`, `branch_abandon` and `rescue_drop`
//! (T-885).
//!
//! Requirements: RR-5, RR-8, RR-13 to RR-18 (ticket 20 of fleetd-p5).
//!
//! Each handler writes the rows of the `horch` command of today, through
//! the same functions, in the transaction of the request. It runs no git
//! and no process: a git command is a job, and the handler queues its row.
//!
//! A git fact that the command of today reads before its first write (a
//! dirty tree, a stash on the branch, unique commits, the HEAD, a base
//! commit, an existing branch or path) comes in the payload. The command
//! reads it, and only an operator request with the token reaches a handler.
//! A missing fact refuses the request. The `worktree_remove` job guard reads
//! the facts that guard work from loss (the stash, the HEAD, a dirty landed
//! tree) again at run time.

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

/// What the handlers of this file read: the generation of the fleetd that
/// applies the requests, its `fleet.toml`, and the refs of git, read with
/// no write (the `branch_abandon` handler verifies its rescue ref).
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

/// Register the handler of each kind of this file in `handlers`.
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

/// The payload of a kind, with no unknown field.
fn payload<T: DeserializeOwned>(kind: &str, payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).with_context(|| format!("a bad {kind} payload"))
}

// ─── job_requeue ────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRequeue {
    /// The id of a run of the job.
    job: i64,
}

/// `horch job requeue <job>`: 1 more run of the key of the job, with its
/// count of abandoned runs at 0, and its needs-you item resolved.
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

// ─── worktree_scratch ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeScratch {
    /// The worker of the scratch tree.
    worker_id: String,
    name: String,
    /// The base of the tree, as the command read it: the run branch, else
    /// the base branch of the run.
    base: String,
    /// The commit of `base`.
    base_commit: String,
    /// The path of the tree exists.
    path_exists: bool,
    /// The branch of the tree exists.
    branch_exists: bool,
}

/// `horch ledger worktree scratch <name>`: 1 `reserved` scratch tree of the
/// worker and its `worktree_add` job.
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

/// The `reserved` row of `lease` and its `worktree_add` job. A lease with a
/// new row always reserves it.
fn reserve(c: &Custody, tx: &Connection, lease: &Lease<'_>) -> Result<(trees::WorktreeRow, i64)> {
    let clock = create::fleet_clock(tx)?;
    match create_worktree(&c.jobs(), tx, lease, clock)? {
        CreateOutcome::Reserved { row, job } => Ok((row, job)),
        CreateOutcome::Attached(row) | CreateOutcome::NeedsRescue(row) => {
            bail!("worktree {} exists already", row.path)
        }
    }
}

// ─── worktree_drop ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeDrop {
    /// The run that owns the worktree.
    run_id: String,
    name: String,
    reason: String,
    keep_branch: bool,
    facts: DropFacts,
}

/// The git facts of a drop, as the command read them. The facts of the
/// folder are `None` when the folder is gone.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DropFacts {
    /// The folder exists.
    path_exists: bool,
    /// The folder has an uncommitted or untracked file.
    dirty: Option<bool>,
    /// The branch of the HEAD of the folder; `None` for a detached HEAD.
    head_branch: Option<String>,
    /// The HEAD of the folder equals the recorded branch.
    head_is_recorded: Option<bool>,
    /// A `git stash` entry names the branch.
    stash: bool,
    /// The HEAD has commits that no other branch, remote or tag has.
    unique_commits: bool,
    /// The commit of the work: the HEAD of the folder, else the branch.
    head: String,
}

/// `horch ledger worktree drop <name> --reason`: the refusals of today
/// (row W6, RR-13), then the removal through custody: a `landed` or
/// `rescued` tree gets its `worktree_remove` job; each other tree moves to
/// `orphaned` (cause `dropped`) and gets its `rescue` job. The row is
/// `abandoned` with the reason.
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

/// The refusals of today's drop, in their order, from the facts of `d`. A
/// missing fact of a folder that exists refuses.
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

/// The fact `name`, or a refusal when the command did not read it.
fn fact(value: Option<bool>, name: &str) -> Result<bool> {
    value.with_context(|| format!("the fact {name} is unknown; nothing removed"))
}

/// The job of the removal of the tree `id` at the commit `head`: its
/// `worktree_remove` job when it is `landed` or `rescued`, else its `rescue`
/// job after the move to `orphaned`.
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
    queued_id(q, "rescue", &row.path)
}

/// The id of the run of a queued job of `kind` for the tree at `path`. A
/// held key refuses, as the command of today does.
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

// ─── worktree_restore ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeRestore {
    /// A task (`T<seq>` of `run_id`), a worktree id, or a triage name.
    name: String,
    /// The active run, when the project has 1.
    run_id: Option<String>,
    /// The main checkout of the project.
    root: String,
    facts: RestoreFacts,
}

/// The git facts of a restore, as the command read them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreFacts {
    /// The path of the new tree exists.
    path_exists: bool,
    /// The branch of the new tree exists.
    branch_exists: bool,
    /// The commit of the branch of the new tree, when it exists.
    branch_commit: Option<String>,
    /// The commit of the rescue ref of the name, when it has one.
    rescue_commit: Option<String>,
    /// The archive ref of the name, else its newest trash ref, and the
    /// commit of that ref.
    archive: Option<RefAt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefAt {
    name: String,
    commit: String,
}

/// `horch worktree restore <name>`: the newest rescue ref of a task or a
/// worktree id, else the archive or trash ref of a triage name, gives a new
/// `reserved` tree and its `worktree_add` job.
fn worktree_restore(c: &Custody, tx: &Connection, p: &Value) -> Result<Value> {
    let r: WorktreeRestore = payload("worktree_restore", p)?;
    match queries::rescue_source(tx, r.run_id.as_deref(), &r.name)? {
        Some(source) => restore_rescue(c, tx, &r.facts, &source),
        None => restore_archive(c, tx, &r),
    }
}

/// The branch `rescue/T<seq>-<n>` at the rescue commit, in its tree
/// `<project>/.worktrees/rescue-T<seq>-<n>`.
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

/// The lease of a restored tree: no task and no worker, so the request
/// never attaches another tree, and the new row has no owner.
fn operator_lease(new: &NewWorktree) -> Lease<'_> {
    Lease {
        task_id: None,
        worker_id: "",
        attempt: None,
        from: None,
        new: Some(new),
    }
}

/// The folder `<root>/.worktrees/<name>` again from its archive or trash
/// ref, on the branch of the archived row, else `restored/<name>`.
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

// ─── worktree_triage ────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorktreeTriage {
    /// The main checkout of the project.
    root: String,
    /// The persona of the tasks that `FINISH` queues.
    primary: String,
    /// The decisions of the reply that the command parsed, in its order.
    decisions: Vec<TriageItem>,
}

/// 1 decision of a triage reply, with the facts of its row.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriageItem {
    row: Row,
    decision: String,
    direction: Option<String>,
    /// The 1-based line of the reply.
    line: usize,
    /// The brief of a `FINISH` that the command wrote, relative to `root`.
    ticket: Option<TicketFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketFile {
    path: String,
    sha256: String,
}

/// `horch worktree triage apply`: each `KEEP` and `FINISH` decision of the
/// reply, as the command of today applies it. The decisions of
/// [`triage_apply::JOB_DECISIONS`] (`ARCHIVE`, `DELETE`, `LAND` and `PR`)
/// run git or gh: they go to 1 `triage_apply` job, and fleetd writes their
/// rows from its result file.
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

/// The path of the brief `file` under `root`, after its read with the size
/// limit of a request and the check of its sha256.
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

// ─── branch_abandon ─────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchAbandon {
    branch: String,
    /// The rescue ref that the command wrote, or `None` when git has no
    /// such branch.
    rescue_ref: Option<String>,
    /// The tip of the branch that the rescue ref holds.
    tip: Option<String>,
}

/// `horch branch abandon <branch>`: the command writes the rescue ref of
/// the tip first (I10). The handler reads the branch and the ref again in
/// git, with no write, and refuses facts that git does not confirm. Then it
/// records the ref and moves the branch to `abandoned`.
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

// ─── feature_abandon ────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureAbandon {
    feature: String,
    /// The rescue ref that the command wrote for each task branch of the
    /// feature that is not landed, with its tip.
    branches: Vec<BranchAbandon>,
}

/// `horch feature abandon <feature>`: the command writes the rescue ref of
/// each task branch of the feature that is not landed first (I10). The
/// handler reads each branch and each ref again in git, with no write, and
/// refuses facts that git does not confirm. Then it records the refs, moves
/// the branches, cancels the open tasks of the run and moves the feature
/// (`integration::record_feature`). The caller stops the workers of
/// `to_stop`; else the pass completes each cancel that waits for a stop.
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

// ─── rescue_drop ────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RescueDrop {
    /// The main checkout of a project of the ledger.
    repo: PathBuf,
    /// The full name of the rescue ref.
    name: String,
    /// The tip of the ref that the command showed.
    tip: String,
    /// `unique` or `superseded`, as the command classed the ref.
    class: String,
    bytes: Option<u64>,
}

/// `horch rescue drop <ref>`: the command reads the facts of the ref and
/// shows them. The handler checks that the repository is a project of the
/// ledger and that git confirms the ref at its tip, with no write. Then it
/// records the drop with 1 event (`rescue_refs::record_drop`). The command
/// deletes the ref after the reply: the effect runs after the commit.
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
