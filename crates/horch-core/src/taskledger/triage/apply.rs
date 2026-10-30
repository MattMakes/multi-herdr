//! Applying the decisions of a triage reply (#308, section 3).
//!
//! Each decision runs on its own, records 1 event and sets the row's
//! `decision` and `decided_at`. The rules of section 5 hold for every one:
//! git is never forced; before a folder is removed, `unsafe_to_remove` (or
//! an archive or trash ref that holds the tip) proves that no commit and no
//! uncommitted file is lost; a failed step deletes nothing and leaves the
//! row `cleanup_failed` with the reason; `LAND` never moves the default
//! branch.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use super::reply::Decided;
use super::{human_bytes, listed_worktrees, refs, Row};
use crate::runtime::process::scrub_child_env;
use crate::taskledger::finish::{self, land, WorktreeRow};
use crate::taskledger::git::{dir_bytes, Git};
use crate::taskledger::model::{Actor, EventKind, TriageDecision, WorktreeStatus};
use crate::taskledger::store::{optional, Store};
use crate::taskledger::tasks::{self, NewTask};
use crate::taskledger::util::{self, About};
use crate::taskledger::{import, runs};

/// What the decisions run with.
#[derive(Debug, Clone)]
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
    let mut version = Command::new(gh);
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
    let wt = adopt(store.conn(), env.root, row)?;
    let result = match d.decision {
        TriageDecision::Keep => Ok(Done {
            text: "kept; it comes back at the next triage".into(),
            ..Default::default()
        }),
        TriageDecision::Archive => archive(env, row, &wt),
        TriageDecision::Delete => delete(env, row, &wt),
        TriageDecision::Land => land_decision(store, env, row, &wt),
        TriageDecision::Finish => finish_decision(store, env, row, &wt, direction(d)),
        TriageDecision::Pr => pr(env, row, &wt, direction(d)),
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

/// The ledger row of a triage row. A folder that no row named gets one
/// (`origin = 'import'`), so its decision is recorded like any other.
fn adopt(conn: &Connection, root: &Path, row: &Row) -> Result<WorktreeRow> {
    if let Some(id) = &row.worktree_id {
        return finish::worktree(conn, id);
    }
    let existing: Option<String> = optional(conn.query_row(
        "SELECT worktree_id FROM worktrees WHERE path = ?1",
        [&row.path],
        |r| r.get(0),
    ))?;
    let id = existing.unwrap_or_else(util::new_id);
    conn.execute(
        "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, base_commit, status, origin, created_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'abandoned', 'import', ?6, ?7)
         ON CONFLICT(path) DO UPDATE SET status = 'abandoned', branch = ?4, base = ?5, reason = ?7,
            decision = NULL, decided_at = NULL, cleaned_at = NULL",
        params![
            id,
            root.to_string_lossy(),
            row.path,
            row.branch.as_deref().unwrap_or(""),
            row.base,
            util::now(),
            "found on disk by worktree triage"
        ],
    )?;
    finish::worktree(conn, &id)
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
        "UPDATE worktrees SET decision = ?2, decided_at = ?3, status = COALESCE(?4, status),
            archive_ref = COALESCE(?5, archive_ref),
            bytes_freed = COALESCE(?6, bytes_freed, CASE WHEN status = 'cleaned' THEN bytes_on_disk END),
            cleaned_at = CASE WHEN ?7 THEN ?3 ELSE cleaned_at END
         WHERE worktree_id = ?1",
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
            Some(q) => ("requeued", q.task_id.clone()),
            None => ("dismissed", format!("worktree triage: {d}")),
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

/// The folder must be on the branch the ledger recorded: removing a
/// checkout that moved would keep the wrong commit.
fn check_identity(git: &Git, wt: &WorktreeRow) -> Result<()> {
    let dir = Path::new(&wt.path);
    if !dir.exists() || wt.branch.is_empty() {
        return Ok(());
    }
    match git.current_branch(dir) {
        Some(b) if b == wt.branch => Ok(()),
        Some(b) => bail!(
            "the worktree is on {b}, not its recorded branch {}; nothing removed",
            wt.branch
        ),
        None => bail!(
            "the worktree HEAD is detached, not on its recorded branch {}; nothing removed",
            wt.branch
        ),
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

/// Remove the folder, never forced. The bytes it held.
fn remove_folder(git: &Git, root: &Path, wt: &WorktreeRow) -> Result<u64> {
    let dir = PathBuf::from(&wt.path);
    if !dir.exists() {
        return Ok(0);
    }
    let bytes = dir_bytes(&dir);
    git.run(root, &["worktree", "remove", &wt.path])?;
    if dir.exists() {
        bail!("{} still exists after git worktree remove", wt.path);
    }
    Ok(bytes)
}

/// `ARCHIVE`: the work stays at `refs/horch/archive/<name>` and on its
/// branch; the folder goes.
fn archive(env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let tip = prepare(env.git, env.root, row, wt)?;
    archive_at(env, row, wt, &tip, "")
}

fn archive_at(env: &Env<'_>, row: &Row, wt: &WorktreeRow, tip: &str, before: &str) -> Result<Done> {
    let r = refs::archive_ref(&row.name);
    refs::write(env.git, env.root, &r, tip)?;
    must_be_safe(env.git, env.root, wt, &r, tip)?;
    let bytes = remove_folder(env.git, env.root, wt)?;
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
fn delete(env: &Env<'_>, row: &Row, wt: &WorktreeRow) -> Result<Done> {
    let tip = prepare(env.git, env.root, row, wt)?;
    let trash = refs::trash_ref(&row.name, crate::clock::now());
    refs::write(env.git, env.root, &trash, &tip)?;
    must_be_safe(env.git, env.root, wt, &trash, &tip)?;
    let bytes = remove_folder(env.git, env.root, wt)?;
    let branch = delete_branch(env.git, env.root, wt, &trash, &tip)?;
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
    let o = land::remove(store, env.git, wt, on, None)?;
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
/// worktree, as `ledger reopen` does.
fn finish_decision(
    store: &mut Store,
    env: &Env<'_>,
    row: &Row,
    wt: &WorktreeRow,
    direction: &str,
) -> Result<Done> {
    let (run, _) = runs::resolve(store.conn(), env.root, None, None)
        .context("FINISH needs an active run; start one with horch fleet")?;
    let ticket = write_ticket(env.root, row, wt, direction)?;
    let title = format!("Finish {}: {}", row.name, first_line(direction, 60));
    let new = NewTask::work(&title, &ticket, env.primary);
    let task = store.immediate(|tx| {
        let task = tasks::add(tx, &run, &new, Actor::Operator)?;
        tx.execute(
            "UPDATE worktrees SET task_id = ?2, worker_id = NULL, status = 'open', reason = NULL
             WHERE worktree_id = ?1",
            [&wt.worktree_id, &task.task_id],
        )?;
        Ok(task)
    })?;
    Ok(Done {
        text: format!(
            "{} queued for {} with the brief {ticket}; it inherits {}",
            task.short(),
            env.primary,
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

/// `PR`: push the branch, open a pull request with the direction as its
/// title and body, then archive the folder.
fn pr(env: &Env<'_>, row: &Row, wt: &WorktreeRow, direction: &str) -> Result<Done> {
    let (Some(gh), Some(remote)) = (env.gh, env.remote) else {
        bail!("PR is not offered here: gh or a git remote is missing");
    };
    let tip = prepare(env.git, env.root, row, wt)?;
    let b = &wt.branch;
    env.git
        .run(env.root, &["push", "--quiet", remote, &format!("{b}:{b}")])?;
    let mut create = Command::new(gh);
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
        env,
        row,
        wt,
        &tip,
        &format!("pushed {b} to {remote}; opened {url}; "),
    )
}

/// `horch worktree restore <name>`: make the folder again from the
/// archive ref (else the newest trash ref). The row is `open` and
/// undecided again, so the next triage shows it.
pub fn restore(store: &Store, git: &Git, root: &Path, name: &str) -> Result<String> {
    let r = refs::restorable(git, root, name)?
        .with_context(|| format!("no refs/horch/archive/{name} and no trash ref of {name}"))?;
    let commit = git.rev(root, &r)?;
    let path = root.join(".worktrees").join(name);
    if path.exists() {
        bail!("{} exists already", path.display());
    }
    let row: Option<(String, String)> = optional(store.conn().query_row(
        "SELECT worktree_id, branch FROM worktrees WHERE archive_ref = ?1
         ORDER BY decided_at DESC LIMIT 1",
        [&r],
        |x| Ok((x.get(0)?, x.get(1)?)),
    ))?;
    let branch = row
        .as_ref()
        .map(|(_, b)| b.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| format!("restored/{name}"));
    let path_text = path.to_string_lossy().into_owned();
    if git.branch_exists(root, &branch) {
        if git.rev(root, &branch)? != commit {
            bail!("the branch {branch} moved since {r}; check it out by hand: git worktree add {path_text} {branch}");
        }
        git.run(root, &["worktree", "add", "--quiet", &path_text, &branch])?;
    } else {
        git.run(
            root,
            &[
                "worktree", "add", "--quiet", "-b", &branch, &path_text, &commit,
            ],
        )?;
    }
    if let Some((id, _)) = &row {
        store.conn().execute(
            "UPDATE worktrees SET status = 'open', decision = NULL, decided_at = NULL, cleaned_at = NULL,
                worker_id = NULL, reason = ?2 WHERE worktree_id = ?1",
            params![id, format!("restored from {r}")],
        )?;
    }
    Ok(format!(
        "restored {path_text} on {branch} at {} from {r}",
        short(&commit)
    ))
}
