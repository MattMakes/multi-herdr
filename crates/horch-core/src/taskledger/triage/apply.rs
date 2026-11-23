use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use horch_platform::sandbox::sandboxed_hiding;
use horch_platform::start;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::reply::Decided;
use super::{has_no_branch, human_bytes, listed_worktrees, refs, Row};
use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{
    self, outcome, JobCtx, JobKey, Queued as JobQueued, Ran,
};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, Outcome};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::integration::adapters::executors::{check_text, clip, facts_of, FACT_TEXT_MAX};
use crate::fleetd::worktree_custody::adapters::executors as custody_executors;
use crate::fleetd::worktree_custody::adapters::git::{self as custody_git, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::remove_git::{self, CliRemoval, Owner};
use crate::fleetd::worktree_custody::adapters::rescue_git::{self, Rescue};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeState,
};
use crate::fleetd::worktree_custody::application::create::{self, Lease, RunJob};
use crate::fleetd::worktree_custody::application::remove;
use crate::fleetd::worktree_custody::domain::worktree::{settle_refusal, Folder};
use crate::fleetd::worktree_custody::domain::{removal, rescue};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::process::scrub_child_env;
use crate::taskledger::finish::{self, land, WorktreeRow};
use crate::taskledger::git::{dir_bytes, Git};
use crate::taskledger::model::{Actor, EventKind, FindingStatus, TriageDecision, WorktreeStatus};
use crate::taskledger::sql_words::word;
use crate::taskledger::store::{optional, Store};
use crate::taskledger::tasks::{self, NewTask};
use crate::taskledger::util::{self, About};
use crate::taskledger::{import, runs};

#[derive(Clone)]
pub struct Env<'a> {
    pub git: &'a Git,
    pub root: &'a Path,
    pub evidence_root: &'a Path,
    pub gh: Option<&'a Path>,
    pub remote: Option<&'a str>,
    pub primary: &'a str,
    pub jobs: &'a dyn RunJob,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub run_id: String,
    pub task_id: String,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub id: String,
    pub name: String,
    pub decision: TriageDecision,
    pub ok: bool,
    pub text: String,
    pub queued: Option<Queued>,
}

impl Applied {
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

#[derive(Debug, Default)]
struct Done {
    status: Option<WorktreeStatus>,
    archive_ref: Option<String>,
    bytes_freed: Option<u64>,
    text: String,
    queued: Option<Queued>,
}

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
        TriageDecision::Archive => archive(&mut Direct(store), env, row, &wt),
        TriageDecision::Delete => delete(&mut Direct(store), env, row, &wt),
        TriageDecision::Land => land_decision(&mut Direct(store), env, row, &wt),
        TriageDecision::Finish => finish_decision(store, env, row, &wt, direction(d)),
        TriageDecision::Pr => pr(&mut Direct(store), env, row, &wt, direction(d)),
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

const TRIAGE: Cause<'static> = Cause {
    actor: Actor::Operator,
    text: "triage",
};

const FOUND: &str = "found on disk by worktree triage";

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

fn check_identity(git: &Git, wt: &WorktreeRow) -> Result<()> {
    let dir = Path::new(&wt.path);
    if !dir.exists() {
        return Ok(());
    }
    match git.current_branch(dir) {
        Some(_) if has_no_branch(&wt.branch) => Ok(()),
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

fn tip(git: &Git, root: &Path, wt: &WorktreeRow) -> Result<String> {
    let dir = Path::new(&wt.path);
    if dir.exists() {
        return git.rev(dir, "HEAD");
    }
    git.rev(root, &wt.branch)
}

fn prepare(git: &Git, root: &Path, row: &Row, wt: &WorktreeRow) -> Result<String> {
    check_identity(git, wt)?;
    commit_wip(git, wt, &row.name)?;
    tip(git, root, wt)
}

fn must_be_safe(git: &Git, root: &Path, wt: &WorktreeRow, holder: &str, tip: &str) -> Result<()> {
    let mut w = wt.clone();
    if has_no_branch(&w.branch) {
        w.branch = tip.to_string();
    }
    if let Some(why) = land::unsafe_to_remove(git, root, &w, holder, None)? {
        bail!("the worktree stays: {why}");
    }
    Ok(())
}

fn remove_folder(store: &mut Store, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<u64> {
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
    let conn = store.conn_mut();
    let row = orphan(&ctx, conn, &wt.worktree_id)?;
    create::rescue_now(conn, &ctx, env.jobs, &row)?;
    remove_now(&ctx, conn, env.jobs, &wt.worktree_id)?;
    if dir.exists() {
        bail!("{} still exists after its worktree_remove job", wt.path);
    }
    Ok(bytes)
}

fn immediate(conn: &mut Connection) -> Result<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

fn settle_gone(store: &mut Store, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<()> {
    let wt = f.wt;
    let folder = if Path::new(&wt.path).exists() {
        Folder::Present
    } else {
        Folder::Missing
    };
    let holds_tip = if has_no_branch(&wt.branch) {
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
    let conn = store.conn_mut();
    let row = orphan(&ctx, conn, &wt.worktree_id)?;
    let tx = immediate(conn)?;
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

fn path_key(path: &str) -> PathBuf {
    let p = Path::new(path);
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) => super::canonical(&dir.to_string_lossy()).join(name),
        _ => p.to_path_buf(),
    }
}

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

struct Freeing<'a> {
    row: &'a Row,
    wt: &'a WorktreeRow,
    tip: String,
    decision: TriageDecision,
}

impl<'a> Freeing<'a> {
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

fn archive(fx: &mut dyn Folders, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let f = Freeing::prepare(env, row, wt, TriageDecision::Archive)?;
    archive_at(fx, env, &f, "")
}

fn archive_at(fx: &mut dyn Folders, env: &Env<'_>, f: &Freeing<'_>, before: &str) -> Result<Done> {
    let (row, wt) = (f.row, f.wt);
    let r = refs::archive_ref(&row.name);
    refs::write(env.git, env.root, &r, &f.tip)?;
    must_be_safe(env.git, env.root, wt, &r, &f.tip)?;
    let bytes = fx.free(env, f, &r)?;
    let branch = if has_no_branch(&wt.branch) {
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

fn checked_out(git: &Git, root: &Path, branch: &str) -> bool {
    listed_worktrees(git, root)
        .iter()
        .any(|l| l.branch.as_deref() == Some(branch))
}

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

fn delete_branch(
    git: &Git,
    root: &Path,
    wt: &WorktreeRow,
    trash: &str,
    tip: &str,
) -> Result<String> {
    let b = &wt.branch;
    if has_no_branch(b) || !git.branch_exists(root, b) {
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

fn delete(fx: &mut dyn Folders, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let f = Freeing::prepare(env, row, wt, TriageDecision::Delete)?;
    let trash = refs::trash_ref(&row.name, crate::clock::now());
    refs::write(env.git, env.root, &trash, &f.tip)?;
    must_be_safe(env.git, env.root, wt, &trash, &f.tip)?;
    let bytes = fx.free(env, &f, &trash)?;
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

fn land_decision(fx: &mut dyn Folders, env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
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
        land_aside(fx, env, row, wt, &dest, &new)
    } else {
        land_on(fx, env, row, wt, &dest, &new)
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn remove_landed(store: &Store, env: &Env<'_>, wt: &WorktreeRow, on: &str) -> Result<String> {
    let o = land::remove_with(store, env.git, wt, on, None, env.jobs)?;
    if !o.status.is_through() {
        bail!("{}", o.note);
    }
    Ok(o.note)
}

trait Folders {
    fn free(&mut self, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<u64>;

    fn free_landed(
        &mut self,
        env: &Env<'_>,
        wt: &WorktreeRow,
        on: &str,
        landed: Option<&str>,
    ) -> Result<String>;
}

struct Direct<'s>(&'s mut Store);

impl Folders for Direct<'_> {
    fn free(&mut self, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<u64> {
        remove_folder(self.0, env, f, holder)
    }

    fn free_landed(
        &mut self,
        env: &Env<'_>,
        wt: &WorktreeRow,
        on: &str,
        landed: Option<&str>,
    ) -> Result<String> {
        if let Some(new) = landed {
            self.0.conn().execute(
                "UPDATE worktrees SET landed_commit = ?2, landed_at = ?3 WHERE worktree_id = ?1",
                params![wt.worktree_id, new, util::now()],
            )?;
        }
        remove_landed(self.0, env, wt, on)
    }
}

fn land_aside(
    fx: &mut dyn Folders,
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
    let gone = fx.free_landed(env, wt, &target, None)?;
    Ok(Done {
        text: format!(
            "left on the new branch {target} at {}; {dest} did not move; {gone}",
            short(new)
        ),
        ..Default::default()
    })
}

fn land_on(
    fx: &mut dyn Folders,
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
    let gone = fx.free_landed(env, wt, dest, Some(new))?;
    Ok(Done {
        text: format!("landed on {dest} at {}; {gone}", short(new)),
        ..Default::default()
    })
}

fn first_line(text: &str, max: usize) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(max)
        .collect()
}

fn write_ticket(root: &Path, row: &Row, wt: (&str, &str), direction: &str) -> Result<String> {
    let (path, branch) = wt;
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
        base = row.base,
        ahead = row.ahead,
        behind = row.behind,
        why = row.why_parked.as_deref().unwrap_or("unknown"),
    );
    std::fs::write(dir.join(&file), body)?;
    Ok(format!("ai_docs/triage/{file}"))
}

pub fn finish_brief(
    conn: &Connection,
    root: &Path,
    row: &Row,
    direction: &str,
) -> Result<Option<String>> {
    let (path, branch) = match &row.worktree_id {
        Some(id) => {
            let wt = finish::worktree(conn, id)?;
            if parked_task(conn, &wt)?.is_some() {
                return Ok(None);
            }
            (wt.path, wt.branch)
        }
        None => (row.path.clone(), row.branch.clone().unwrap_or_default()),
    };
    runs::resolve(conn, root, None, None)
        .context("FINISH needs an active run; start one with horch fleet")?;
    write_ticket(root, row, (&path, &branch), direction).map(Some)
}

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
            Some(write_ticket(
                env.root,
                row,
                (&wt.path, &wt.branch),
                direction,
            )?)
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

pub const LEDGER_ONLY: [TriageDecision; 2] = [TriageDecision::Keep, TriageDecision::Finish];

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

fn parked_task(conn: &Connection, wt: &WorktreeRow) -> Result<Option<tasks::Task>> {
    let Some(id) = &wt.task_id else {
        return Ok(None);
    };
    Ok(Some(tasks::get(conn, id)?).filter(|t| t.is_parked()))
}

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

fn pr(
    fx: &mut dyn Folders,
    env: &Env<'_>,
    row: &Row,
    wt: &WorktreeRow,
    direction: &str,
) -> Result<Done> {
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
        fx,
        env,
        &f,
        &format!("pushed {b} to {remote}; opened {url}; "),
    )
}

pub fn archived_row(conn: &Connection, root: &Path, r: &str) -> Result<Option<String>> {
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
    let lease = Lease {
        task_id: None,
        worker_id: "",
        attempt: None,
        from: None,
        new: Some(&new),
    };
    let id = finish::lease_tree(store, jobs, &lease)?;
    let tx = Transaction::new_unchecked(store.conn(), TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")?;
    create::give_to_operator(&tx, &id)?;
    note_restored(&tx, &id, &r)?;
    tx.commit()?;
    Ok(format!(
        "restored {path_text} on {branch} at {} from {r}",
        short(&commit)
    ))
}

pub(crate) fn note_restored(conn: &Connection, id: &str, r: &str) -> Result<()> {
    conn.execute(
        "UPDATE worktrees SET reason = ?2 WHERE worktree_id = ?1",
        params![id, format!("restored from {r}")],
    )?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobInput {
    root: PathBuf,
    items: Vec<JobItem>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobItem {
    worktree_id: String,
    row: Row,
    decision: String,
    direction: Option<String>,
}

pub const JOB_DECISIONS: [TriageDecision; 4] = [
    TriageDecision::Archive,
    TriageDecision::Delete,
    TriageDecision::Land,
    TriageDecision::Pr,
];

pub fn queue_job(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    root: &Path,
    items: &[(Row, TriageDecision, Option<String>)],
) -> Result<i64> {
    let mut input = JobInput {
        root: root.to_path_buf(),
        items: Vec::new(),
    };
    for (row, decision, direction) in items {
        if !JOB_DECISIONS.contains(decision) {
            bail!(
                "{decision} of {} is not a decision of a triage_apply job",
                row.id
            );
        }
        let wt = adopt(tx, root, row)?;
        input.items.push(JobItem {
            worktree_id: wt.worktree_id,
            row: row.clone(),
            decision: decision.as_str().to_string(),
            direction: direction.clone(),
        });
    }
    let subject = root.to_string_lossy();
    let input_key = serde_json::to_string(&input)?;
    let key = JobKey {
        kind: JobKind::TriageApply,
        subject: &subject,
        input_key: &input_key,
    };
    match jobs::queue(ctx, tx, &key, Some(&subject), create::fleet_clock(tx)?)? {
        JobQueued::New(id) => Ok(id),
        JobQueued::Active(r) => bail!(
            "the triage_apply job {} of {subject} is {}; send the reply again after it ends",
            r.id,
            r.state.as_str()
        ),
        JobQueued::Reused(r) | JobQueued::Held(r) => {
            bail!(
                "the triage_apply job {} of {subject} gives no new run",
                r.id
            )
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionFacts {
    worktree_id: String,
    ok: bool,
    text: String,
    archive_ref: Option<String>,
    bytes_freed: Option<u64>,
    landed_commit: Option<String>,
    rescue_ref: Option<String>,
    removed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobFacts {
    decisions: Vec<DecisionFacts>,
}

struct NoNestedJob;

impl RunJob for NoNestedJob {
    fn run_job(&self, _: &JobCtx<'_>, _: &mut Connection, job: i64) -> Result<JobRow> {
        bail!("a triage_apply run starts no other job (job {job})")
    }
}

pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    let input: JobInput =
        serde_json::from_str(&row.input_key).context("the input of the triage_apply job")?;
    let fleet_root = custody_executors::fleet_root(conn)?;
    let git_bin = git_bin(&BinOverrides::from_env(&ProcessEnv));
    let custody = CliWorktreeGit::new(&git_bin, &fleet_root).with_sandbox_host(sandboxed_hiding);
    if let Some(ran) = custody_git::no_sandbox(&custody) {
        return Ok(ran);
    }
    let git = Git::new(&git_bin);
    let root = input.root.as_path();
    let gh = ProcessEnv
        .var_os("HORCH_GH_BIN")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("gh"), PathBuf::from);
    let wants_pr = input.items.iter().any(|i| i.decision == "PR");
    let remote = wants_pr.then(|| pr_remote(&git, root, &gh)).flatten();
    let evidence = conn
        .path()
        .and_then(|p| Path::new(p).parent())
        .map_or_else(|| PathBuf::from("evidence"), |p| p.join("evidence"));
    let env = Env {
        git: &git,
        root,
        evidence_root: &evidence,
        gh: remote.is_some().then_some(gh.as_path()),
        remote: remote.as_deref(),
        primary: "",
        jobs: &NoNestedJob,
    };
    let mut facts = JobFacts::default();
    let mut lines = Vec::new();
    for item in &input.items {
        let mut fx = InJob::new(conn, &fleet_root, &custody);
        let fact = run_item(&mut fx, &env, item);
        lines.push(format!(
            "{} {} {}{}: {}",
            item.row.id,
            item.row.name,
            item.decision,
            if fact.ok { "" } else { " FAILED" },
            fact.text
        ));
        facts.decisions.push(fact);
    }
    Ok(Ran {
        outcome: Outcome {
            passed: true,
            verdict: true,
            text: clip(lines.join("\n")),
        },
        data: Some(serde_json::to_value(&facts)?),
    })
}

fn run_item(fx: &mut InJob<'_>, env: &Env<'_>, item: &JobItem) -> DecisionFacts {
    let done = finish::worktree(fx.conn, &item.worktree_id).and_then(|wt| {
        let direction = item.direction.as_deref().unwrap_or("");
        match TriageDecision::parse(&item.decision)? {
            TriageDecision::Archive => archive(fx, env, &item.row, &wt),
            TriageDecision::Delete => delete(fx, env, &item.row, &wt),
            TriageDecision::Land => land_decision(fx, env, &item.row, &wt),
            TriageDecision::Pr => pr(fx, env, &item.row, &wt, direction),
            d => bail!("{d} is not a decision of a triage_apply job"),
        }
    });
    match done {
        Ok(done) => DecisionFacts {
            worktree_id: item.worktree_id.clone(),
            ok: true,
            text: clip(done.text),
            archive_ref: done.archive_ref,
            bytes_freed: done.bytes_freed.or(fx.bytes),
            landed_commit: fx.landed.take(),
            rescue_ref: fx.rescue_ref.take(),
            removed: fx.removed,
        },
        Err(e) => DecisionFacts {
            worktree_id: item.worktree_id.clone(),
            text: clip(format!("{e:#}")),
            rescue_ref: fx.rescue_ref.take(),
            removed: fx.removed,
            ..DecisionFacts::default()
        },
    }
}

struct InJob<'a> {
    conn: &'a Connection,
    fleet_root: &'a Path,
    custody: &'a CliWorktreeGit,
    rescue_ref: Option<String>,
    bytes: Option<u64>,
    landed: Option<String>,
    removed: bool,
}

impl<'a> InJob<'a> {
    fn new(conn: &'a Connection, fleet_root: &'a Path, custody: &'a CliWorktreeGit) -> Self {
        InJob {
            conn,
            fleet_root,
            custody,
            rescue_ref: None,
            bytes: None,
            landed: None,
            removed: false,
        }
    }

    fn rescue_and_remove(&mut self, wt: &WorktreeRow) -> Result<u64> {
        let id = wt.worktree_id.as_str();
        let tree = trees::get(self.conn, id)?.with_context(|| format!("no worktree {id}"))?;
        let dir = PathBuf::from(&tree.path);
        let bytes = dir_bytes(&dir);
        let git = self
            .custody
            .clone()
            .reading(&[PathBuf::from(&tree.repo_root)]);
        let saved = rescue_git::rescue(&git, self.fleet_root, &tree);
        if !saved.outcome.passed {
            rescue_git::drop_index_folder(self.fleet_root, &tree);
            bail!("the worktree stays: {}", saved.outcome.text);
        }
        let rescue: Rescue = serde_json::from_value(saved.data.unwrap_or_default())
            .context("the rescue gives no rescue ref")?;
        self.rescue_ref = Some(rescue.rescue_ref.clone());
        let held = |mut t: trees::WorktreeRow| {
            t.state = WorktreeState::Rescued;
            t.rescue_ref = Some(rescue.rescue_ref.clone());
            t.owner_attempt = None;
            t
        };
        let row = held(tree.clone());
        let quarantine = removal::quarantine_path(&dir, id);
        let writer = custody_git::tree_writer(git, &row)?.writing(&[quarantine]);
        let gw = CliRemoval::new(writer, self.fleet_root.to_path_buf());
        let conn = self.conn;
        let ledger = |id: &str| Ok(trees::get(conn, id)?.map(held));
        let gone = remove_git::remove(&gw, &row, None, &Owner::NoAttempt, &ledger, true);
        rescue_git::drop_index_folder(self.fleet_root, &tree);
        if !gone.outcome.passed {
            bail!("{}", gone.outcome.text);
        }
        self.removed = true;
        self.bytes = Some(bytes);
        Ok(bytes)
    }
}

impl Folders for InJob<'_> {
    fn free(&mut self, env: &Env<'_>, f: &Freeing<'_>, holder: &str) -> Result<u64> {
        let wt = f.wt;
        if Path::new(&wt.path).exists() {
            return self.rescue_and_remove(wt);
        }
        let holds_tip = if has_no_branch(&wt.branch) {
            None
        } else {
            let tip = env.git.rev(env.root, &wt.branch).ok();
            Some(tip.is_some() && refs::read(env.git, env.root, holder) == tip)
        };
        let listed = git_lists(env.git, env.root, &wt.path)?;
        if let Some(why) = settle_refusal(Folder::Missing, listed, holds_tip) {
            bail!("the row stays: {why}");
        }
        self.removed = true;
        Ok(0)
    }

    fn free_landed(
        &mut self,
        env: &Env<'_>,
        wt: &WorktreeRow,
        on: &str,
        landed: Option<&str>,
    ) -> Result<String> {
        if let Some(why) = land::unsafe_to_remove(env.git, env.root, wt, on, None)? {
            bail!("the worktree stays: {why}");
        }
        self.landed = landed.map(str::to_string);
        let bytes = self.rescue_and_remove(wt)?;
        Ok(format!(
            "{} is gone ({} freed)",
            wt.path,
            human_bytes(bytes)
        ))
    }
}

pub fn ingest(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow, ran: &Ran) -> Result<()> {
    let Some(facts) = facts_of::<JobFacts>(ran.data.as_ref())? else {
        return Ok(());
    };
    let input: JobInput = serde_json::from_str(&job.input_key)?;
    if facts.decisions.len() != input.items.len() {
        bail!(
            "the result file has {} decisions; the job has {}",
            facts.decisions.len(),
            input.items.len()
        );
    }
    for (item, fact) in input.items.iter().zip(&facts.decisions) {
        let decision = TriageDecision::parse(&item.decision)?;
        let wt = finish::worktree(tx, &item.worktree_id)?;
        tx.execute_batch("SAVEPOINT triage_decision")?;
        match ingest_one(ctx, tx, item, decision, &wt, fact) {
            Ok(()) => tx.execute_batch("RELEASE triage_decision")?,
            Err(why) => {
                tx.execute_batch("ROLLBACK TO triage_decision; RELEASE triage_decision")?;
                let why = format!("fleetd refused the facts of this decision: {why:#}");
                failed(tx, &item.row, &wt, decision, &why)?;
            }
        }
    }
    Ok(())
}

fn ingest_one(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    item: &JobItem,
    decision: TriageDecision,
    wt: &WorktreeRow,
    fact: &DecisionFacts,
) -> Result<()> {
    check_fact(item, decision, fact)?;
    let id = wt.worktree_id.as_str();
    if !fact.ok {
        match (fact.removed, fact.rescue_ref.as_deref()) {
            (true, r) => settle_removed(ctx, tx, id, r)?,
            (false, Some(r)) => settle_rescued(ctx, tx, id, r)?,
            (false, None) => {}
        }
        return failed(tx, &item.row, wt, decision, &fact.text);
    }
    if let Some(c) = &fact.landed_commit {
        tx.execute(
            "UPDATE worktrees SET landed_commit = ?2, landed_at = ?3 WHERE worktree_id = ?1",
            params![id, c, util::now()],
        )?;
    }
    settle_removed(ctx, tx, id, fact.rescue_ref.as_deref())?;
    let done = Done {
        status: Some(match decision {
            TriageDecision::Delete => WorktreeStatus::Trashed,
            TriageDecision::Land => WorktreeStatus::Cleaned,
            _ => WorktreeStatus::Archived,
        }),
        archive_ref: fact.archive_ref.clone(),
        bytes_freed: fact.bytes_freed,
        text: fact.text.clone(),
        queued: None,
    };
    record(tx, &item.row, wt, decision, &done)
}

fn check_fact(item: &JobItem, decision: TriageDecision, f: &DecisionFacts) -> Result<()> {
    if f.worktree_id != item.worktree_id {
        bail!(
            "the result file names worktree {}; the job has {}",
            f.worktree_id,
            item.worktree_id
        );
    }
    check_text("text", &f.text, FACT_TEXT_MAX)?;
    if let Some(r) = &f.rescue_ref {
        let folder = rescue::ref_folder(&item.worktree_id);
        let n = r.strip_prefix(&folder).unwrap_or("");
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            bail!("the rescue ref {r:?} is not a rescue ref of its worktree");
        }
    }
    if !f.ok {
        return Ok(());
    }
    if !f.removed {
        bail!("{decision} of {} passed and its folder stays", item.row.id);
    }
    let archive_ok = match (decision, f.archive_ref.as_deref()) {
        (TriageDecision::Archive | TriageDecision::Pr, Some(r)) => {
            r == refs::archive_ref(&item.row.name)
        }
        (TriageDecision::Delete, Some(r)) => r.strip_prefix(refs::TRASH).is_some_and(|rest| {
            !rest.is_empty() && rest.len() < 512 && !rest.contains(['\0', ' ', '\n', '\t'])
        }),
        (TriageDecision::Land, None) => true,
        _ => false,
    };
    if !archive_ok {
        bail!("the ref {:?} is not a ref of {decision}", f.archive_ref);
    }
    if f.landed_commit.is_some() && decision != TriageDecision::Land {
        bail!("{decision} gives a landed commit");
    }
    if let Some(c) = &f.landed_commit {
        let hex = c
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !(hex && matches!(c.len(), 40 | 64)) {
            bail!("the landed commit {c:?} is not an object id");
        }
    }
    if f.bytes_freed.is_some_and(|b| b > i64::MAX as u64) {
        bail!("the bytes freed are out of range");
    }
    Ok(())
}

fn settle_orphaned(ctx: &JobCtx<'_>, tx: &Connection, id: &str) -> Result<()> {
    let row = trees::get(tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    let keep = [
        WorktreeState::Orphaned,
        WorktreeState::Rescued,
        WorktreeState::Landing,
    ];
    if !keep.contains(&row.state) {
        door(ctx, tx, id, "orphaned", TRIAGE)?;
        trees::set_owner(tx, id, None)?;
    }
    Ok(())
}

fn settle_rescued(ctx: &JobCtx<'_>, tx: &Connection, id: &str, rescue_ref: &str) -> Result<()> {
    settle_orphaned(ctx, tx, id)?;
    trees::set_rescue_ref(tx, id, rescue_ref)?;
    let cause = Cause {
        actor: Actor::Operator,
        text: "triage: its rescue ref is read back and verified",
    };
    door(ctx, tx, id, "rescued", cause)
}

fn settle_removed(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    id: &str,
    rescue_ref: Option<&str>,
) -> Result<()> {
    match rescue_ref {
        Some(r) => settle_rescued(ctx, tx, id, r)?,
        None => settle_orphaned(ctx, tx, id)?,
    }
    let cause = Cause {
        actor: Actor::Operator,
        text: "triage: its triage_apply job removed the folder",
    };
    door(ctx, tx, id, "removed", cause)?;
    tx.execute(
        "UPDATE worktrees SET cleaned_at = ?2 WHERE worktree_id = ?1",
        params![id, util::now()],
    )?;
    Ok(())
}

fn door(ctx: &JobCtx<'_>, tx: &Connection, id: &str, to: &str, cause: Cause<'_>) -> Result<()> {
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        to,
        cause,
        ctx.needs_you,
    )
    .map(|_| ())
    .map_err(|e| anyhow::anyhow!("worktree {id} cannot move to {to}: {e}"))
}
