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

/// How many times a land rebases and checks again because the run branch
/// moved during its check.
const MAX_LAND_TRIES: u32 = 5;

/// How long a check runs before it is killed, unless the task sets it
/// (`horch ledger add --check-timeout`).
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The longest check timeout a task may set; a longer one is cut to this.
pub const MAX_CHECK_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a timed-out check gets to exit after its group is killed.
const REAP_WAIT: Duration = Duration::from_secs(5);

/// How often a land that waits for the land lock names its holder.
const LOCK_NOTICE_EVERY: Duration = Duration::from_secs(30);

fn check_timeout_path(evidence_root: &Path, task_id: &str) -> PathBuf {
    super::task_dir(evidence_root, task_id).join("check-timeout-s")
}

/// Record the check timeout of 1 task. Refuses 0 and more than
/// [`MAX_CHECK_TIMEOUT`].
pub fn set_check_timeout(evidence_root: &Path, task_id: &str, timeout: Duration) -> Result<()> {
    if timeout.is_zero() || timeout > MAX_CHECK_TIMEOUT {
        bail!(
            "a check timeout is 1 to {} s, not {} s",
            MAX_CHECK_TIMEOUT.as_secs(),
            timeout.as_secs()
        );
    }
    let path = check_timeout_path(evidence_root, task_id);
    std::fs::create_dir_all(path.parent().expect("a task folder"))?;
    std::fs::write(&path, format!("{}\n", timeout.as_secs()))?;
    Ok(())
}

/// The check timeout of 1 task: its own, else [`CHECK_TIMEOUT`].
pub fn check_timeout(evidence_root: &Path, task_id: &str) -> Duration {
    std::fs::read_to_string(check_timeout_path(evidence_root, task_id))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(|s| Duration::from_secs(s).min(MAX_CHECK_TIMEOUT))
        .unwrap_or(CHECK_TIMEOUT)
}

/// How 1 check run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Checked {
    Passed,
    Failed,
    TimedOut(Duration),
}

/// Run `cmd`, which runs `command`, in `dir`, in its own process group;
/// the output goes to `log`. At `timeout` the whole group is killed (#160),
/// and so it is on every other way out of this function, and when horch
/// gets SIGINT, SIGTERM or SIGHUP while the check runs.
fn run_check(
    mut cmd: Command,
    dir: &Path,
    command: &str,
    log: &Path,
    timeout: Duration,
) -> Result<Checked> {
    use std::io::Write;
    let mut file = std::fs::File::create(log)?;
    writeln!(file, "$ {command}\n# in {}", dir.display())?;
    cmd.current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file.try_clone()?);
    crate::runtime::process::scrub_child_env(&mut cmd);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd
        .spawn()
        .with_context(|| format!("running the check `{command}`"))?;
    let mut group = CheckGroup::new(child);
    let deadline = std::time::Instant::now()
        .checked_add(timeout.min(MAX_CHECK_TIMEOUT))
        .context("the check deadline")?;
    let status = loop {
        if let Some(st) = group.child.try_wait()? {
            break st;
        }
        if std::time::Instant::now() >= deadline {
            group.kill();
            let reaped = group.reap(REAP_WAIT);
            writeln!(file, "\n# timed out after {} s", timeout.as_secs())?;
            if !reaped {
                writeln!(file, "# the check did not exit after the kill")?;
            }
            return Ok(Checked::TimedOut(timeout));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    group.done = true;
    signals::forget();
    writeln!(file, "\n# exit: {:?}", status.code())?;
    Ok(if status.success() {
        Checked::Passed
    } else {
        Checked::Failed
    })
}

/// A running check. Dropped before it exited (an error, a panic), it kills
/// the check's whole group, so no check outlives its finish.
struct CheckGroup {
    child: std::process::Child,
    done: bool,
}

impl CheckGroup {
    fn new(child: std::process::Child) -> Self {
        signals::watch(child.id());
        Self { child, done: false }
    }

    /// Kill the group; when that fails, kill at least the shell.
    fn kill(&mut self) {
        if !kill_group(self.child.id()) {
            let _ = self.child.kill();
        }
    }

    /// Wait up to `limit` for the killed check to exit. Whether it did.
    fn reap(&mut self, limit: Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_)) | Err(_)) {
                self.done = true;
                signals::forget();
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for CheckGroup {
    fn drop(&mut self) {
        if !self.done {
            self.kill();
            self.reap(REAP_WAIT);
        }
        signals::forget();
    }
}

/// Kill the process group `pgid` (the check's pid: `process_group(0)`).
/// Whether the kill was sent.
#[cfg(unix)]
fn kill_group(pgid: u32) -> bool {
    // SAFETY: killpg takes plain integers and touches no memory.
    unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
fn kill_group(_pgid: u32) -> bool {
    false
}

/// SIGINT, SIGTERM and SIGHUP to horch while a check runs kill the check's
/// group too: the check has its own group, so a terminal's signal does not
/// reach it, and without this it ran on with no deadline.
#[cfg(unix)]
mod signals {
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Once;

    /// The group of the running check, 0 when none runs.
    static GROUP: AtomicI32 = AtomicI32::new(0);
    static INSTALL: Once = Once::new();

    extern "C" fn on_signal(sig: libc::c_int) {
        let pgid = GROUP.swap(0, Ordering::SeqCst);
        // SAFETY: killpg, signal and raise are async-signal-safe. The
        // default action is put back and the signal raised again, so horch
        // ends as it would have without this handler.
        unsafe {
            if pgid > 0 {
                libc::killpg(pgid, libc::SIGKILL);
            }
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    pub(super) fn watch(pgid: u32) {
        GROUP.store(pgid as i32, Ordering::SeqCst);
        INSTALL.call_once(|| {
            // SAFETY: the handler only calls async-signal-safe functions.
            unsafe {
                let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                libc::signal(libc::SIGINT, handler);
                libc::signal(libc::SIGTERM, handler);
                libc::signal(libc::SIGHUP, handler);
            }
        });
    }

    pub(super) fn forget() {
        GROUP.store(0, Ordering::SeqCst);
    }
}

#[cfg(not(unix))]
mod signals {
    pub(super) fn watch(_pgid: u32) {}
    pub(super) fn forget() {}
}

/// The task's check command, if it has one.
fn check_command<'a>(cx: &'a Ctx<'_>) -> Option<&'a str> {
    cx.task
        .land_check
        .as_deref()
        .filter(|c| !c.trim().is_empty())
}

/// The process that runs the check `command` in `dir`: the shell, or for a
/// Codex worker the shell inside the sandbox of its pane (#181), with the
/// pane's TMPDIR, as `/tmp` is read-only there.
fn check_process(cx: &Ctx<'_>, dir: &Path, command: &str, sandboxed: bool) -> Result<Command> {
    let codex = &cx.tools.codex;
    let mut cmd = if sandboxed {
        let role = cx.worker.role.as_deref().unwrap_or(&cx.worker.worker_id);
        let tmpdir = crate::harness::codex::pane_tmpdir(&codex.state_root, role);
        std::fs::create_dir_all(&tmpdir)
            .with_context(|| format!("creating {}", tmpdir.display()))?;
        let mut cmd = Command::new(&codex.bin);
        cmd.args(crate::harness::codex::sandbox_args(&codex.state_root, dir))
            .arg(&cx.tools.shell)
            .env("TMPDIR", tmpdir);
        cmd
    } else {
        Command::new(&cx.tools.shell)
    };
    cmd.arg("-c").arg(command);
    Ok(cmd)
}

/// Run the task's check in `dir`. `None` when it passed or there is none,
/// else how it ended and why it did not pass.
fn check_in(cx: &Ctx<'_>, dir: &Path, log: &Path) -> Result<Option<(Checked, String)>> {
    let Some(cmd) = check_command(cx) else {
        return Ok(None);
    };
    let timeout = check_timeout(&cx.tools.evidence_root, &cx.task.task_id);
    let sandboxed = cx.tools.codex.runs(cx.worker);
    let process = check_process(cx, dir, cmd, sandboxed)?;
    let checked = run_check(process, dir, cmd, log, timeout)?;
    let why = match checked {
        Checked::Passed => return Ok(None),
        Checked::Failed if sandboxed => format!(
            "`{cmd}` failed in the codex sandbox, which lets it write only {} and \
             the horch state dir; read {}",
            dir.display(),
            log.display()
        ),
        Checked::Failed => format!("`{cmd}` failed; read {}", log.display()),
        Checked::TimedOut(t) => format!(
            "`{cmd}` timed out after {} s and was killed; read {}",
            t.as_secs(),
            log.display()
        ),
    };
    Ok(Some((checked, why)))
}

/// Committed work only: a step judges commits, never a half-saved tree.
/// In the shared checkout only tracked files count: its untracked files are
/// the operator's or other workers', not the task's.
fn require_clean(git: &Git, dir: &Path, shared: bool) -> Result<()> {
    let dirty = if shared {
        git.dirty_tracked(dir)?
    } else {
        git.dirty(dir)?
    };
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
    let Some(cmd) = check_command(cx) else {
        return Ok(Outcome::not_applicable(
            "the task has no check command (horch ledger add --check)",
        ));
    };
    require_clean(&cx.tools.git, &cx.dir, !cx.task.uses_worktree())?;
    let log = cx
        .evidence_dir
        .join(format!("{}-{}.log", cx.seq, name.as_str()));
    let tree = cx.tools.git.tree(&cx.dir).ok();
    match check_in(cx, &cx.dir, &log)? {
        None => Ok(Outcome::pass("exit 0", Some(log), &format!("`{cmd}` passed")).judged(tree)),
        Some((checked, why)) => {
            let verdict = match checked {
                Checked::TimedOut(_) => "timed out",
                _ => "red",
            };
            Ok(Outcome::fail(verdict, Some(log), &format!("{why}; fix it")).judged(tree))
        }
    }
}

/// The lock every land of 1 repository takes, so lands go in a line. While
/// it waits, it prints who holds it every 30 s.
pub(crate) fn land_lock(cx: &Ctx<'_>) -> Result<crate::fsx::DirLockGuard> {
    let dir = cx.tools.evidence_root.join("locks");
    let key = sha_hex(&cx.root.to_string_lossy());
    let holder = format!(
        "{} of {} (PID {})",
        cx.task.short(),
        cx.worker.label(),
        std::process::id()
    );
    take_lock(
        &dir,
        &format!("land-{}", &key[..16]),
        &holder,
        LOCK_NOTICE_EVERY,
        LAND_LOCK_TIMEOUT,
        |held_by| eprintln!("horch ledger: waiting for the land lock; {held_by} holds it"),
    )
}

/// Take lock `name` in `dir` and write `holder` into it. Every `every`
/// that it waits, `notice` gets the current holder's line.
fn take_lock(
    dir: &Path,
    name: &str,
    holder: &str,
    every: Duration,
    timeout: Duration,
    mut notice: impl FnMut(&str),
) -> Result<crate::fsx::DirLockGuard> {
    let start = std::time::Instant::now();
    loop {
        let left = timeout.saturating_sub(start.elapsed());
        match crate::fsx::DirLock::acquire(dir, name, LAND_LOCK_TIMEOUT, every.min(left)) {
            Ok(guard) => {
                std::fs::write(guard.path().join("holder"), format!("{holder}\n"))?;
                return Ok(guard);
            }
            Err(crate::fsx::FsxError::LockTimeout { .. }) if !left.is_zero() => {
                let path = dir.join(format!("{name}.lock")).join("holder");
                let held_by = std::fs::read_to_string(path)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "another process".into());
                notice(&held_by);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn sha_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// `land`: rebase on the run branch, run the check, fast-forward the run
/// branch under the land lock, push it. A red check or a conflict changes
/// nothing.
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
    require_clean(git, &dir, false)?;
    // The task may be cancelled at any time before the fast-forward (#136).
    let still_mine = |store: &Store| -> Result<Option<Outcome>> {
        let t = super::tasks::get(store.conn(), &cx.task.task_id)?;
        if t.status == "landing"
            && t.current_worker_id.as_deref() == Some(cx.worker.worker_id.as_str())
        {
            return Ok(None);
        }
        let why = format!(
            "{} is {}: nothing landed. Stop work, then run horch done",
            t.short(),
            t.status
        );
        Ok(Some(Outcome::fail("not landed", None, &why)))
    };
    if let Some(o) = still_mine(store)? {
        return Ok(o);
    }
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
    // Rebase and check outside the land lock; under it, fast-forward only
    // when the run branch did not move meanwhile (#160). When it moved, the
    // next try takes the lock first, so a land runs the check at most twice.
    let mut tries = 0;
    let mut held: Option<crate::fsx::DirLockGuard> = None;
    let (_lock, new) = loop {
        tries += 1;
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
        if let Some((_, why)) = check_in(cx, &dir, &log)? {
            return red(store, format!("after the rebase: {why}"));
        }
        let new = git.rev(&dir, "HEAD")?;
        if !git.is_ancestor(&cx.root, &target_before, &new) {
            return red(
                store,
                "the rebased branch does not contain the run branch".into(),
            );
        }
        let lock = match held.take() {
            Some(lock) => lock,
            None => land_lock(cx)?,
        };
        if git.rev(&cx.root, &run_branch)? == target_before {
            break (lock, new);
        }
        if tries >= MAX_LAND_TRIES {
            return red(
                store,
                format!("{run_branch} moved during each of {tries} checks; run finish again"),
            );
        }
        held = Some(lock);
    };
    // Under the land lock, just before the fast-forward: a cancel that
    // arrived during the rebase, the check or the lock wait lands nothing.
    if let Some(o) = still_mine(store)? {
        return Ok(o);
    }
    // Fast-forward the run branch: by merge when it is checked out in the
    // main checkout, else by a compare-and-swap of its ref. Never forced.
    let checked_out = git.current_branch(&cx.root).as_deref() == Some(run_branch.as_str());
    let target_before = git.rev(&cx.root, &run_branch)?;
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
        .with_context(|| format!("listing the commits of {} after its land", wt.branch))?;
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
    require_clean(git, &cx.root, true)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_land_that_waits_names_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let every = Duration::from_millis(50);
        let take = |holder: &str, timeout: Duration, notice: &mut dyn FnMut(&str)| {
            take_lock(dir.path(), "land-x", holder, every, timeout, |h| notice(h))
        };
        let mut held = Some(take("T-3 of w1 (PID 7)", every, &mut |_| {}).unwrap());
        // No time left: the wait gives up and names nobody.
        let mut seen = Vec::new();
        let r = take("T-4 of w2 (PID 8)", Duration::ZERO, &mut |h| {
            seen.push(h.to_string())
        });
        assert!(r.is_err(), "the lock is held");
        assert!(seen.is_empty(), "{seen:?}");
        // The holder lets go after the 2nd line. The long timeout is never
        // reached: the lines, not the clock, end the wait.
        let g = take("T-4 of w2 (PID 8)", Duration::from_secs(600), &mut |h| {
            seen.push(h.to_string());
            if seen.len() == 2 {
                held = None;
            }
        })
        .unwrap();
        assert_eq!(seen, vec!["T-3 of w1 (PID 7)"; 2], "1 line per wait");
        assert!(held.is_none());
        assert_eq!(
            std::fs::read_to_string(g.path().join("holder")).unwrap(),
            "T-4 of w2 (PID 8)\n"
        );
    }
}
