//! `horch ledger commit`: the 1 trusted way a sandboxed worker commits.
//!
//! A Codex worker runs `horch ledger ...` outside its sandbox (the
//! `["horch", "ledger"]` execpolicy rule), and no raw `git` command. So this
//! command must not let the worker steer git into running its own code: it
//! takes the worktree from the ledger, never from the caller, clears the git
//! environment, turns off every config hook that runs a program, and refuses
//! paths that leave the worktree or enter a nested repository, whose
//! `.git/config` the worker can write
//! (docs/live-checks/codex-git.md, T-10 decision 01b).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Args;
use horch_core::competition::preflight::parse_git_version;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::{self, WorktreeRow};
use horch_core::taskledger::model::{
    EventKind, ProgressSource, TaskStatus, WorkerStatus, WorktreeStatus,
};
use horch_core::taskledger::{self, tasks, workers, Store};

use super::taskledger::worker_id;
use crate::output;

#[derive(Args)]
pub struct CommitArgs {
    /// The commit message.
    #[arg(short = 'm', long = "message")]
    pub message: String,
    /// The files to stage and commit (after `--`). Default: every change.
    #[arg(last = true)]
    pub paths: Vec<PathBuf>,
}

/// Config pairs on every git call: no hook, no fsmonitor, no signing
/// program, no git started in a gitlink (it would read that repository's
/// worker-writable config), and no detached maintenance after a commit.
const SAFE_CONFIG: [&str; 7] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgsign=false",
    "gpg.program=/usr/bin/false",
    "diff.ignoreSubmodules=all",
    "submodule.recurse=false",
    "maintenance.auto=false",
];

/// git's empty tree id in a SHA-1 repository, and in a SHA-256 one.
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const EMPTY_TREE_SHA256: &str = "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321";

/// The most paths 1 commit takes: a bound on the `git status` and
/// `check-attr` output that horch reads outside the sandbox.
const MAX_PATHS: usize = 10_000;

/// The most bytes of 1 NUL-ended record of git output that horch reads: a
/// 3-byte status code and a path of `PATH_MAX` (4096) bytes.
const MAX_RECORD: usize = 4099;

/// The most bytes of 1 file, and of all files, that 1 commit stages: git
/// hashes them outside the sandbox. `len()` is the full length of a sparse
/// file too.
const MAX_FILE_BYTES: u64 = 100 << 20;
const MAX_TOTAL_BYTES: u64 = 1 << 30;

/// The longest 1 git child runs before horch kills it.
const GIT_DEADLINE: Duration = Duration::from_secs(120);

/// The most time a killed git child gets to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(1);

/// The most bytes of git's stderr kept for an error message.
const MAX_STDERR: u64 = 4096;

/// Plain data that names the author and committer; never a program.
const IDENTITY_VARS: [&str; 6] = [
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
];

pub fn run(ctx: &RuntimeContext, args: CommitArgs) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let mut store = taskledger::open(ctx)?;
    let (task, wt) = owned_worktree(&store, &wid)?;
    let root = std::fs::canonicalize(&wt.path)
        .with_context(|| format!("refused: the worktree {} does not exist", wt.path))?;
    let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
    if !cwd.starts_with(&root) {
        bail!(
            "refused: run horch ledger commit inside the worktree of {task}: {}",
            root.display()
        );
    }
    let paths = args
        .paths
        .iter()
        .map(|p| checked_path(&root, &cwd, p))
        .collect::<Result<Vec<_>>>()?;

    let repo_root = Path::new(&wt.repo_root);
    // The sandbox can write the worktree, the main checkout (a pane's
    // project root) and the state root: no git from there may run.
    let writable = [root.as_path(), repo_root, ctx.paths.state_root.as_path()];
    let bin = &trusted_git(&ctx.bins.harness.git, &writable)?;
    let (gitdir, tree) = trusted_gitdir(bin, repo_root, Path::new(&wt.path), &root)?;
    let git = Trusted::new(bin, &root, &gitdir, tree);
    let names = paths_to_stage(&git, &paths)?;
    refuse_large_files(&root, &names)?;
    refuse_required_filters(&git, names.clone())?;
    let id = commit(&git, &args.message, &paths, &names)?;
    // The commit exists now: print its id first, and never fail on the
    // bookkeeping after it, or a retry finds nothing to commit.
    output::println(&id);
    let short: String = id.chars().take(12).collect();
    let recorded = store.immediate(|tx| {
        workers::progress(
            tx,
            &wid,
            ProgressSource::Agent,
            EventKind::Progress,
            &format!("commit {short}"),
        )
    });
    if let Err(e) = recorded {
        eprintln!("horch: committed {short}, but the task ledger did not record it: {e:#}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The short name of the task this worker owns now, and its open worktree,
/// or a refusal. A reaped or requeued worker keeps its old `task_id`, so the
/// task must still name this worker, and a landed worktree takes no commit.
fn owned_worktree(store: &Store, wid: &str) -> Result<(String, WorktreeRow)> {
    let worker = workers::get(store.conn(), wid)?;
    let Some(task_id) = worker.task_id.clone() else {
        bail!("refused: worker {wid} has no claimed task. Run horch ledger claim first.");
    };
    let task = tasks::get(store.conn(), &task_id)?;
    let short = task.short();
    if task.is_review() {
        bail!("refused: {short} is a review task. A reviewer reports findings and never commits.");
    }
    if worker.status()? != WorkerStatus::Working || task.current_worker_id.as_deref() != Some(wid) {
        bail!("refused: worker {wid} does not own {short} now.");
    }
    if !matches!(task.status()?, TaskStatus::Running | TaskStatus::Landing) {
        bail!("refused: {short} is {}, not running.", task.status);
    }
    let Some(wt) = finish::worktree_of(store.conn(), &task_id)? else {
        bail!("refused: {short} has no worktree. Run horch ledger worktree new <name> first.");
    };
    if wt.status != WorktreeStatus::Open.as_str() {
        bail!(
            "refused: the worktree of {short} is {}, not open.",
            wt.status
        );
    }
    Ok((short, wt))
}

/// Stage and commit `paths`, or `names` (every change `git status` listed)
/// when there are none; the new commit id. git's own `index.lock` is held
/// for the whole command, so a second commit or a git command that writes
/// the index is refused and no index update is lost. Every file is made
/// with `create_new`, which never follows a planted link.
fn commit(git: &Trusted, message: &str, paths: &[PathBuf], names: &[OsString]) -> Result<String> {
    let lock = git.gitdir.join("index.lock");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|e| held_lock(&lock, e))?;
    // Names this command, so a lock that a killed run left is told apart.
    let _ = writeln!(file, "{LOCK_OWNER} {}", std::process::id());
    drop(file);
    let result = commit_locked(git, message, paths, names);
    if let Err(e) = std::fs::remove_file(&lock) {
        eprintln!(
            "horch: could not remove {}: {e}. horch ledger commit made it and has ended: \
             remove it, or every later git command in this worktree is refused.",
            lock.display()
        );
    }
    result
}

/// The first word of the `index.lock` that horch ledger commit makes; its
/// pid follows.
const LOCK_OWNER: &str = "horch ledger commit pid";

/// The refusal for an `index.lock` that exists. It names the lock, and
/// calls it stale when a horch ledger commit that has ended made it. horch
/// never removes it: git, in its own process group, can outlive horch.
fn held_lock(lock: &Path, e: std::io::Error) -> anyhow::Error {
    let owner = std::fs::read_to_string(lock).ok().and_then(|text| {
        text.trim()
            .strip_prefix(LOCK_OWNER)?
            .trim()
            .parse::<u32>()
            .ok()
    });
    match owner {
        Some(pid) if !is_running(pid) => anyhow::anyhow!(
            "refused: {} is stale: horch ledger commit pid {pid} made it and has ended. \
             Ask the orchestrator to remove it, then commit again.",
            lock.display()
        ),
        _ => anyhow::anyhow!(
            "refused: {} exists ({e}): another git command is using the index of this \
             worktree. If none is running, ask the orchestrator to remove it.",
            lock.display()
        ),
    }
}

/// False only when no process has `pid`.
fn is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the pid exists.
        let found = unsafe { libc::kill(pid, 0) } == 0;
        found || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// [`commit`] under the index lock. git stages into `index.horch`, which
/// replaces the index only when the commit succeeds: a failed commit leaves
/// the index as it was.
fn commit_locked(
    git: &Trusted,
    message: &str,
    paths: &[PathBuf],
    names: &[OsString],
) -> Result<String> {
    let index = git.gitdir.join("index");
    let scratch = git.gitdir.join("index.horch");
    let list = git.gitdir.join("index.horch.paths");
    // Files that a killed run left, with the lock of a git killed while it
    // wrote the copy: under the lock, nothing else writes them.
    let leftovers = [
        scratch.with_extension("horch.lock"),
        scratch.clone(),
        list.clone(),
    ];
    for file in &leftovers {
        let _ = std::fs::remove_file(file);
    }
    let staged = copy_index(&index, &scratch)
        .and_then(|()| stage_and_commit(&git.with_index(&scratch), message, paths, names, &list));
    let _ = std::fs::remove_file(&list);
    let id = match staged {
        Ok(id) => id,
        Err(e) => {
            let _ = std::fs::remove_file(&scratch);
            return Err(e);
        }
    };
    // The commit exists: a failed index update is a warning, or a retry
    // finds nothing to commit.
    if let Err(e) = std::fs::rename(&scratch, &index) {
        eprintln!(
            "horch: committed {id}, but could not update {}: {e}. The index still shows the old HEAD.",
            index.display()
        );
        let _ = std::fs::remove_file(&scratch);
    }
    Ok(id)
}

/// `names` as a new file at `list`, each name ended by NUL.
fn write_list(list: &Path, names: &[OsString]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(list)
        .with_context(|| format!("creating {}", list.display()))?;
    for name in names {
        file.write_all(name.as_encoded_bytes())
            .and_then(|()| file.write_all(b"\0"))
            .with_context(|| format!("writing {}", list.display()))?;
    }
    Ok(())
}

/// A copy of the index at `scratch`, a new file; nothing when there is no
/// index yet (git then starts an empty one).
fn copy_index(index: &Path, scratch: &Path) -> Result<()> {
    let mut source = match std::fs::File::open(index) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", index.display())),
    };
    let mut copy = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(scratch)
        .with_context(|| format!("creating {}", scratch.display()))?;
    std::io::copy(&mut source, &mut copy)
        .with_context(|| format!("copying {}", index.display()))?;
    Ok(())
}

/// Stage `paths`, or with none, exactly `names` through the NUL-ended list
/// file `list`: `git add -A` of the whole tree would start a git in each
/// gitlink, which runs that repository's filters.
fn stage_and_commit(
    git: &Trusted,
    message: &str,
    paths: &[PathBuf],
    names: &[OsString],
    list: &Path,
) -> Result<String> {
    let named = paths.iter().map(|p| p.as_os_str().to_owned());
    let mut add: Vec<OsString> = vec!["add".into()];
    // Quiet: the summary lists every file, past the bound of 1 record.
    let mut commit: Vec<OsString> = vec![
        "commit".into(),
        "--quiet".into(),
        "-m".into(),
        message.into(),
    ];
    if paths.is_empty() {
        write_list(list, names)?;
        let mut from = OsString::from("--pathspec-from-file=");
        from.push(list);
        add.extend(["-A".into(), from, "--pathspec-file-nul".into()]);
    } else {
        add.push("--".into());
        add.extend(named.clone());
        // Only the named files: paths an earlier attempt staged stay out.
        commit.push("--".into());
        commit.extend(named);
    }
    git.run(&add)?;
    git.run(&commit)?;
    git.run(&["rev-parse".into(), "HEAD".into()])
}

/// The paths git stages: `paths`, or every change `git status` lists when
/// there are none. At most [`MAX_PATHS`].
fn paths_to_stage(git: &Trusted, paths: &[PathBuf]) -> Result<Vec<OsString>> {
    let names = if paths.is_empty() {
        let names = changed_paths(git)?;
        refuse_nested_repos(&git.root, &names)?;
        names
    } else {
        paths.iter().map(|p| p.as_os_str().to_owned()).collect()
    };
    if names.len() > MAX_PATHS {
        bail!("{}", too_many_paths());
    }
    Ok(names)
}

/// Refused when `git status` lists a folder: with `--untracked-files=all`
/// that is an untracked nested repository, which `git add` stages as a
/// gitlink.
fn refuse_nested_repos(root: &Path, names: &[OsString]) -> Result<()> {
    for name in names {
        if std::fs::symlink_metadata(root.join(name)).is_ok_and(|m| m.is_dir()) {
            bail!(
                "refused: {} is a nested repository; horch ledger commit never stages \
                 one as a gitlink. Commit the other files with -- <files>.",
                Path::new(name).display()
            );
        }
    }
    Ok(())
}

/// Refused when a path to stage is longer than [`MAX_FILE_BYTES`], or all
/// of them than [`MAX_TOTAL_BYTES`]. A symlink counts as the link, which
/// git stores; a deleted path counts 0. The deadline on git bounds a file
/// that grows after this check.
fn refuse_large_files(root: &Path, names: &[OsString]) -> Result<()> {
    let mut total: u64 = 0;
    for name in names {
        let path = root.join(name);
        let len = match std::fs::symlink_metadata(&path) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if len > MAX_FILE_BYTES {
            bail!(
                "refused: {} is {len} bytes; horch ledger commit stages at most \
                 {MAX_FILE_BYTES} bytes per file. Ask the orchestrator for a Claude worker.",
                path.display()
            );
        }
        total = total.saturating_add(len);
        if total > MAX_TOTAL_BYTES {
            bail!(
                "refused: the paths to commit are more than {MAX_TOTAL_BYTES} bytes. \
                 Commit them in parts with -- <files>."
            );
        }
    }
    Ok(())
}

/// Refused when a path to commit has a required `filter` attribute (Git
/// LFS, for one): with attributes from the empty tree, git would store its
/// raw content.
fn refuse_required_filters(git: &Trusted, names: Vec<OsString>) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let required = required_filters(git)?;
    let refused: Vec<String> = path_filters(git, names)?
        .into_iter()
        .filter(|(_, filter)| required.contains(filter))
        .map(|(path, filter)| format!("{} (filter={filter})", path.to_string_lossy()))
        .collect();
    if !refused.is_empty() {
        bail!(
            "refused: horch ledger commit runs no git filters, so it cannot commit {}, \
             whose filter is required. Ask the orchestrator for a Claude worker to commit \
             these paths.",
            refused.join(", ")
        );
    }
    Ok(())
}

fn too_many_paths() -> String {
    format!(
        "refused: horch ledger commit takes at most {MAX_PATHS} paths (a rename or copy \
         counts 2). Commit them in parts with -- <files>."
    )
}

/// Every change `git status` lists, read as bytes: a name that is not UTF-8
/// stays the same name. git is stopped at the first record past the cap.
fn changed_paths(git: &Trusted) -> Result<Vec<OsString>> {
    let args: [OsString; 5] = [
        "status".into(),
        "--porcelain=v1".into(),
        "-z".into(),
        "--untracked-files=all".into(),
        "--ignore-submodules=all".into(),
    ];
    let out = run_git(git.command(clean_git(&git.bin, git.tree), &args), MAX_PATHS)?;
    if !out.complete {
        bail!("{}", too_many_paths());
    }
    if !out.status.success() {
        bail!("git status failed: {}", out.status);
    }
    let mut names = Vec::new();
    let mut records = out.records.iter().filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if let Some(path) = record.get(3..) {
            names.push(os_name(path));
        }
        // A rename or copy is followed by its old path.
        if record
            .get(..2)
            .is_some_and(|xy| xy.iter().any(|b| matches!(b, b'R' | b'C')))
        {
            records.next();
        }
    }
    Ok(names)
}

/// Each of `names` with its `filter` attribute, when it has one. Read with
/// `git check-attr`, which reads attributes and never runs a filter.
fn path_filters(git: &Trusted, names: Vec<OsString>) -> Result<Vec<(OsString, String)>> {
    let expected = 3 * names.len();
    let mut args: Vec<OsString> = vec![
        "check-attr".into(),
        "-z".into(),
        "filter".into(),
        "--".into(),
    ];
    args.extend(names.iter().cloned());
    let out = run_git(git.command(attribute_git(&git.bin), &args), expected)?;
    if !out.complete || !out.status.success() {
        bail!("git check-attr failed: {}", out.status);
    }
    filter_records(&out.records, names.len())
}

/// `git check-attr -z filter` output for `count` paths, as `(path, filter)`
/// for each path with a filter. Refused unless it is exactly `count`
/// triples of `<path>, filter, <value>`: a path left out would go unchecked.
fn filter_records(records: &[Vec<u8>], count: usize) -> Result<Vec<(OsString, String)>> {
    if records.len() != 3 * count {
        bail!(
            "refused: git check-attr printed {} records for {count} paths, not {}",
            records.len(),
            3 * count
        );
    }
    let mut filters = Vec::new();
    for triple in records.as_chunks::<3>().0 {
        if triple[1] != b"filter" {
            bail!("refused: unexpected git check-attr output {triple:?}");
        }
        let value = String::from_utf8_lossy(&triple[2]).into_owned();
        if !matches!(value.as_str(), "unspecified" | "unset") {
            filters.push((os_name(&triple[0]), value));
        }
    }
    Ok(filters)
}

/// The filters whose `filter.<name>.required` is true: git itself fails
/// without them, and `git lfs install` sets `filter.lfs.required` in the
/// operator's global config. The repository config wins over the global
/// one. 1 lookup per config, not per path or filter. A failed lookup or a
/// value that is not a boolean refuses the commit: horch never guesses.
fn required_filters(git: &Trusted) -> Result<HashSet<String>> {
    const QUERY: [&str; 4] = [
        "--null",
        "--type=bool",
        "--get-regexp",
        r"^filter\..*\.required$",
    ];
    let mut global = global_git(&git.bin);
    global.args(["config", "--global"]).args(QUERY);
    let mut values = config_values(global, "global")?;
    let mut args: Vec<OsString> = vec!["config".into()];
    args.extend(QUERY.iter().map(OsString::from));
    let repo = git.command(clean_git(&git.bin, git.tree), &args);
    values.extend(config_values(repo, "repository")?);
    Ok(values
        .into_iter()
        .filter(|(_, required)| *required)
        .map(|(name, _)| name)
        .collect())
}

/// Filter name to `required`, from 1 `git config --get-regexp` run. Exit 1
/// with no output means the config sets none.
fn config_values(cmd: Command, scope: &str) -> Result<HashMap<String, bool>> {
    let out = run_git(cmd, MAX_PATHS)?;
    match out.status.code() {
        Some(0) if out.complete => {}
        Some(1) if out.records.is_empty() => return Ok(HashMap::new()),
        _ => bail!(
            "refused: cannot read filter.<name>.required from the {scope} git config ({}). \
             horch ledger commit does not guess whether a filter is required.",
            out.status
        ),
    }
    out.records.iter().map(|r| config_entry(r, scope)).collect()
}

/// `filter.<name>.required\n<true|false>` as `(name, required)`.
fn config_entry(record: &[u8], scope: &str) -> Result<(String, bool)> {
    let text = String::from_utf8_lossy(record);
    let entry = text.split_once('\n').and_then(|(key, value)| {
        let name = key.strip_prefix("filter.")?.strip_suffix(".required")?;
        let required = match value {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        Some((name.to_string(), required))
    });
    entry.with_context(|| format!("refused: unreadable {scope} git config entry {text:?}"))
}

/// The NUL-ended stdout records of 1 git run, and how it ended.
#[derive(Debug)]
struct Records {
    status: ExitStatus,
    records: Vec<Vec<u8>>,
    /// False when git printed more than the cap: it was stopped there.
    complete: bool,
    /// The first [`MAX_STDERR`] bytes of stderr.
    stderr: Vec<u8>,
}

impl Records {
    /// stdout of a git call that prints at most 1 record, or its error.
    fn text(self, what: &str) -> Result<String> {
        if !self.complete || !self.status.success() {
            let stderr = String::from_utf8_lossy(&self.stderr);
            let why = if stderr.trim().is_empty() {
                self.status.to_string()
            } else {
                stderr.trim().to_string()
            };
            bail!("git {what} failed: {why}");
        }
        Ok(String::from_utf8_lossy(&self.records.concat())
            .trim()
            .to_string())
    }
}

/// The 1 way horch ledger commit runs git: [`run_until`] with
/// [`GIT_DEADLINE`].
fn run_git(cmd: Command, max: usize) -> Result<Records> {
    run_until(cmd, max, GIT_DEADLINE)
}

/// Run `cmd` and read at most `max` NUL-ended stdout records of at most
/// [`MAX_RECORD`] bytes each. git is killed at the first record past either
/// bound, and at `limit`, which is an error. So no worker-controlled input
/// grows horch's memory or time past them.
fn run_until(mut cmd: Command, max: usize, limit: Duration) -> Result<Records> {
    let end = Instant::now() + limit;
    // git leads its own process group, so the kill also reaches a filter or
    // helper that holds a pipe.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git")?;
    let stdout = child.stdout.take().context("git has no stdout")?;
    let stderr = child.stderr.take().context("git has no stderr")?;
    let mut reader = Some(std::thread::spawn(move || {
        read_records(BufReader::new(stdout), max)
    }));
    let errors = std::thread::spawn(move || read_stderr(stderr));
    let mut read = None;
    let status = loop {
        if let Some(status) = child.try_wait().context("waiting for git")? {
            break status;
        }
        if reader.as_ref().is_some_and(JoinHandle::is_finished) {
            read = reader.take().and_then(|h| h.join().ok());
        }
        let capped = read.as_ref().is_some_and(|r| !matches!(r, Ok((_, true))));
        let late = Instant::now() >= end;
        if capped || late {
            let status = kill_and_reap(&mut child)?;
            if !capped {
                bail!(
                    "git ran longer than {} s and was stopped",
                    limit.as_secs_f32()
                );
            }
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let read = match read {
        Some(read) => read,
        None => reader.and_then(|h| join_by(h, end)).with_context(|| {
            format!("git output was still open after {} s", limit.as_secs_f32())
        })?,
    };
    let (records, complete) = read?;
    Ok(Records {
        status,
        records,
        complete,
        stderr: join_by(errors, end).unwrap_or_default(),
    })
}

/// Stop git and its process group, then reap git: TERM first, so git
/// removes its lock files, and KILL when git is not reaped within
/// [`REAP_GRACE`]. A git that is not reaped after the KILL either is an
/// error: no wait here blocks, and the whole stop takes at most 2 graces.
fn kill_and_reap(child: &mut Child) -> Result<ExitStatus> {
    #[cfg(unix)]
    {
        signal_group(child, libc::SIGTERM);
        if let Ok(status) = reap_within(child, REAP_GRACE) {
            // A helper that ignored the TERM still holds the group id, so
            // no new process can have it yet.
            signal_group(child, libc::SIGKILL);
            return Ok(status);
        }
    }
    #[cfg(unix)]
    let killed = signal_group(child, libc::SIGKILL);
    #[cfg(not(unix))]
    let killed = child.kill().is_ok();
    let kill_error = (!killed).then(std::io::Error::last_os_error);
    reap_within(child, REAP_GRACE).with_context(|| match &kill_error {
        Some(e) => format!("could not kill git: {e}"),
        None => "git was killed".to_string(),
    })
}

/// Send `signal` to the process group that `child` leads; true when sent.
/// git is unreaped when this is called first, so its pid cannot belong to
/// another process: the group signal reaches only this call's descendants.
#[cfg(unix)]
fn signal_group(child: &Child, signal: libc::c_int) -> bool {
    // SAFETY: `kill` only signals; the group is ours (see above).
    unsafe { libc::kill(-(child.id() as libc::pid_t), signal) == 0 }
}

/// Reap `child` by polling, never by a blocking wait: a child that does not
/// exit within `grace` is an error, so the call still returns.
fn reap_within(child: &mut Child, grace: Duration) -> Result<ExitStatus> {
    let until = Instant::now() + grace;
    loop {
        if let Some(status) = child.try_wait().context("waiting for git")? {
            return Ok(status);
        }
        if Instant::now() >= until {
            bail!("git did not exit within {} ms", grace.as_millis());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The result of `handle`, or `None` when it is not done at `end`.
fn join_by<T>(handle: JoinHandle<T>, end: Instant) -> Option<T> {
    while !handle.is_finished() {
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.join().ok()
}

/// The first [`MAX_STDERR`] bytes of `stderr`; the rest is read and dropped,
/// so git never blocks on a full pipe.
fn read_stderr(mut stderr: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let _ = stderr.by_ref().take(MAX_STDERR).read_to_end(&mut kept);
    let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    kept
}

/// At most `max` NUL-ended records from `reader`, and whether it ended
/// within them. A record longer than [`MAX_RECORD`] is an error.
fn read_records(mut reader: impl BufRead, max: usize) -> Result<(Vec<Vec<u8>>, bool)> {
    let mut records = Vec::new();
    loop {
        let mut record = Vec::new();
        reader
            .by_ref()
            .take(MAX_RECORD as u64 + 1)
            .read_until(0, &mut record)
            .context("reading git output")?;
        if record.is_empty() {
            return Ok((records, true));
        }
        if record.last() == Some(&0) {
            record.pop();
        } else if record.len() > MAX_RECORD {
            bail!("refused: git printed a record longer than {MAX_RECORD} bytes");
        }
        if records.len() == max {
            return Ok((records, false));
        }
        records.push(record);
    }
}

/// The worktree's git dir, from trusted metadata only: the 1 entry under
/// the common dir (from `git rev-parse` in the main checkout the ledger
/// recorded) whose `gitdir` file names the recorded worktree. Not its folder
/// name: git appends a number when that name is taken. Refused unless the
/// worktree's `.git` is a file that says exactly `gitdir: <that entry>`.
/// Also returns the empty tree id of the repository's object format.
fn trusted_gitdir(
    bin: &Path,
    repo_root: &Path,
    worktree: &Path,
    root: &Path,
) -> Result<(PathBuf, &'static str)> {
    let (common, tree) = repo_metadata(bin, repo_root)?;
    let gitdir = worktree_entry(&common, worktree, root)?;
    check_dotgit(root, &gitdir)?;
    Ok((gitdir, tree))
}

/// The common git dir of the main checkout `repo_root`, and the empty tree
/// id of its object format.
fn repo_metadata(bin: &Path, repo_root: &Path) -> Result<(PathBuf, &'static str)> {
    // The attribute source is read only when attributes are: a SHA-1 id is
    // safe here in a SHA-256 repository too.
    let mut cmd = clean_git(bin, EMPTY_TREE_SHA1);
    cmd.arg("-C").arg(repo_root);
    for pair in SAFE_CONFIG {
        cmd.args(["-c", pair]);
    }
    cmd.args([
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
        "--show-object-format",
    ]);
    let text = run_git(cmd, 1)?
        .text("rev-parse")
        .with_context(|| format!("refused: {} is not a git checkout", repo_root.display()))?;
    let mut lines = text.lines();
    let common = PathBuf::from(lines.next().unwrap_or_default().trim());
    let tree = match lines.next().map(str::trim) {
        Some("sha1") => EMPTY_TREE_SHA1,
        Some("sha256") => EMPTY_TREE_SHA256,
        other => bail!(
            "refused: {} has an unknown object format {other:?}",
            repo_root.display()
        ),
    };
    Ok((common, tree))
}

/// The 1 entry under `<common>/worktrees` whose `gitdir` file names the
/// recorded worktree.
fn worktree_entry(common: &Path, worktree: &Path, root: &Path) -> Result<PathBuf> {
    let recorded = format!("{}/.git", worktree.display());
    let names_worktree = |entry: &Path| {
        std::fs::read_to_string(entry.join("gitdir")).is_ok_and(|t| {
            let t = t.trim();
            // With `worktree.useRelativePaths`, git writes the path relative
            // to the entry; `join` keeps an absolute path as it is.
            t == recorded
                || std::fs::canonicalize(entry.join(t)).is_ok_and(|p| p == root.join(".git"))
        })
    };
    let matches: Vec<PathBuf> = std::fs::read_dir(common.join("worktrees"))
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|e| names_worktree(e))
                .collect()
        })
        .unwrap_or_default();
    let [gitdir] = matches.as_slice() else {
        bail!(
            "refused: {} entries under {} name the worktree {}; expected exactly 1",
            matches.len(),
            common.join("worktrees").display(),
            worktree.display()
        );
    };
    Ok(gitdir.clone())
}

/// Refused unless the worktree's `.git` is a file that says exactly
/// `gitdir: <gitdir>`, or a relative path from the worktree to it (git
/// writes that with `worktree.useRelativePaths`).
fn check_dotgit(root: &Path, gitdir: &Path) -> Result<()> {
    let dotgit = root.join(".git");
    let text = std::fs::symlink_metadata(&dotgit)
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| std::fs::read_to_string(&dotgit).ok())
        .unwrap_or_default();
    let line = text.strip_suffix('\n').unwrap_or(&text);
    // git may have written the canonical or the given form of the path.
    let canonical = std::fs::canonicalize(gitdir).unwrap_or_else(|_| gitdir.to_path_buf());
    if [gitdir, &canonical]
        .iter()
        .any(|p| line == format!("gitdir: {}", p.display()))
    {
        return Ok(());
    }
    let relative = line.strip_prefix("gitdir: ").map(Path::new);
    if relative.is_some_and(|r| {
        r.is_relative() && std::fs::canonicalize(root.join(r)).is_ok_and(|p| p == canonical)
    }) {
        return Ok(());
    }
    bail!(
        "refused: {} was changed; it must say exactly: gitdir: {}",
        dotgit.display(),
        gitdir.display()
    )
}

/// Absolute git paths `horch ledger commit` tries in order when
/// `HORCH_GIT_BIN` is not absolute. Never a lookup through `PATH`: a
/// worker-writable folder on it would run a fake git outside the sandbox.
const TRUSTED_GITS: [&str; 3] = [
    "/usr/bin/git",
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
];

/// The git to run: `configured` when absolute, else the first of
/// [`TRUSTED_GITS`] at 2.41 or later. Refused inside a `writable` folder
/// (the sandbox writes each one), and below 2.41, which has no
/// `GIT_ATTR_SOURCE`, so a `.gitattributes` file could select a filter.
fn trusted_git(configured: &Path, writable: &[&Path]) -> Result<PathBuf> {
    let candidates: Vec<PathBuf> = if configured.is_absolute() {
        vec![configured.to_path_buf()]
    } else {
        TRUSTED_GITS
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect()
    };
    let writable: Vec<PathBuf> = writable
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .collect();
    let mut too_old = Vec::new();
    for candidate in candidates {
        let real = std::fs::canonicalize(&candidate)
            .with_context(|| format!("refused: no git at {}", candidate.display()))?;
        if writable.iter().any(|w| real.starts_with(w)) {
            bail!(
                "refused: git {} is inside the worktree, the main checkout or the state root",
                real.display()
            );
        }
        match git_version(&real) {
            Some(v) if v >= (2, 41) => return Ok(real),
            v => too_old.push(format!("{} ({v:?})", real.display())),
        }
    }
    bail!(
        "refused: horch ledger commit needs git 2.41 or later, at HORCH_GIT_BIN or \
         one of {}: older git has no attribute source, so a worker's .gitattributes \
         could select a filter that runs outside the sandbox. Found: {}",
        TRUSTED_GITS.join(", "),
        if too_old.is_empty() {
            "none".to_string()
        } else {
            too_old.join(", ")
        }
    )
}

/// The git `horch ledger commit` would run, for `horch doctor`: refused when
/// there is no git 2.41 or later at `HORCH_GIT_BIN` or a [`TRUSTED_GITS`] path,
/// or when it is inside the main checkout (which holds every ledger worktree)
/// or the state root.
pub fn commit_git(ctx: &RuntimeContext) -> Result<PathBuf> {
    let project = ctx.paths.project().ok();
    let mut writable = vec![ctx.paths.state_root.as_path()];
    writable.extend(project.as_deref());
    trusted_git(&ctx.bins.harness.git, &writable)
}

/// `(major, minor)` of the git at `bin`, run with a clean environment.
fn git_version(bin: &Path) -> Option<(u32, u32)> {
    let mut cmd = git_without_env(bin, Some(EMPTY_TREE_SHA1));
    cmd.arg("--version");
    let text = run_git(cmd, 1).ok()?.text("--version").ok()?;
    parse_git_version(&text)
}

/// `arg` as an absolute path inside `root`, or a refusal. Symlinks in the
/// part that exists are resolved before the check.
fn checked_path(root: &Path, cwd: &Path, arg: &Path) -> Result<PathBuf> {
    let joined = normalize(&cwd.join(arg));
    let resolved = resolve_existing(&joined);
    // git gets `joined`, so a symlink is committed as the link itself; both
    // it and its target must pass every check.
    for path in [&joined, &resolved] {
        check_inside(root, arg, path)?;
    }
    // git stages a folder's files without a check of their attributes.
    if std::fs::symlink_metadata(&joined).is_ok_and(|m| m.is_dir()) {
        bail!("refused: {} is a folder; name its files", arg.display());
    }
    Ok(joined)
}

fn check_inside(root: &Path, arg: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "refused: {} is outside the worktree {}",
            arg.display(),
            root.display()
        );
    }
    let inside = path.strip_prefix(root).unwrap_or(Path::new(""));
    if inside.components().any(|c| c.as_os_str() == ".git") {
        bail!("refused: {} is inside git metadata", arg.display());
    }
    // A folder below the root that holds its own `.git` is a nested
    // repository: git would read its config, which the worker can write.
    let mut dir = Some(path);
    while let Some(d) = dir {
        if d == root {
            break;
        }
        if d.join(".git").exists() {
            bail!(
                "refused: {} is in the nested repository {}",
                arg.display(),
                d.display()
            );
        }
        dir = d.parent();
    }
    Ok(())
}

/// `.` and `..` removed without touching the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The canonical form of the longest existing prefix of `p`, with the rest
/// appended: a path to a file that is not there yet (or was deleted) still
/// resolves its folders.
fn resolve_existing(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut head = p.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&head) {
            return rest.iter().rev().fold(c, |acc, part| acc.join(part));
        }
        match (head.file_name().map(|f| f.to_owned()), head.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                head = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// git in the recorded worktree with a cleared environment.
#[derive(Clone)]
struct Trusted {
    bin: PathBuf,
    root: PathBuf,
    gitdir: PathBuf,
    tree: &'static str,
    identity: Vec<(String, String)>,
    /// The index file to use instead of the git dir's own.
    index: Option<PathBuf>,
}

impl Trusted {
    fn new(bin: &Path, root: &Path, gitdir: &Path, tree: &'static str) -> Trusted {
        let mut git = Trusted {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
            gitdir: gitdir.to_path_buf(),
            tree,
            identity: Vec::new(),
            index: None,
        };
        // `-c` wins over the repository config: pass only what it lacks.
        git.identity = global_identity(bin)
            .into_iter()
            .filter(|(key, _)| !git.repo_sets(key))
            .collect();
        git
    }

    /// True when the repository config (with the worktree's) sets `key`.
    fn repo_sets(&self, key: &str) -> bool {
        let args = ["config".into(), "--get".into(), OsString::from(key)];
        let out = run_git(self.command(clean_git(&self.bin, self.tree), &args), 1);
        out.is_ok_and(|o| o.text("config").is_ok_and(|v| !v.is_empty()))
    }

    /// This git, with `index` as its index file.
    fn with_index(&self, index: &Path) -> Trusted {
        Trusted {
            index: Some(index.to_path_buf()),
            ..self.clone()
        }
    }

    /// git `args` with attributes from the empty tree; its trimmed stdout.
    fn run(&self, args: &[OsString]) -> Result<String> {
        let what = args
            .first()
            .map(|a| a.to_string_lossy().into_owned())
            .unwrap_or_default();
        run_git(self.command(clean_git(&self.bin, self.tree), args), 1)
            .with_context(|| format!("running {}", self.bin.display()))?
            .text(&what)
    }

    /// `cmd` with `args`, pinned to the recorded worktree and git dir.
    fn command(&self, mut cmd: Command, args: &[OsString]) -> Command {
        // The git dir is named, never discovered: discovery would follow the
        // worktree's `.git` file, which the sandbox can rewrite.
        cmd.arg("-C")
            .arg(&self.root)
            .env("GIT_DIR", &self.gitdir)
            .env("GIT_WORK_TREE", &self.root);
        if let Some(index) = &self.index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        for pair in SAFE_CONFIG {
            cmd.args(["-c", pair]);
        }
        for (key, value) in &self.identity {
            cmd.arg("-c").arg(format!("{key}={value}"));
        }
        for var in IDENTITY_VARS {
            if let Some(v) = std::env::var_os(var) {
                cmd.env(var, v);
            }
        }
        cmd.args(args);
        cmd
    }
}

/// A file name from git's `-z` output, byte for byte on Unix.
#[cfg(unix)]
fn os_name(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).to_owned()
}

#[cfg(not(unix))]
fn os_name(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

/// A git command with every `GIT_*` variable removed, no system config and
/// no global config, and attributes from the empty tree `tree`.
fn clean_git(bin: &Path, tree: &str) -> Command {
    let mut cmd = git_without_env(bin, Some(tree));
    // Attributes come from the empty tree: no `.gitattributes` file selects
    // a filter.
    cmd.env("GIT_ATTR_SOURCE", tree);
    pinned_env(&mut cmd);
    cmd
}

/// [`clean_git`] that reads the worktree's attributes: for `check-attr`.
fn attribute_git(bin: &Path) -> Command {
    let mut cmd = git_without_env(bin, None);
    pinned_env(&mut cmd);
    cmd
}

fn pinned_env(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        // A path is a path: no glob or magic pathspec stages files that the
        // filter check never saw.
        .env("GIT_LITERAL_PATHSPECS", "1");
}

/// `user.name` and `user.email` from the operator's global config, read as
/// values only: with the global config off, a commit keeps its author.
fn global_identity(bin: &Path) -> Vec<(String, String)> {
    ["user.name", "user.email"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), global_config(bin, key)?)))
        .collect()
}

/// 1 value from the operator's global config, read as a value only.
fn global_config(bin: &Path, key: &str) -> Option<String> {
    let mut cmd = global_git(bin);
    cmd.args(["config", "--global", "--get", key]);
    let value = run_git(cmd, 1).ok()?.text("config").ok()?;
    (!value.is_empty()).then_some(value)
}

/// git outside any repository that reads the operator's global config only.
fn global_git(bin: &Path) -> Command {
    let mut cmd = git_without_env(bin, Some(EMPTY_TREE_SHA1));
    cmd.current_dir("/").env("GIT_CONFIG_NOSYSTEM", "1");
    cmd
}

/// A git command with every `GIT_*` variable removed, and attributes from
/// the empty tree `tree` as a global option. Fail-closed: a git that does
/// not know `--attr-source` exits with an error, where it would ignore
/// `GIT_ATTR_SOURCE` and let a `.gitattributes` file select a filter.
fn git_without_env(bin: &Path, tree: Option<&str>) -> Command {
    let mut cmd = Command::new(bin);
    if let Some(tree) = tree {
        cmd.arg(format!("--attr-source={tree}"));
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env_remove("ANTHROPIC_API_KEY")
        .env("PATH", "/usr/bin:/bin");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn records_end_within_the_cap() {
        let (records, complete) = read_records(Cursor::new(b"a\0bb\0c".to_vec()), 3).unwrap();
        assert!(complete);
        assert_eq!(records, vec![b"a".to_vec(), b"bb".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn reading_stops_at_the_first_record_past_the_cap() {
        let input: Vec<u8> = (0..50)
            .flat_map(|n| format!("{n}\0").into_bytes())
            .collect();
        let mut reader = Cursor::new(input);
        let (records, complete) = read_records(&mut reader, 10).unwrap();
        assert!(!complete);
        assert_eq!(records.len(), 10);
        // Only 1 record past the cap was read: the rest stays unread.
        assert_eq!(
            reader.position(),
            b"0\x001\x002\x003\x004\x005\x006\x007\x008\x009\x0010\0".len() as u64
        );
    }

    #[test]
    fn a_record_longer_than_the_bound_is_refused_before_it_is_stored() {
        let mut long = vec![b'x'; MAX_RECORD + 1];
        long.push(0);
        let mut reader = Cursor::new(long);
        let e = read_records(&mut reader, 10).unwrap_err();
        assert!(e.to_string().contains("longer than 4099 bytes"), "{e}");
        assert_eq!(reader.position(), MAX_RECORD as u64 + 1);
        let mut exact = vec![b'x'; MAX_RECORD];
        exact.push(0);
        let (records, complete) = read_records(Cursor::new(exact), 10).unwrap();
        assert!(complete && records[0].len() == MAX_RECORD);
    }

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_the_deadline_is_killed_and_is_an_error() {
        let started = Instant::now();
        let e = run_until(sh("exec sleep 30"), 1, Duration::from_millis(300)).unwrap_err();
        assert!(e.to_string().contains("was stopped"), "{e}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_the_record_cap_is_killed_at_once() {
        let started = Instant::now();
        let out = run_until(
            sh("exec yes x | tr '\\n' '\\0'"),
            10,
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(!out.complete);
        assert_eq!(out.records.len(), 10);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_within_the_bounds_keeps_its_output_and_stderr() {
        let out = run_until(sh("printf 'a\\0b'; echo oops >&2; exit 3"), 5, GIT_DEADLINE).unwrap();
        assert!(out.complete);
        assert_eq!(out.records, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(out.status.code(), Some(3));
        let e = out.text("x").unwrap_err();
        assert!(e.to_string().contains("oops"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn reaping_a_child_that_does_not_exit_returns_inside_the_grace() {
        let mut cmd = sh("exec sleep 30");
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().unwrap();
        let started = Instant::now();
        let e = reap_within(&mut child, Duration::from_millis(200)).unwrap_err();
        assert!(e.to_string().contains("did not exit within 200 ms"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(kill_and_reap(&mut child).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn the_deadline_kill_also_ends_a_helper_that_git_started() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("helper.pid");
        let script = format!(
            "sleep 30 & echo $! > '{}'; exec sleep 30",
            pid_file.display()
        );
        let started = Instant::now();
        run_until(sh(&script), 1, Duration::from_millis(300)).unwrap_err();
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The helper is reparented and reaped once it is dead.
        // SAFETY: signal 0 only checks that the pid exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "helper {pid} survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn check_attr_output_of_the_wrong_shape_is_refused() {
        let rec = |parts: &[&str]| {
            parts
                .iter()
                .map(|p| p.as_bytes().to_vec())
                .collect::<Vec<_>>()
        };
        let good = rec(&["a", "filter", "lfs", "b", "filter", "unspecified"]);
        assert_eq!(
            filter_records(&good, 2).unwrap(),
            vec![(OsString::from("a"), "lfs".to_string())]
        );
        // A short tail is refused, not dropped.
        assert!(filter_records(&good[..5], 2).is_err());
        assert!(filter_records(&good, 3).is_err());
        let wrong = rec(&["a", "diff", "lfs"]);
        assert!(filter_records(&wrong, 1).is_err());
    }

    #[test]
    fn config_entries_parse_or_refuse() {
        assert_eq!(
            config_entry(b"filter.lfs.required\ntrue", "global").unwrap(),
            ("lfs".to_string(), true)
        );
        assert_eq!(
            config_entry(b"filter.a.b.required\nfalse", "global").unwrap(),
            ("a.b".to_string(), false)
        );
        for bad in [
            &b"filter.lfs.required"[..],
            b"filter.lfs.required\nmaybe",
            b"core.x\ntrue",
        ] {
            assert!(config_entry(bad, "global").is_err(), "{bad:?}");
        }
    }
}
