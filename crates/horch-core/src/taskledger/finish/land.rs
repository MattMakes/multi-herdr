//! The check, recheck, land and cleanup steps (#36).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusqlite::params;

use super::{set_worktree, worktree_of, Ctx, Outcome, WorktreeRow};
use crate::harness::codex::fleet_write_sandbox;
use crate::harness::HarnessKind;
use crate::taskledger::bounded::run_logged;
use crate::taskledger::git::{dir_bytes, Git};
use crate::taskledger::model::{Actor, EventKind, StepName, WorktreeStatus};
use crate::taskledger::store::Store;
use crate::taskledger::util::{self, About};

/// How long a land waits for another land of the same repository.
pub const LAND_LOCK_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// The longest a check or recheck runs before its process group is
/// stopped and the step fails.
const CHECK_DEADLINE: Duration = Duration::from_secs(60 * 60);

/// The most bytes of check output the step log keeps; the rest is read and
/// dropped.
const CHECK_LOG_CAP: u64 = 64 << 20;

/// Absolute codex paths a Codex-held check tries in order when
/// `HORCH_CODEX_BIN` is not absolute. Never a lookup through `PATH`: a
/// worker-writable folder on it would run a fake codex outside the sandbox.
const TRUSTED_CODEXES: [&str; 3] = [
    "/opt/homebrew/bin/codex",
    "/usr/local/bin/codex",
    "/usr/bin/codex",
];

/// How a check ended. A runner failure is an `Err` around it.
enum CheckResult {
    Passed,
    Failed(String),
}

/// Run `command` in `dir` with the shell, in its own process group, for at
/// most [`CHECK_DEADLINE`]; its output streams to `log`, capped at
/// [`CHECK_LOG_CAP`]. An `Err` is a check that could not run. When the worker
/// that holds the claim runs on Codex, the check runs in the same `codex
/// sandbox` its pane has: the committed code is the worker's, and outside a
/// sandbox it would run with the operator's rights. The harness comes from
/// the ledger's worker row, which spawn wrote, never from the worker's
/// environment. A row with no harness fails: the check would run
/// unsandboxed for a worker that may be on Codex.
fn run_check(cx: &Ctx<'_>, dir: &Path, command: &str, log: &Path) -> Result<CheckResult> {
    let sandboxed = is_codex(cx)?;
    // /tmp is read-only in the sandbox: a private temp dir in the state
    // root, as a pane has, removed after the check.
    let tmp = sandboxed.then(|| {
        cx.tools
            .state_root
            .join("tmp")
            .join(format!("check-{}", cx.worker.worker_id))
    });
    let mut cmd = match &tmp {
        Some(tmp) => {
            let codex = trusted_codex(cx, dir)?;
            std::fs::create_dir_all(tmp).with_context(|| format!("creating {}", tmp.display()))?;
            let mut cmd = fleet_write_sandbox(&codex, &cx.tools.state_root, dir);
            cmd.arg(&cx.tools.shell).env("TMPDIR", tmp);
            cmd
        }
        None => Command::new(&cx.tools.shell),
    };
    cmd.arg("-c")
        .arg(command)
        .current_dir(dir)
        .env_remove("ANTHROPIC_API_KEY");
    let place = if sandboxed { " (codex sandbox)" } else { "" };
    std::fs::write(log, format!("$ {command}\n# in {}{place}\n", dir.display()))?;
    let ran = run_logged(cmd, log, CHECK_DEADLINE, CHECK_LOG_CAP);
    if let Some(tmp) = &tmp {
        let _ = std::fs::remove_dir_all(tmp);
    }
    let (status, timed_out) = ran.with_context(|| format!("running the check `{command}`"))?;
    let end = if timed_out {
        format!(
            "\n# check ran longer than {} min\n",
            CHECK_DEADLINE.as_secs() / 60
        )
    } else {
        format!("\n# exit: {:?}\n", status.code())
    };
    let mut file = std::fs::OpenOptions::new().append(true).open(log)?;
    std::io::Write::write_all(&mut file, end.as_bytes())?;
    Ok(match (timed_out, status.success()) {
        (true, _) => CheckResult::Failed(format!(
            "check ran longer than {} min; read {}",
            CHECK_DEADLINE.as_secs() / 60,
            log.display()
        )),
        (false, true) => CheckResult::Passed,
        (false, false) => CheckResult::Failed(format!(
            "`{command}` failed; read {} and fix it",
            log.display()
        )),
    })
}

/// Whether the worker that holds the claim runs on Codex, from its ledger
/// row. A row with no harness is an error.
fn is_codex(cx: &Ctx<'_>) -> Result<bool> {
    match cx.worker.harness.as_deref() {
        Some(h) => Ok(h == HarnessKind::Codex.as_str()),
        None => bail!(
            "the ledger does not record this worker's harness. The orchestrator \
             replaces the worker with horch spawn, or records a new row with \
             horch ledger worker new --harness <claude|codex|...>"
        ),
    }
}

/// The codex that sandboxes a check: `HORCH_CODEX_BIN` when absolute, else
/// the first of [`TRUSTED_CODEXES`], as a canonical path. Refused inside the
/// worktree `dir`, the main checkout or the state root: the sandbox writes
/// each, and this codex runs outside it.
fn trusted_codex(cx: &Ctx<'_>, dir: &Path) -> Result<PathBuf> {
    let configured = &cx.tools.codex;
    let candidates: Vec<PathBuf> = if configured.is_absolute() {
        vec![configured.clone()]
    } else {
        TRUSTED_CODEXES.iter().map(PathBuf::from).collect()
    };
    let writable: Vec<PathBuf> = [dir, cx.root.as_path(), cx.tools.state_root.as_path()]
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();
    for candidate in &candidates {
        let Ok(real) = std::fs::canonicalize(candidate) else {
            continue;
        };
        if let Some(root) = writable.iter().find(|w| real.starts_with(w)) {
            bail!(
                "refused: codex {} is inside {}, which a Codex worker can write. Set \
                 HORCH_CODEX_BIN to an absolute codex path outside it",
                real.display(),
                root.display()
            );
        }
        if real.is_file() {
            return Ok(real);
        }
    }
    bail!(
        "refused: no codex at {}. Set HORCH_CODEX_BIN to an absolute codex path",
        candidates
            .iter()
            .map(|c| c.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
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
    if cx.task.uses_worktree() {
        require_clean(&cx.tools.git, &cx.dir)?;
    }
    let log = cx
        .evidence_dir
        .join(format!("{}-{}.log", cx.seq, name.as_str()));
    let tree = cx.tools.git.tree(&cx.dir).ok();
    match run_check(cx, &cx.dir, cmd, &log)? {
        CheckResult::Passed => {
            Ok(Outcome::pass("exit 0", Some(log), &format!("`{cmd}` passed")).judged(tree))
        }
        CheckResult::Failed(why) => Ok(Outcome::fail("red", Some(log), &why).judged(tree)),
    }
}

/// What a worker does about a land whose rebase conflicts. A Codex worker
/// cannot write `.git`, so it cannot rebase: it reports the conflict.
fn conflict_text(cx: &Ctx<'_>, run_branch: &str, dir: &Path) -> String {
    if cx.worker.harness.as_deref() == Some(HarnessKind::Codex.as_str()) {
        format!(
            "rebase on {run_branch} conflicts in {}. A Codex worker cannot rebase: send \
             BLOCKED: to the orchestrator with the conflicting files, and ask for a Claude \
             worker to rebase",
            dir.display()
        )
    } else {
        format!(
            "rebase on {run_branch} conflicts; resolve it with git rebase {run_branch} in {}, \
             commit, then run finish again",
            dir.display()
        )
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
    if wt.status == WorktreeStatus::Landed.as_str()
        || wt.status == WorktreeStatus::CleanupFailed.as_str()
    {
        let c = wt.landed_commit.clone().unwrap_or_default();
        return Ok(Outcome::pass(&c, None, "already landed"));
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
    // Until the run branch moves, any failure sets the worktree back to
    // open: a Codex worker commits only in an open worktree, and a land
    // left in `landing` is never retried.
    let new = match advance(cx, &dir, &run_branch, &log) {
        Ok(Advance::Moved(new)) => new,
        Ok(Advance::Red(why)) => return red(store, why),
        Err(e) => return red(store, format!("the land stopped: {e:#}")),
    };
    // The run branch has the commit: record it before the push can fail.
    store.conn().execute(
        "UPDATE worktrees SET status = 'landed', landed_commit = ?2, landed_at = ?3 WHERE worktree_id = ?1",
        params![wt.worktree_id, new, util::now()],
    )?;
    let mut note = format!(
        "{} landed on {run_branch} at {}",
        wt.branch,
        &new[..new.len().min(12)]
    );
    if let Some(remote) = &cx.run.remote {
        let (push, skipped) = git.push(&cx.root, remote, &run_branch, &[])?;
        if let Some(skipped) = skipped {
            note.push_str(&format!("; {skipped}"));
        }
        if !push.ok {
            // The land stands; the push is retried by the next finish.
            return Ok(Outcome::fail(
                "landed, not pushed",
                Some(log),
                &format!("push of {run_branch} to {remote} failed: {}", push.stderr),
            ));
        }
        note.push_str(&format!("; pushed to {remote}"));
    }
    store.conn().execute(
        "UPDATE worktrees SET reason = NULL WHERE worktree_id = ?1",
        params![wt.worktree_id],
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

/// How the run branch fared in a land.
enum Advance {
    /// The run branch is at this commit now.
    Moved(String),
    /// The run branch did not move, for this reason.
    Red(String),
}

/// Rebase the worktree in `dir` on the run branch's tip, run the land
/// check, then fast-forward the run branch. An `Err` comes only before the
/// run branch moves.
fn advance(cx: &Ctx<'_>, dir: &Path, run_branch: &str, log: &Path) -> Result<Advance> {
    let git = &cx.tools.git;
    // Rebase on the run branch's tip.
    let target_before = git.rev(&cx.root, run_branch)?;
    let rebase = git.try_run(dir, &["rebase", &target_before])?;
    if !rebase.ok {
        let _ = git.try_run(dir, &["rebase", "--abort"]);
        std::fs::write(log, format!("{}\n{}", rebase.stdout, rebase.stderr))?;
        return Ok(Advance::Red(conflict_text(cx, run_branch, dir)));
    }
    if let Some(cmd) = cx
        .task
        .land_check
        .as_deref()
        .filter(|c| !c.trim().is_empty())
    {
        match run_check(cx, dir, cmd, log) {
            Ok(CheckResult::Passed) => {}
            Ok(CheckResult::Failed(why)) => {
                return Ok(Advance::Red(format!("after the rebase: {why}")))
            }
            Err(e) => return Ok(Advance::Red(format!("the check could not run: {e:#}"))),
        }
    }
    let new = git.rev(dir, "HEAD")?;
    if !git.is_ancestor(&cx.root, &target_before, &new) {
        return Ok(Advance::Red(
            "the rebased branch does not contain the run branch".into(),
        ));
    }
    // Fast-forward the run branch: by merge when it is checked out in the
    // main checkout, else by a compare-and-swap of its ref. Never forced.
    let checked_out = git.current_branch(&cx.root).as_deref() == Some(run_branch);
    if checked_out {
        // The repository's filters (git-lfs), never its hooks: a hooks path
        // can be a folder the worker's commits fill.
        let ff = git
            .with_repo_filters()
            .try_run(&cx.root, &["merge", "--ff-only", &new])?;
        if !ff.ok {
            return Ok(Advance::Red(format!(
                "fast-forward of {run_branch} failed: {}",
                ff.stderr
            )));
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
            return Ok(Advance::Red(format!(
                "{run_branch} moved during the land; run finish again"
            )));
        }
    }
    Ok(Advance::Moved(new))
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
    if let Some(remote) = &cx.run.remote {
        let (push, _) = git.push(&cx.root, remote, run_branch, &[])?;
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
        let _ = git.try_run(root, &["fetch", remote, run_branch]);
        let remote_ref = format!("refs/remotes/{remote}/{run_branch}");
        if git.rev(root, &remote_ref).is_err() || !git.is_ancestor(root, landed, &remote_ref) {
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
