//! Applying the decisions of a triage reply (#308, section 3).
//!
//! Each decision runs on its own, records 1 event and sets the row's
//! `decision` and `decided_at`. The rules of section 5 hold for every one:
//! git is never forced; before a folder is removed, `unsafe_to_remove` (or
//! an archive or trash ref that holds the tip) proves that no commit and no
//! uncommitted file is lost; a failed step deletes nothing and leaves the
//! row `cleanup_failed` with the reason; `LAND` never moves the default
//! branch.
//!
//! A folder goes only through the custody jobs (fleetd-p3 ticket 13,
//! RR-9): after the commit of its files and the read-back of its archive or
//! trash ref, a `rescue` job puts it on a verified rescue ref, then a
//! `worktree_remove` job removes it. A restore makes its tree through a
//! `worktree_add` job.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use horch_platform::start;
use rusqlite::{params, Connection};

use super::reply::Decided;
use super::{human_bytes, listed_worktrees, refs, Row};
use crate::fleetd::fleet_runtime::application::jobs::{outcome, JobCtx, Queued as JobQueued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{self, Lease, RunJob};
use crate::fleetd::worktree_custody::application::remove;
use crate::fleetd::worktree_custody::domain::worktree::{settle_refusal, Folder};
use crate::runtime::process::scrub_child_env;
use crate::taskledger::finish::{self, land, WorktreeRow};
use crate::taskledger::git::{dir_bytes, Git};
use crate::taskledger::model::{Actor, EventKind, FindingStatus, TriageDecision, WorktreeStatus};
use crate::taskledger::sql_words::word;
use crate::taskledger::store::{optional, Store, BUSY_TIMEOUT};
use crate::taskledger::tasks::{self, NewTask};
use crate::taskledger::util::{self, About};
use crate::taskledger::{import, runs};

/// What the decisions run with.
#[derive(Clone)]
pub struct Env<'a> {
    pub git: &'a Git,
    /// The main checkout.
    pub root: &'a Path,
    /// Where the land lock lives (the finish steps' evidence root).
    pub evidence_root: &'a Path,
    /// `gh`, when `PR` is offered.
    pub gh: Option<&'a Path>,
    /// The remote `PR` pushes to, when it is offered.
    pub remote: Option<&'a str>,
    /// The persona of the tasks that `FINISH` queues.
    pub primary: &'a str,
    /// What runs the `rescue` and `worktree_remove` jobs of a removal.
    pub jobs: &'a dyn RunJob,
}

/// A task that `FINISH` queued: the caller wakes the run's orchestrator.
#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub run_id: String,
    pub task_id: String,
    /// `T-<seq>`.
    pub task: String,
}

/// The result of 1 decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub id: String,
    pub name: String,
    pub decision: TriageDecision,
    pub ok: bool,
    /// The result line.
    pub text: String,
    pub queued: Option<Queued>,
}

impl Applied {
    /// `W1 T7-cache ARCHIVE: <text>`, or `... FAILED: <text>`.
    pub fn line(&self) -> String {
        format!(
            "{} {} {}{}: {}",
            self.id,
            self.name,
            self.decision,
            if self.ok { "" } else { " FAILED" },
            self.text
        )
    }
}

/// What a decision did, for its record.
#[derive(Debug, Default)]
struct Done {
    /// The new status; `None` keeps the one the row has (or was given).
    status: Option<WorktreeStatus>,
    archive_ref: Option<String>,
    bytes_freed: Option<u64>,
    text: String,
    queued: Option<Queued>,
}

/// The remote `PR` uses, when `gh` runs and the repository has a remote.
pub fn pr_remote(git: &Git, root: &Path, gh: &Path) -> Option<String> {
    let mut version = start::command(gh);
    version.arg("--version");
    scrub_child_env(&mut version);
    let gh_runs = version.output().is_ok_and(|o| o.status.success());
    if !gh_runs {
        return None;
    }
    if git.has_remote(root, "origin") {
        return Some("origin".into());
    }
    let remotes = git.run(root, &["remote"]).ok()?;
    remotes.lines().next().map(str::to_string)
}

/// Run every decision of a parsed reply, in reply order.
pub fn apply(
    store: &mut Store,
    env: &Env<'_>,
    rows: &[Row],
    decided: &[Decided],
) -> Result<Vec<Applied>> {
    let mut out = Vec::new();
    for d in decided {
        let row = rows
            .iter()
            .find(|r| r.id == d.id)
            .with_context(|| format!("{} is not a row of this triage", d.id))?;
        out.push(apply_one(store, env, row, d)?);
    }
    Ok(out)
}

fn apply_one(store: &mut Store, env: &Env<'_>, row: &Row, d: &Decided) -> Result<Applied> {
    let wt = store.immediate(|tx| adopt(tx, env.root, row))?;
    let result = match d.decision {
        TriageDecision::Keep => Ok(Done {
            text: "kept; it comes back at the next triage".into(),
            ..Default::default()
        }),
        TriageDecision::Archive => archive(store, env, row, &wt),
        TriageDecision::Delete => delete(store, env, row, &wt),
        TriageDecision::Land => land_decision(store, env, row, &wt),
        TriageDecision::Finish => finish_decision(store, env, row, &wt, direction(d)),
        TriageDecision::Pr => pr(store, env, row, &wt, direction(d)),
    };
    let (ok, text, queued) = match result {
        Ok(done) => {
            record(store.conn(), row, &wt, d.decision, &done)?;
            (true, done.text, done.queued)
        }
        Err(e) => {
            let why = format!("{e:#}");
            failed(store.conn(), row, &wt, d.decision, &why)?;
            (false, why, None)
        }
    };
    Ok(Applied {
        id: d.id.clone(),
        name: d.name.clone(),
        decision: d.decision,
        ok,
        text,
        queued,
    })
}

fn direction(d: &Decided) -> &str {
    d.direction.as_deref().unwrap_or("")
}

/// The cause of each custody write of triage.
const TRIAGE: Cause<'static> = Cause {
    actor: Actor::Operator,
    text: "triage",
};

/// The reason of a row that triage adopts.
const FOUND: &str = "found on disk by worktree triage";

/// The ledger row of a triage row, in `tx`. A folder that no live row names
/// gets one (`origin = 'import'`) in `orphaned`, through the custody store
/// (row W7), so its decision is recorded like any other. A `removed` row
/// of the same path stays as history.
fn adopt(tx: &Connection, root: &Path, row: &Row) -> Result<WorktreeRow> {
    if let Some(id) = &row.worktree_id {
        return finish::worktree(tx, id);
    }
    let branch = row.branch.as_deref().unwrap_or("");
    let existing: Option<String> = optional(tx.query_row(
        "SELECT worktree_id FROM worktrees WHERE path = ?1 AND state <> 'removed'",
        [&row.path],
        |r| r.get(0),
    ))?;
    let id = match existing {
        Some(id) => {
            tx.execute(
                &format!(
                    "UPDATE worktrees SET status = {abandoned}, branch = ?2, base = ?3, reason = ?4,
                        decision = NULL, decided_at = NULL, cleaned_at = NULL
                     WHERE worktree_id = ?1",
                    abandoned = word(WorktreeStatus::Abandoned)
                ),
                params![id, branch, row.base, FOUND],
            )?;
            id
        }
        None => {
            let new = NewWorktree {
                worktree_id: util::new_id(),
                task_id: None,
                worker_id: None,
                repo_root: root.to_string_lossy().into_owned(),
                path: row.path.clone(),
                branch: branch.to_string(),
                base: row.base.clone(),
                base_commit: None,
                status: WorktreeStatus::Abandoned,
                mode: None,
            };
            trees::insert_orphan(tx, None, &new, TRIAGE)?;
            tx.execute(
                "UPDATE worktrees SET origin = 'import', reason = ?2 WHERE worktree_id = ?1",
                params![new.worktree_id, FOUND],
            )?;
            new.worktree_id
        }
    };
    finish::worktree(tx, &id)
}

/// The event kind that records a decision.
fn event_kind(d: TriageDecision) -> EventKind {
    match d {
        TriageDecision::Land => EventKind::Landed,
        TriageDecision::Pr => EventKind::PrUpdated,
        TriageDecision::Finish => EventKind::Requeued,
        TriageDecision::Archive | TriageDecision::Delete => EventKind::Cleaned,
        TriageDecision::Keep => EventKind::Note,
    }
}

fn about<'a>(row: &'a Row, wt: &'a WorktreeRow) -> About<'a> {
    About {
        run: row.run_id.as_deref(),
        task: wt.task_id.as_deref(),
        worker: None,
    }
}

/// Record a decision that ran: its columns, its event, its findings.
fn record(
    conn: &Connection,
    row: &Row,
    wt: &WorktreeRow,
    d: TriageDecision,
    done: &Done,
) -> Result<()> {
    let cleaned = done.bytes_freed.is_some();
    conn.execute(
        &format!(
            "UPDATE worktrees SET decision = ?2, decided_at = ?3, status = COALESCE(?4, status),
                archive_ref = COALESCE(?5, archive_ref),
                bytes_freed = COALESCE(?6, bytes_freed, CASE WHEN status = {cleaned} THEN bytes_on_disk END),
                cleaned_at = CASE WHEN ?7 THEN ?3 ELSE cleaned_at END
             WHERE worktree_id = ?1",
            cleaned = word(WorktreeStatus::Cleaned)
        ),
        params![
            wt.worktree_id,
            d.as_str(),
            util::now(),
            done.status.map(|s| s.as_str()),
            done.archive_ref,
            done.bytes_freed.map(|b| b as i64),
            cleaned
        ],
    )?;
    util::event(
        conn,
        &about(row, wt),
        Actor::Operator,
        event_kind(d),
        &format!("Triage {} {}: {d}. {}", row.id, row.name, done.text),
    )?;
    for f in &row.findings {
        let (status, note) = match &done.queued {
            Some(q) => (FindingStatus::Requeued, q.task_id.clone()),
            None => (FindingStatus::Dismissed, format!("worktree triage: {d}")),
        };
        import::decide(conn, *f, status, "operator", &note)?;
    }
    Ok(())
}

/// A decision that failed: nothing was deleted; the row is
/// `cleanup_failed` with the reason, and has no decision.
fn failed(
    conn: &Connection,
    row: &Row,
    wt: &WorktreeRow,
    d: TriageDecision,
    why: &str,
) -> Result<()> {
    finish::set_worktree(
        conn,
        &wt.worktree_id,
        WorktreeStatus::CleanupFailed,
        Some(&format!("triage {d}: {why}")),
    )?;
    util::event(
        conn,
        &about(row, wt),
        Actor::Operator,
        EventKind::CleanupFailed,
        &format!("Triage {} {}: {d} failed: {why}", row.id, row.name),
    )
}

/// A checked-out branch must be the recorded branch, when the row records
/// one. A clean detached HEAD is also safe when a durable branch, remote,
/// or tag already reaches it; a folder with no recorded branch is checked
/// too (RR-17).
fn check_identity(git: &Git, wt: &WorktreeRow) -> Result<()> {
    let dir = Path::new(&wt.path);
    if !dir.exists() {
        return Ok(());
    }
    match git.current_branch(dir) {
        Some(_) if wt.branch.is_empty() => Ok(()),
        Some(b) if b == wt.branch => Ok(()),
        Some(b) => bail!(
            "the worktree is on {b}, not its recorded branch {}; nothing removed",
            wt.branch
        ),
        None => {
            if !git.dirty(dir)?.is_empty() {
                bail!("the worktree HEAD is detached with uncommitted files; nothing removed");
            }
            let head = git.rev(dir, "HEAD")?;
            let refs = git.run(
                Path::new(&wt.repo_root),
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "--contains",
                    &head,
                    "refs/heads",
                    "refs/remotes",
                    "refs/tags",
                ],
            )?;
            if refs.is_empty() {
                bail!("the worktree HEAD is detached at a unique commit; nothing removed");
            }
            Ok(())
        }
    }
}

/// Commit every modified, staged and untracked file of the folder. Ignored
/// files are never committed (`git add -A` follows `.gitignore`).
fn commit_wip(git: &Git, wt: &WorktreeRow, name: &str) -> Result<()> {
    let dir = Path::new(&wt.path);
    if !dir.exists() || git.dirty(dir)?.is_empty() {
        return Ok(());
    }
    git.run(dir, &["add", "-A"])?;
    git.run(
        dir,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            &format!("triage: uncommitted work of {name}"),
        ],
    )?;
    if !git.dirty(dir)?.is_empty() {
        bail!("files stay uncommitted after the triage commit; nothing removed");
    }
    Ok(())
}

/// The commit the work is at: the folder's HEAD, else the branch.
fn tip(git: &Git, root: &Path, wt: &WorktreeRow) -> Result<String> {
    let dir = Path::new(&wt.path);
    if dir.exists() {
        return git.rev(dir, "HEAD");
    }
    git.rev(root, &wt.branch)
}

/// Commit the uncommitted files, and return the tip that holds them.
fn prepare(git: &Git, root: &Path, row: &Row, wt: &WorktreeRow) -> Result<String> {
    check_identity(git, wt)?;
    commit_wip(git, wt, &row.name)?;
    tip(git, root, wt)
}

/// `unsafe_to_remove` against `holder`, a ref that holds the work.
fn must_be_safe(git: &Git, root: &Path, wt: &WorktreeRow, holder: &str, tip: &str) -> Result<()> {
    let mut w = wt.clone();
    if w.branch.is_empty() {
        w.branch = tip.to_string();
    }
    if let Some(why) = land::unsafe_to_remove(git, root, &w, holder, None)? {
        bail!("the worktree stays: {why}");
    }
    Ok(())
}

/// Remove the folder, never forced, and only through custody (RR-9): the
/// row is `orphaned` (cause `triage`), a `rescue` job puts the folder on a
/// verified rescue ref, then a `worktree_remove` job removes it. A refused
/// move, a failed rescue or a refused removal keeps the folder. The bytes
/// it held. A folder that is gone already is settled by [`settle_gone`]
/// with `holder`, the ref that the decision `f` wrote.
fn remove_folder(store: &Store, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<u64> {
    let wt = f.wt;
    let dir = PathBuf::from(&wt.path);
    if !dir.exists() {
        settle_gone(store, env, f, holder)?;
        return Ok(0);
    }
    let bytes = dir_bytes(&dir);
    let config = create::fleet_config(store.path())?;
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut conn = open(store)?;
    let row = orphan(&ctx, &mut conn, &wt.worktree_id)?;
    create::rescue_now(&mut conn, &ctx, env.jobs, &row)?;
    remove_now(&ctx, &mut conn, env.jobs, &wt.worktree_id)?;
    if dir.exists() {
        bail!("{} still exists after its worktree_remove job", wt.path);
    }
    Ok(bytes)
}

/// A connection of its own to the ledger of `store`: a job runs with no
/// transaction open, and the custody calls take `&mut Connection`.
fn open(store: &Store) -> Result<Connection> {
    let conn = Connection::open(store.path())
        .with_context(|| format!("opening the task ledger {}", store.path().display()))?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

fn immediate(conn: &mut Connection) -> Result<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

/// The operator decision of `f` on a row whose folder is gone: the
/// answer to the FM-27 escalation. The row goes through `orphaned` to
/// `removed` only when [`settle_refusal`] permits it: the folder is gone,
/// `git worktree list` does not name its path, and `holder`, read back,
/// holds the tip of its branch (or the row has no branch). This is the
/// only caller of the door move `orphaned` to `removed`. A refusal keeps
/// the row.
fn settle_gone(store: &Store, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<()> {
    let wt = f.wt;
    let folder = if Path::new(&wt.path).exists() {
        Folder::Present
    } else {
        Folder::Missing
    };
    let holds_tip = if wt.branch.is_empty() {
        None
    } else {
        let tip = env.git.rev(env.root, &wt.branch).ok();
        Some(tip.is_some() && refs::read(env.git, env.root, holder) == tip)
    };
    let listed = git_lists(env.git, env.root, &wt.path)?;
    if let Some(why) = settle_refusal(folder, listed, holds_tip) {
        bail!("the row stays: {why}");
    }
    let config = create::fleet_config(store.path())?;
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut conn = open(store)?;
    let row = orphan(&ctx, &mut conn, &wt.worktree_id)?;
    let tx = immediate(&mut conn)?;
    let text = format!(
        "triage {}: the folder is gone and {holder} holds its work",
        f.decision
    );
    let cause = Cause {
        actor: Actor::Operator,
        text: &text,
    };
    match transition(
        &tx,
        None,
        Entity::Worktree,
        &wt.worktree_id,
        "removed",
        cause,
        &NeedsYouList,
    ) {
        Ok(_) => Ok(tx.commit()?),
        Err(DoorError::Refused { .. }) => {
            tx.commit()?;
            bail!(
                "the row stays: it is {}, and triage cannot move it to removed",
                row.state
            )
        }
        Err(e) => Err(e.into()),
    }
}

/// Whether `git worktree list` names `path`. A list that git does not give
/// is an error: the fact is unknown.
fn git_lists(git: &Git, root: &Path, path: &str) -> Result<bool> {
    let out = git.try_run(root, &["worktree", "list", "--porcelain"])?;
    if !out.ok {
        bail!("git worktree list failed; the row stays");
    }
    let want = path_key(path);
    Ok(out
        .stdout
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .any(|p| path_key(p) == want))
}

/// `path` with its folder canonical: the path itself may be gone.
fn path_key(path: &str) -> PathBuf {
    let p = Path::new(path);
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) => super::canonical(&dir.to_string_lossy()).join(name),
        _ => p.to_path_buf(),
    }
}

/// The row `id`, moved to `orphaned` unless it is `orphaned` or `rescued`.
/// A `landing` row stays: its rescue moves it to `rescued`. Triage lists
/// only a row that no live worker holds, so an orphaned row has no owner
/// attempt (MAP.md of fleetd-p3, Q10). A move that the door refuses is
/// recorded (1 event, 1 needs-you item) and keeps the folder.
fn orphan(ctx: &JobCtx<'_>, conn: &mut Connection, id: &str) -> Result<trees::WorktreeRow> {
    let tx = immediate(conn)?;
    let row = trees::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let keep = [
        WorktreeState::Orphaned,
        WorktreeState::Rescued,
        WorktreeState::Landing,
    ];
    if keep.contains(&row.state) {
        return Ok(row);
    }
    match transition(
        &tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "orphaned",
        TRIAGE,
        ctx.needs_you,
    ) {
        Ok(_) => {}
        Err(DoorError::Refused { .. }) => {
            tx.commit()?;
            bail!(
                "the worktree stays: its row is {}, and triage cannot move it to orphaned",
                row.state
            );
        }
        Err(e) => return Err(e.into()),
    }
    trees::set_owner(&tx, id, None)?;
    let row = trees::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    tx.commit()?;
    Ok(row)
}

/// Queue the `worktree_remove` job of the row `id`, run it through `jobs`
/// with no transaction open, then observe it in 1 transaction.
fn remove_now(ctx: &JobCtx<'_>, conn: &mut Connection, jobs: &dyn RunJob, id: &str) -> Result<()> {
    let tx = immediate(conn)?;
    let clock = create::fleet_clock(&tx)?;
    let row = trees::get(&tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let job = match remove::queue_remove(ctx, &tx, &row, "triage", false, clock)? {
        Ok(JobQueued::New(job)) => job,
        Ok(JobQueued::Active(r) | JobQueued::Reused(r)) => r.id,
        Ok(JobQueued::Held(r)) => bail!(
            "the worktree_remove job of {} ended abandoned {} times; run horch job requeue {} after you read why",
            row.path,
            r.abandoned_runs,
            r.id
        ),
        Err(why) => bail!("the worktree stays: {why}"),
    };
    tx.commit()?;
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = remove::observe(ctx, &tx, &ended)?;
    tx.commit()?;
    match seen {
        remove::Observed::Removed => Ok(()),
        _ => bail!(
            "its worktree_remove job did not remove it ({}: {}); the worktree stays",
            ended.state.as_str(),
            outcome(&ended).map(|o| o.text).unwrap_or_default()
        ),
    }
}

/// A decision that frees the folder of `wt` (ARCHIVE, DELETE or PR): the
/// row, its worktree, the tip that holds the work once the uncommitted
/// files are committed, and the decision, which names the cause of a
/// settle.
struct Freeing<'a> {
    row: &'a Row,
    wt: &'a WorktreeRow,
    tip: String,
    decision: TriageDecision,
}

impl<'a> Freeing<'a> {
    /// Check the folder, commit its uncommitted files, and read its tip.
    fn prepare(
        env: &Env<'_>,
        row: &'a Row,
        wt: &'a WorktreeRow,
        decision: TriageDecision,
    ) -> Result<Self> {
        let tip = prepare(env.git, env.root, row, wt)?;
        Ok(Freeing {
            row,
            wt,
            tip,
            decision,
        })
    }
}

/// `ARCHIVE`: the work stays at `refs/horch/archive/<name>` and on its
/// branch; the folder goes.
fn archive(store: &Store, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let f = Freeing::prepare(env, row, wt, TriageDecision::Archive)?;
    archive_at(store, env, &f, "")
}

fn archive_at(store: &Store, env: &Env<'_>, f: &Freeing<'_>, before: &str) -> Result<Done> {
    let (row, wt) = (f.row, f.wt);
    let r = refs::archive_ref(&row.name);
    refs::write(env.git, env.root, &r, &f.tip)?;
    must_be_safe(env.git, env.root, wt, &r, &f.tip)?;
    let bytes = remove_folder(store, env, f, &r)?;
    let branch = if wt.branch.is_empty() {
        String::new()
    } else {
        format!("; the branch {} stays", wt.branch)
    };
    Ok(Done {
        status: Some(WorktreeStatus::Archived),
        archive_ref: Some(r.clone()),
        bytes_freed: Some(bytes),
        text: format!(
            "{before}archived at {r} ({} freed){branch}. Restore: horch worktree restore {}",
            human_bytes(bytes),
            row.name
        ),
        queued: None,
    })
}

/// Whether `branch` is checked out in any worktree of the repository.
fn checked_out(git: &Git, root: &Path, branch: &str) -> bool {
    listed_worktrees(git, root)
        .iter()
        .any(|l| l.branch.as_deref() == Some(branch))
}

/// Whether `branch` is the default branch: `main`, `master`, or what
/// `origin/HEAD` names.
fn is_default(git: &Git, root: &Path, branch: &str) -> bool {
    if matches!(branch, "main" | "master") {
        return true;
    }
    git.try_run(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .filter(|o| o.ok)
    .is_some_and(|o| o.stdout.strip_prefix("origin/") == Some(branch))
}

/// Delete the worktree's branch after `DELETE` removed its folder. Only
/// when the trash ref, read back, holds the branch's commit: the branch is
/// then a 2nd name of work that the trash ref keeps.
fn delete_branch(
    git: &Git,
    root: &Path,
    wt: &WorktreeRow,
    trash: &str,
    tip: &str,
) -> Result<String> {
    let b = &wt.branch;
    if b.is_empty() || !git.branch_exists(root, b) {
        return Ok("no branch to delete".into());
    }
    if refs::read(git, root, trash).as_deref() != Some(tip) {
        bail!("{trash} does not hold {tip}; the branch {b} stays");
    }
    if git.rev(root, b)? != tip {
        bail!("the branch {b} moved during the delete; it stays");
    }
    if checked_out(git, root, b) || is_default(git, root, b) {
        bail!("the branch {b} is checked out or is the default branch; it stays");
    }
    git.run(root, &["update-ref", "-d", &format!("refs/heads/{b}"), tip])?;
    Ok(format!("the branch {b} is deleted"))
}

/// `DELETE`: the work goes to a dated trash ref, then the folder and the
/// branch go. `horch ledger gc` deletes the trash ref 30 days on.
fn delete(store: &Store, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let f = Freeing::prepare(env, row, wt, TriageDecision::Delete)?;
    let trash = refs::trash_ref(&row.name, crate::clock::now());
    refs::write(env.git, env.root, &trash, &f.tip)?;
    must_be_safe(env.git, env.root, wt, &trash, &f.tip)?;
    let bytes = remove_folder(store, env, &f, &trash)?;
    let branch = delete_branch(env.git, env.root, wt, &trash, &f.tip)?;
    Ok(Done {
        status: Some(WorktreeStatus::Trashed),
        archive_ref: Some(trash.clone()),
        bytes_freed: Some(bytes),
        text: format!(
            "trashed at {trash} ({} freed); {branch}. horch ledger gc deletes the ref after {} days",
            human_bytes(bytes),
            refs::TRASH_DAYS
        ),
        queued: None,
    })
}

/// Whether landing `head` (and the folder's uncommitted files) on
/// `dest_tip` conflicts. Read only: no ref, index or folder changes.
fn would_conflict(git: &Git, root: &Path, dir: &Path, dest_tip: &str, head: &str) -> Result<bool> {
    let merge = git.try_run(
        root,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            dest_tip,
            head,
        ],
    )?;
    if !merge.ok {
        return Ok(true);
    }
    let fork = git.run(root, &["merge-base", head, dest_tip])?;
    let changed = git.run(root, &["diff", "--name-only", &fork, dest_tip])?;
    let changed: Vec<&str> = changed.lines().collect();
    Ok(git
        .dirty(dir)?
        .iter()
        .filter_map(|l| l.get(3..))
        .any(|p| changed.contains(&p)))
}

const CONFLICTS: &str = "conflicts: choose FINISH";

/// `LAND`: rebase the work onto the branch the main checkout has checked
/// out. On the default branch it goes to a new branch `land/<name>`; on a
/// feature branch it fast-forwards that branch under the land lock.
fn land_decision(store: &mut Store, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let (git, root) = (env.git, env.root);
    let dir = Path::new(&wt.path);
    if !dir.exists() {
        bail!("the folder is gone; choose ARCHIVE or DELETE");
    }
    check_identity(git, wt)?;
    let dest = git
        .current_branch(root)
        .context("the main checkout is detached; check out the branch to land on")?;
    if dest == wt.branch {
        bail!("the main checkout is on {dest}, the worktree's own branch");
    }
    let dest_tip = git.rev(root, &dest)?;
    if would_conflict(git, root, dir, &dest_tip, &git.rev(dir, "HEAD")?)? {
        bail!("{CONFLICTS}");
    }
    commit_wip(git, wt, &row.name)?;
    let rebase = git.try_run(dir, &["rebase", "--quiet", &dest_tip])?;
    if !rebase.ok {
        let _ = git.try_run(dir, &["rebase", "--abort"]);
        bail!("{CONFLICTS}");
    }
    let new = git.rev(dir, "HEAD")?;
    if is_default(git, root, &dest) {
        land_aside(store, env, row, wt, &dest, &new)
    } else {
        land_on(store, env, row, wt, &dest, &new)
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// Remove the folder after a land; the work is on `on`.
fn remove_landed(store: &Store, env: &Env<'_>, wt: &WorktreeRow, on: &str) -> Result<String> {
    let o = land::remove_with(store, env.git, wt, on, None, env.jobs)?;
    if !o.status.is_through() {
        bail!("{}", o.note);
    }
    Ok(o.note)
}

/// The main checkout is on the default branch: leave it, and put the work
/// on `land/<name>` for the operator.
fn land_aside(
    store: &Store,
    env: &Env<'_>,
    row: &Row,
    wt: &WorktreeRow,
    dest: &str,
    new: &str,
) -> Result<Done> {
    let target = format!("land/{}", row.name);
    if env.git.branch_exists(env.root, &target) {
        bail!(
            "{target} exists already; the rebased branch {} stays",
            wt.branch
        );
    }
    env.git.run(env.root, &["branch", &target, new])?;
    let gone = remove_landed(store, env, wt, &target)?;
    Ok(Done {
        text: format!(
            "left on the new branch {target} at {}; {dest} did not move; {gone}",
            short(new)
        ),
        ..Default::default()
    })
}

/// The main checkout is on a feature branch: fast-forward it to the work.
fn land_on(
    store: &Store,
    env: &Env<'_>,
    row: &Row,
    wt: &WorktreeRow,
    dest: &str,
    new: &str,
) -> Result<Done> {
    let (git, root) = (env.git, env.root);
    {
        let holder = format!(
            "worktree triage of {} (PID {})",
            row.name,
            std::process::id()
        );
        let _lock = land::repo_land_lock(env.evidence_root, root, &holder)?;
        if git.current_branch(root).as_deref() != Some(dest) {
            bail!(
                "the main checkout left {dest} during the land; the rebased branch {} stays",
                wt.branch
            );
        }
        let ff = git.try_run(root, &["merge", "--ff-only", "--quiet", new])?;
        if !ff.ok {
            bail!(
                "the fast-forward of {dest} failed: {}; the rebased branch {} stays",
                ff.stderr,
                wt.branch
            );
        }
    }
    store.conn().execute(
        "UPDATE worktrees SET landed_commit = ?2, landed_at = ?3 WHERE worktree_id = ?1",
        params![wt.worktree_id, new, util::now()],
    )?;
    let gone = remove_landed(store, env, wt, dest)?;
    Ok(Done {
        text: format!("landed on {dest} at {}; {gone}", short(new)),
        ..Default::default()
    })
}

/// The first line of `text`, at most `max` characters.
fn first_line(text: &str, max: usize) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(max)
        .collect()
}

/// Write the brief of a `FINISH` task to `ai_docs/triage/<name>.md` (a free
/// name), and return its path from the root.
fn write_ticket(root: &Path, row: &Row, wt: &WorktreeRow, direction: &str) -> Result<String> {
    let dir = root.join("ai_docs").join("triage");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stem = row.name.replace('/', "-");
    let file = (1..)
        .map(|n| match n {
            1 => format!("{stem}.md"),
            n => format!("{stem}-{n}.md"),
        })
        .find(|f| !dir.join(f).exists())
        .context("no free ticket name")?;
    let body = format!(
        "# Finish {name}\n\n{direction}\n\n## Worktree\n\n- Path: {path}\n- Branch: {branch}\n- Base: {base}\n- Ahead {ahead}, behind {behind}\n- Why parked: {why}\n",
        name = row.name,
        path = wt.path,
        branch = wt.branch,
        base = row.base,
        ahead = row.ahead,
        behind = row.behind,
        why = row.why_parked.as_deref().unwrap_or("unknown"),
    );
    std::fs::write(dir.join(&file), body)?;
    Ok(format!("ai_docs/triage/{file}"))
}

/// `FINISH`: queue a task with the direction as its brief; it inherits the
/// worktree, as `ledger reopen` does. A worktree whose task is parked
/// unlanded resumes that task instead (trace-fixes D16).
fn finish_decision(
    store: &mut Store,
    env: &Env<'_>,
    row: &Row,
    wt: &WorktreeRow,
    direction: &str,
) -> Result<Done> {
    let ticket = match parked_task(store.conn(), wt)? {
        Some(_) => None,
        None => {
            runs::resolve(store.conn(), env.root, None, None)
                .context("FINISH needs an active run; start one with horch fleet")?;
            Some(write_ticket(env.root, row, wt, direction)?)
        }
    };
    store.immediate(|tx| {
        finish_in(
            tx,
            env.root,
            env.primary,
            row,
            wt,
            direction,
            ticket.as_deref(),
        )
    })
}

/// The rows of `FINISH`, in `tx`: a parked task resumes; else 1 task with
/// the brief `ticket` inherits the worktree. A task to queue with no
/// `ticket` is refused.
fn finish_in(
    tx: &Connection,
    root: &Path,
    primary: &str,
    row: &Row,
    wt: &WorktreeRow,
    direction: &str,
    ticket: Option<&str>,
) -> Result<Done> {
    if let Some(parked) = parked_task(tx, wt)? {
        return resume_parked(tx, wt, &parked, direction);
    }
    let (run, _) = runs::resolve(tx, root, None, None)
        .context("FINISH needs an active run; start one with horch fleet")?;
    let ticket = ticket.context("FINISH needs its brief, and no ticket file is given")?;
    let title = format!("Finish {}: {}", row.name, first_line(direction, 60));
    let new = NewTask::work(&title, ticket, primary);
    let task = tasks::add(tx, &run, &new, Actor::Operator)?;
    tx.execute(
        &format!(
            "UPDATE worktrees SET task_id = ?2, worker_id = NULL, status = {open}, reason = NULL
             WHERE worktree_id = ?1",
            open = word(WorktreeStatus::Open)
        ),
        [&wt.worktree_id, &task.task_id],
    )?;
    Ok(Done {
        text: format!(
            "{} queued for {primary} with the brief {ticket}; it inherits {}",
            task.short(),
            wt.path
        ),
        queued: Some(Queued {
            run_id: run.run_id.clone(),
            task_id: task.task_id.clone(),
            task: task.short(),
        }),
        ..Default::default()
    })
}

/// The decisions that [`apply_in_ledger`] applies: they write rows only.
pub const LEDGER_ONLY: [TriageDecision; 2] = [TriageDecision::Keep, TriageDecision::Finish];

/// Apply the decision `d` of `row` with rows only, in `tx`, as [`apply`]
/// does: `KEEP`, or `FINISH` with `ticket`, the brief that the command
/// wrote. A decision that fails is recorded as failed, as in [`apply`]. A
/// decision that is not in [`LEDGER_ONLY`] is an error, and writes nothing.
pub fn apply_in_ledger(
    tx: &Connection,
    root: &Path,
    primary: &str,
    row: &Row,
    d: &Decided,
    ticket: Option<&str>,
) -> Result<Applied> {
    if !LEDGER_ONLY.contains(&d.decision) {
        bail!("{} needs git; it is not applied with rows only", d.decision);
    }
    let wt = adopt(tx, root, row)?;
    let result = match d.decision {
        TriageDecision::Finish => finish_in(tx, root, primary, row, &wt, direction(d), ticket),
        _ => Ok(Done {
            text: "kept; it comes back at the next triage".into(),
            ..Default::default()
        }),
    };
    let (ok, text, queued) = match result {
        Ok(done) => {
            record(tx, row, &wt, d.decision, &done)?;
            (true, done.text, done.queued)
        }
        Err(e) => {
            let why = format!("{e:#}");
            failed(tx, row, &wt, d.decision, &why)?;
            (false, why, None)
        }
    };
    Ok(Applied {
        id: d.id.clone(),
        name: d.name.clone(),
        decision: d.decision,
        ok,
        text,
        queued,
    })
}

/// The task of `wt` when it is parked unlanded.
fn parked_task(conn: &Connection, wt: &WorktreeRow) -> Result<Option<tasks::Task>> {
    let Some(id) = &wt.task_id else {
        return Ok(None);
    };
    Ok(Some(tasks::get(conn, id)?).filter(|t| t.is_parked()))
}

/// `FINISH` on a parked task: requeue it on its own worktree, with the
/// direction as the requeue reason. Its dependents keep waiting for it.
fn resume_parked(
    tx: &Connection,
    wt: &WorktreeRow,
    parked: &tasks::Task,
    direction: &str,
) -> Result<Done> {
    let why = match direction.trim() {
        "" => "worktree triage FINISH".to_string(),
        d => format!("worktree triage FINISH: {d}"),
    };
    let task = tasks::requeue_by_operator(tx, parked, &why)?;
    Ok(Done {
        text: format!(
            "{} resumed on {}; it keeps its ticket",
            task.short(),
            wt.path
        ),
        queued: Some(Queued {
            run_id: task.run_id.clone(),
            task_id: task.task_id.clone(),
            task: task.short(),
        }),
        ..Default::default()
    })
}

/// `PR`: push the branch, open a pull request with the direction as its
/// title and body, then archive the folder.
fn pr(store: &Store, env: &Env<'_>, row: &Row, wt: &WorktreeRow, direction: &str) -> Result<Done> {
    let (Some(gh), Some(remote)) = (env.gh, env.remote) else {
        bail!("PR is not offered here: gh or a git remote is missing");
    };
    let f = Freeing::prepare(env, row, wt, TriageDecision::Pr)?;
    let b = &wt.branch;
    let push = env
        .git
        .try_network(env.root, &["push", "--quiet", remote, &format!("{b}:{b}")])?;
    if !push.ok {
        bail!("git push {remote} {b}:{b} failed: {}", push.stderr);
    }
    let mut create = start::command(gh);
    create
        .current_dir(env.root)
        .args(["pr", "create", "--head", b, "--title"])
        .arg(first_line(direction, 200))
        .arg("--body")
        .arg(direction);
    scrub_child_env(&mut create);
    let out = create
        .output()
        .with_context(|| format!("running {}", gh.display()))?;
    if !out.status.success() {
        bail!(
            "gh pr create failed: {}; {b} is pushed to {remote}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    archive_at(
        store,
        env,
        &f,
        &format!("pushed {b} to {remote}; opened {url}; "),
    )
}

/// The newest row of the project at `root` that triage settled on the ref
/// `r`. Another project can archive the same name, so the row must be of
/// this project.
pub(crate) fn archived_row(conn: &Connection, root: &Path, r: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT worktree_id, repo_root FROM worktrees WHERE archive_ref = ?1
         ORDER BY decided_at DESC, rowid DESC",
    )?;
    let rows = stmt.query_map([r], |x| {
        Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?))
    })?;
    let root = super::canonical(&root.to_string_lossy());
    for row in rows {
        let (id, repo_root) = row?;
        if super::canonical(&repo_root) == root {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// `horch worktree restore <name>`: make the folder again from the
/// archive ref (else the newest trash ref), through a `worktree_add` job
/// (`jobs` runs it). The archived row stays as history; the restored tree
/// has a new row, `open` and undecided, so the next triage shows it.
pub fn restore(
    store: &Store,
    git: &Git,
    jobs: &dyn RunJob,
    root: &Path,
    name: &str,
) -> Result<String> {
    let r = refs::restorable(git, root, name)?
        .with_context(|| format!("no refs/horch/archive/{name} and no trash ref of {name}"))?;
    let commit = git.rev(root, &r)?;
    let path = root.join(".worktrees").join(name);
    if path.exists() {
        bail!("{} exists already", path.display());
    }
    let row = archived_row(store.conn(), root, &r)?;
    let old = match &row {
        Some(id) => trees::get(store.conn(), id)?,
        None => None,
    };
    let branch = old
        .as_ref()
        .map(|o| o.branch.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| format!("restored/{name}"));
    let path_text = path.to_string_lossy().into_owned();
    if git.branch_exists(root, &branch) && git.rev(root, &branch)? != commit {
        bail!("the branch {branch} moved since {r}; check it out by hand: git worktree add {path_text} {branch}");
    }
    // The job attaches the branch at `commit`, or makes it there.
    let new = NewWorktree {
        worktree_id: util::new_id(),
        task_id: old.as_ref().and_then(|o| o.task_id.clone()),
        worker_id: None,
        repo_root: root.to_string_lossy().into_owned(),
        path: path_text.clone(),
        branch: branch.clone(),
        base: old
            .as_ref()
            .map_or_else(|| branch.clone(), |o| o.base.clone()),
        base_commit: Some(commit.clone()),
        status: WorktreeStatus::Open,
        mode: old.and_then(|o| o.mode),
    };
    // No task and no worker: the request never attaches another tree, and
    // the new row has no owner.
    let lease = Lease {
        task_id: None,
        worker_id: "",
        attempt: None,
        from: None,
        new: Some(&new),
    };
    let id = finish::lease_tree(store, jobs, &lease)?;
    let mut conn = open(store)?;
    let tx = immediate(&mut conn)?;
    create::give_to_operator(&tx, &id)?;
    note_restored(&tx, &id, &r)?;
    tx.commit()?;
    Ok(format!(
        "restored {path_text} on {branch} at {} from {r}",
        short(&commit)
    ))
}

/// The reason of the tree `id` that a restore made from the ref `r`.
pub(crate) fn note_restored(conn: &Connection, id: &str, r: &str) -> Result<()> {
    conn.execute(
        "UPDATE worktrees SET reason = ?2 WHERE worktree_id = ?1",
        params![id, format!("restored from {r}")],
    )?;
    Ok(())
}
