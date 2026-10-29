//! The check, recheck, land and cleanup steps (#36).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusqlite::params;

use super::{set_worktree, worktree_of, Ctx, Outcome, WorktreeRow};
use crate::taskledger::git::{dir_bytes, Git};
use crate::taskledger::model::{Actor, EventKind, StepName, WorktreeStatus};
use crate::taskledger::store::Store;
use crate::taskledger::util::{self, About};

/// How long a land waits for another land of the same repository.
pub const LAND_LOCK_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Run `command` in `dir` with the shell; the log goes to `log`.
fn run_check(shell: &Path, dir: &Path, command: &str, log: &Path) -> Result<bool> {
    let out = Command::new(shell)
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .with_context(|| format!("running the check `{command}`"))?;
    let mut text = format!("$ {command}\n# in {}\n", dir.display());
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    text.push_str(&format!("\n# exit: {:?}\n", out.status.code()));
    std::fs::write(log, text)?;
    Ok(out.status.success())
}

/// Committed work only: a step judges commits, never a half-saved tree.
fn require_clean(git: &Git, dir: &Path) -> Result<()> {
    let dirty = git.dirty(dir)?;
    if !dirty.is_empty() {
        bail!(
            "{} has uncommitted changes ({} files, e.g. {}). Commit your work, then run horch ledger finish again",
            dir.display(),
            dirty.len(),
            dirty[0].trim()
        );
    }
    Ok(())
}

/// `check` and `recheck`: the task's check command, exit 0 to pass.
pub fn check(cx: &Ctx<'_>, name: StepName) -> Result<Outcome> {
    let Some(cmd) = cx
        .task
        .land_check
        .as_deref()
        .filter(|c| !c.trim().is_empty())
    else {
        return Ok(Outcome::not_applicable(
            "the task has no check command (horch ledger add --check)",
        ));
    };
    require_clean(&cx.tools.git, &cx.dir)?;
    let log = cx
        .evidence_dir
        .join(format!("{}-{}.log", cx.seq, name.as_str()));
    let tree = cx.tools.git.tree(&cx.dir).ok();
    if run_check(&cx.tools.shell, &cx.dir, cmd, &log)? {
        Ok(Outcome::pass("exit 0", Some(log), &format!("`{cmd}` passed")).judged(tree))
    } else {
        Ok(Outcome::fail(
            "red",
            Some(log.clone()),
            &format!("`{cmd}` failed; read {} and fix it", log.display()),
        )
        .judged(tree))
    }
}

/// The lock every land of 1 repository takes, so lands go in a line.
pub(crate) fn land_lock(cx: &Ctx<'_>) -> Result<crate::fsx::DirLockGuard> {
    let dir = cx.tools.evidence_root.join("locks");
    let key = sha_hex(&cx.root.to_string_lossy());
    Ok(crate::fsx::DirLock::acquire(
        &dir,
        &format!("land-{}", &key[..16]),
        LAND_LOCK_TIMEOUT,
        LAND_LOCK_TIMEOUT,
    )?)
}

fn sha_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// `land`: rebase on the run branch, run the check, fast-forward the run
/// branch, push it. A red check or a conflict changes nothing.
pub fn land(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let git = &cx.tools.git;
    let run_branch = cx
        .run
        .run_branch
        .clone()
        .context("the run has no run branch")?;
    let log = cx.evidence_dir.join(format!("{}-land.log", cx.seq));
    let Some(wt) = cx.worktree.clone() else {
        return land_shared(cx, &run_branch, &log);
    };
    if is_landed(&wt) && !reopen_if_new_commits(store, git, &cx.root, &wt, &run_branch)? {
        return land_again(cx, &wt, &run_branch, &log);
    }
    let dir = PathBuf::from(&wt.path);
    require_clean(git, &dir)?;
    let _lock = land_lock(cx)?;
    set_worktree(store.conn(), &wt.worktree_id, WorktreeStatus::Landing, None)?;
    let about = About {
        run: Some(&cx.run.run_id),
        task: Some(&cx.task.task_id),
        worker: Some(&cx.worker.worker_id),
    };
    util::event(
        store.conn(),
        &about,
        Actor::Worker,
        EventKind::LandStarted,
        &format!("Landing {} on {run_branch}.", wt.branch),
    )?;
    let red = |store: &mut Store, why: String| -> Result<Outcome> {
        set_worktree(
            store.conn(),
            &wt.worktree_id,
            WorktreeStatus::Open,
            Some(&why),
        )?;
        util::event(
            store.conn(),
            &about,
            Actor::Worker,
            EventKind::LandRed,
            &why,
        )?;
        Ok(Outcome::fail("not landed", Some(log.clone()), &why))
    };
    // Rebase on the run branch's tip.
    let target_before = git.rev(&cx.root, &run_branch)?;
    let rebase = git.try_run(&dir, &["rebase", &target_before])?;
    if !rebase.ok {
        let _ = git.try_run(&dir, &["rebase", "--abort"]);
        std::fs::write(&log, format!("{}\n{}", rebase.stdout, rebase.stderr))?;
        return red(
            store,
            format!(
                "rebase on {run_branch} conflicts; resolve it with git rebase {run_branch} in {}, commit, then run finish again",
                dir.display()
            ),
        );
    }
    if let Some(cmd) = cx
        .task
        .land_check
        .as_deref()
        .filter(|c| !c.trim().is_empty())
    {
        if !run_check(&cx.tools.shell, &dir, cmd, &log)? {
            return red(
                store,
                format!("`{cmd}` failed after the rebase; read {}", log.display()),
            );
        }
    }
    let new = git.rev(&dir, "HEAD")?;
    if !git.is_ancestor(&cx.root, &target_before, &new) {
        return red(
            store,
            "the rebased branch does not contain the run branch".into(),
        );
    }
    // Fast-forward the run branch: by merge when it is checked out in the
    // main checkout, else by a compare-and-swap of its ref. Never forced.
    let checked_out = git.current_branch(&cx.root).as_deref() == Some(run_branch.as_str());
    if checked_out {
        let ff = git.try_run(&cx.root, &["merge", "--ff-only", &new])?;
        if !ff.ok {
            return red(
                store,
                format!("fast-forward of {run_branch} failed: {}", ff.stderr),
            );
        }
    } else {
        let r = git.try_run(
            &cx.root,
            &[
                "update-ref",
                &format!("refs/heads/{run_branch}"),
                &new,
                &target_before,
            ],
        )?;
        if !r.ok {
            return red(
                store,
                format!("{run_branch} moved during the land; run finish again"),
            );
        }
    }
    let mut note = format!(
        "{} landed on {run_branch} at {}",
        wt.branch,
        &new[..new.len().min(12)]
    );
    if let Some(remote) = &cx.run.remote {
        let push = git.try_run(
            &cx.root,
            &["push", remote, &format!("{run_branch}:{run_branch}")],
        )?;
        if !push.ok {
            // The land stands; the next finish pushes again (`land_again`).
            set_worktree(store.conn(), &wt.worktree_id, WorktreeStatus::Landed, None)?;
            store.conn().execute(
                "UPDATE worktrees SET landed_commit = ?2, landed_at = ?3 WHERE worktree_id = ?1",
                params![wt.worktree_id, new, util::now()],
            )?;
            return Ok(Outcome::fail(
                "landed, not pushed",
                Some(log),
                &format!("push of {run_branch} to {remote} failed: {}", push.stderr),
            ));
        }
        note.push_str(&format!("; pushed to {remote}"));
    }
    store.conn().execute(
        "UPDATE worktrees SET status = 'landed', landed_commit = ?2, landed_at = ?3, reason = NULL
         WHERE worktree_id = ?1",
        params![wt.worktree_id, new, util::now()],
    )?;
    util::event(
        store.conn(),
        &about,
        Actor::Worker,
        EventKind::Landed,
        &note,
    )?;
    std::fs::write(&log, format!("{note}\n"))?;
    Ok(Outcome::pass(&new, Some(log), &note))
}

/// Whether the worktree's work landed already.
fn is_landed(wt: &WorktreeRow) -> bool {
    wt.status == WorktreeStatus::Landed.as_str()
        || wt.status == WorktreeStatus::CleanupFailed.as_str()
}

/// A landed worktree whose branch has commits the run branch lacks goes
/// back to `open`, so they are checked, reviewed and landed (#154). Returns
/// whether it did.
pub(crate) fn reopen_if_new_commits(
    store: &Store,
    git: &Git,
    root: &Path,
    wt: &WorktreeRow,
    run_branch: &str,
) -> Result<bool> {
    if !is_landed(wt) {
        return Ok(false);
    }
    let new = git
        .commits_missing_from(root, &wt.branch, run_branch)
        .unwrap_or_default();
    if new.is_empty() {
        return Ok(false);
    }
    set_worktree(
        store.conn(),
        &wt.worktree_id,
        WorktreeStatus::Open,
        Some(&format!("{} commits after the land", new.len())),
    )?;
    Ok(true)
}

/// Whether `remote`'s run branch holds `commit` (after a fetch).
fn remote_has(git: &Git, root: &Path, remote: &str, run_branch: &str, commit: &str) -> bool {
    let _ = git.try_run(root, &["fetch", remote, run_branch]);
    let remote_ref = format!("refs/remotes/{remote}/{run_branch}");
    git.rev(root, &remote_ref).is_ok() && git.is_ancestor(root, commit, &remote_ref)
}

/// A landed worktree with no new commit: push the run branch again when the
/// remote lacks the land, else pass at once.
fn land_again(cx: &Ctx<'_>, wt: &WorktreeRow, run_branch: &str, log: &Path) -> Result<Outcome> {
    let git = &cx.tools.git;
    let landed = wt.landed_commit.clone().unwrap_or_default();
    let Some(remote) = cx.run.remote.as_deref() else {
        return Ok(Outcome::pass(&landed, None, "already landed"));
    };
    if landed.is_empty() || remote_has(git, &cx.root, remote, run_branch, &landed) {
        return Ok(Outcome::pass(&landed, None, "already landed"));
    }
    let _lock = land_lock(cx)?;
    let push = git.try_run(
        &cx.root,
        &["push", remote, &format!("{run_branch}:{run_branch}")],
    )?;
    if !push.ok {
        return Ok(Outcome::fail(
            "landed, not pushed",
            None,
            &format!(
                "push of {run_branch} to {remote} failed again: {}",
                push.stderr
            ),
        ));
    }
    let note = format!("{run_branch} pushed to {remote}; it holds {landed}");
    std::fs::write(log, format!("{note}\n"))?;
    Ok(Outcome::pass(&landed, Some(log.to_path_buf()), &note))
}

/// A shared-checkout task: its commits are on the run branch already when
/// the checkout is on it; push them.
fn land_shared(cx: &Ctx<'_>, run_branch: &str, log: &Path) -> Result<Outcome> {
    let git = &cx.tools.git;
    require_clean(git, &cx.root)?;
    if git.current_branch(&cx.root).as_deref() != Some(run_branch) {
        bail!("a shared-checkout task lands only when the checkout is on {run_branch}");
    }
    let head = git.rev(&cx.root, "HEAD")?;
    if let Some(start) = super::shared_start(&cx.tools.evidence_root, &cx.task.task_id) {
        if head == start || !git.is_ancestor(&cx.root, &start, &head) {
            bail!(
                "{run_branch} has no commit since the claim (it started at {}). Commit your work on {run_branch}, then run horch ledger finish again",
                &start[..start.len().min(12)]
            );
        }
    }
    if let Some(remote) = &cx.run.remote {
        let push = git.try_run(
            &cx.root,
            &["push", remote, &format!("{run_branch}:{run_branch}")],
        )?;
        if !push.ok {
            return Ok(Outcome::fail("not pushed", None, &push.stderr));
        }
    }
    std::fs::write(log, format!("{run_branch} at {head}\n"))?;
    Ok(Outcome::pass(
        &head,
        Some(log.to_path_buf()),
        "on the run branch",
    ))
}

/// Why a worktree may not be removed yet, or `None` when no work can be lost.
pub fn unsafe_to_remove(
    git: &Git,
    root: &Path,
    wt: &WorktreeRow,
    run_branch: &str,
    remote: Option<&str>,
) -> Result<Option<String>> {
    let dir = Path::new(&wt.path);
    if !dir.exists() {
        return Ok(None);
    }
    let dirty = git.dirty(dir)?;
    if !dirty.is_empty() {
        return Ok(Some(format!(
            "{} uncommitted or untracked files, e.g. {}",
            dirty.len(),
            dirty[0].trim()
        )));
    }
    let stashes = git.stashes_on(root, &wt.branch)?;
    if !stashes.is_empty() {
        return Ok(Some(format!("a stash names {}: {}", wt.branch, stashes[0])));
    }
    let missing = git.commits_missing_from(root, &wt.branch, run_branch)?;
    if !missing.is_empty() {
        return Ok(Some(format!(
            "{} commits on {} are not on {run_branch}",
            missing.len(),
            wt.branch
        )));
    }
    if let (Some(remote), Some(landed)) = (remote, wt.landed_commit.as_deref()) {
        if !remote_has(git, root, remote, run_branch, landed) {
            return Ok(Some(format!(
                "{remote}/{run_branch} does not have {landed} yet"
            )));
        }
    }
    Ok(None)
}

/// Remove 1 landed worktree when no work can be lost; record the bytes.
pub fn remove(
    store: &Store,
    git: &Git,
    wt: &WorktreeRow,
    run_branch: &str,
    remote: Option<&str>,
) -> Result<Outcome> {
    let root = PathBuf::from(&wt.repo_root);
    if let Some(why) = unsafe_to_remove(git, &root, wt, run_branch, remote)? {
        return Ok(Outcome::fail(
            "kept",
            None,
            &format!("the worktree stays: {why}"),
        ));
    }
    let dir = PathBuf::from(&wt.path);
    let bytes = dir_bytes(&dir) as i64;
    let mut errors = Vec::new();
    if dir.exists() {
        let out = git.try_run(&root, &["worktree", "remove", &wt.path])?;
        if !out.ok {
            errors.push(format!("git worktree remove: {}", out.stderr));
        }
    }
    if errors.is_empty() && git.branch_exists(&root, &wt.branch) {
        // `branch -d` checks the branch is merged into its upstream (else the
        // checked-out branch): make that the run branch, so git itself
        // verifies the work is on it. Never `-D`.
        let _ = git.try_run(
            &root,
            &["branch", "--set-upstream-to", run_branch, &wt.branch],
        );
        let out = git.try_run(&root, &["branch", "-d", &wt.branch])?;
        if !out.ok {
            errors.push(format!("git branch -d: {}", out.stderr));
        }
    }
    if errors.is_empty() && dir.exists() {
        errors.push(format!("{} still exists", dir.display()));
    }
    if !errors.is_empty() {
        let why = errors.join("; ");
        set_worktree(
            store.conn(),
            &wt.worktree_id,
            WorktreeStatus::CleanupFailed,
            Some(&why),
        )?;
        util::event(
            store.conn(),
            &About {
                task: wt.task_id.as_deref(),
                worker: wt.worker_id.as_deref(),
                ..Default::default()
            },
            Actor::Horch,
            EventKind::CleanupFailed,
            &why,
        )?;
        return Ok(Outcome::fail(
            "cleanup failed",
            None,
            &format!("{why}; the watchdog retries it"),
        ));
    }
    store.conn().execute(
        "UPDATE worktrees SET status = 'cleaned', cleaned_at = ?2, bytes_on_disk = ?3, reason = NULL
         WHERE worktree_id = ?1",
        params![wt.worktree_id, util::now(), bytes],
    )?;
    util::event(
        store.conn(),
        &About {
            task: wt.task_id.as_deref(),
            worker: wt.worker_id.as_deref(),
            ..Default::default()
        },
        Actor::Horch,
        EventKind::Cleaned,
        &format!("Removed {} ({} bytes freed).", wt.path, bytes),
    )?;
    Ok(Outcome::pass(
        &format!("{bytes} bytes freed"),
        None,
        &format!("{} is gone", wt.path),
    ))
}

/// `cleanup`: remove the worktree when no work can be lost.
pub fn cleanup(store: &mut Store, cx: &Ctx<'_>) -> Result<Outcome> {
    let Some(wt) = worktree_of(store.conn(), &cx.task.task_id)? else {
        return Ok(Outcome::not_applicable("the task has no worktree"));
    };
    let run_branch = cx
        .run
        .run_branch
        .clone()
        .context("the run has no run branch")?;
    remove(
        store,
        &cx.tools.git,
        &wt,
        &run_branch,
        cx.run.remote.as_deref(),
    )
}

/// Retry every `cleanup_failed` worktree (the watchdog calls this).
pub fn retry_cleanup(store: &mut Store, git: &Git) -> Result<Vec<(String, Outcome)>> {
    let mut out = Vec::new();
    for wt in super::worktrees_with(store.conn(), WorktreeStatus::CleanupFailed)? {
        let Some(task) = &wt.task_id else { continue };
        let t = crate::taskledger::tasks::get(store.conn(), task)?;
        let run = crate::taskledger::runs::get(store.conn(), &t.run_id)?;
        let Some(rb) = run.run_branch.clone() else {
            continue;
        };
        let o = remove(store, git, &wt, &rb, run.remote.as_deref())?;
        out.push((wt.path.clone(), o));
    }
    Ok(out)
}
