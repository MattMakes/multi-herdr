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
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};
use clap::Args;
use horch_core::competition::preflight::parse_git_version;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::bounded::{run_records, run_until, Records, GIT_DEADLINE};
use horch_core::taskledger::finish::{self, WorktreeRow};
use horch_core::taskledger::git::{
    filter_driver_names, filter_overrides, worktree_gitdir, SAFE_CONFIG,
};
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

/// git's empty tree id in a SHA-1 repository, and in a SHA-256 one.
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const EMPTY_TREE_SHA256: &str = "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321";

/// The most paths 1 commit takes: a bound on the `git status` and
/// `check-attr` output that horch reads outside the sandbox.
const MAX_PATHS: usize = 10_000;

/// The most bytes of 1 file, and of all files, that 1 commit stages: git
/// hashes them outside the sandbox. `len()` is the full length of a sparse
/// file too.
const MAX_FILE_BYTES: u64 = 100 << 20;
const MAX_TOTAL_BYTES: u64 = 1 << 30;

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
    // Before any per-path work: a path named twice counts twice.
    if args.paths.len() > MAX_PATHS {
        bail!("{}", too_many_paths());
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
    let git = Trusted::new(bin, &root, &gitdir, tree)?;
    let stage = paths_to_stage(&git, &paths)?;
    refuse_large_files(&root, &stage.add)?;
    refuse_required_filters(&git, &stage.add, Attributes::Worktree)?;
    refuse_staging_filters(&git, &stage.add)?;
    let id = commit(&git, &args.message, &stage)?;
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

/// Stage and commit `stage`; the new commit id. git's own `index.lock` is
/// held for the whole command, so a second commit or a git command that
/// writes the index is refused and no index update is lost. Every file is
/// made with `create_new`, which never follows a planted link.
fn commit(git: &Trusted, message: &str, stage: &Stage) -> Result<String> {
    let lock = git.gitdir.join("index.lock");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|e| held_lock(&lock, e))?;
    // Names this command, so a lock that a killed run left is told apart.
    let pid = std::process::id();
    let started = horch_core::procid::start_time(pid).unwrap_or(0);
    let _ = writeln!(file, "{LOCK_OWNER} {pid} {started}");
    drop(file);
    let result = commit_locked(git, message, stage);
    if let Err(e) = std::fs::remove_file(&lock) {
        eprintln!(
            "horch: could not remove {}: {e}. horch ledger commit made it and has ended: \
             remove it, or every later git command in this worktree is refused.",
            lock.display()
        );
    }
    result
}

/// The first words of the `index.lock` that horch ledger commit makes; its
/// pid and the pid's start time follow.
const LOCK_OWNER: &str = "horch ledger commit pid";

/// The refusal for an `index.lock` that exists. It names the lock, and
/// calls it stale when a horch ledger commit that has ended made it. horch
/// never removes it: git, in its own process group, can outlive horch.
fn held_lock(lock: &Path, e: std::io::Error) -> anyhow::Error {
    let owner = std::fs::read_to_string(lock).ok().and_then(|text| {
        let mut words = text.trim().strip_prefix(LOCK_OWNER)?.split_whitespace();
        let pid = words.next()?.parse::<u32>().ok()?;
        // None or 0: the start time was unreadable when the lock was made.
        let started = words
            .next()
            .and_then(|t| t.parse::<u64>().ok())
            .filter(|t| *t != 0);
        Some((pid, started))
    });
    match owner {
        // A reused pid has another start time: the lock is still stale.
        Some((pid, started)) if !horch_core::procid::alive(pid, started) => anyhow::anyhow!(
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

/// [`commit`] under the index lock. git stages into `index.horch`, which
/// replaces the index only when the commit succeeds: a failed commit leaves
/// the index as it was.
fn commit_locked(git: &Trusted, message: &str, stage: &Stage) -> Result<String> {
    let index = git.gitdir.join("index");
    let scratch = git.gitdir.join("index.horch");
    let next = git.gitdir.join("index.horch.next");
    let list = git.gitdir.join("index.horch.paths");
    // Files that a killed run left, with the lock of a git killed while it
    // wrote the copy: under the lock, nothing else writes them.
    let leftovers = [
        scratch.with_extension("horch.lock"),
        next.with_extension("next.lock"),
        scratch.clone(),
        next.clone(),
        list.clone(),
    ];
    for file in &leftovers {
        let _ = std::fs::remove_file(file);
    }
    let staged = write_list(&list, &stage.add)
        .and_then(|()| stage_and_commit(git, &scratch, &index, &list, message, stage));
    let id = match staged {
        Ok(id) => id,
        Err(e) => {
            let _ = std::fs::remove_file(&scratch);
            let _ = std::fs::remove_file(&list);
            return Err(e);
        }
    };
    // The commit exists: a failed index update is a warning, or a retry
    // finds nothing to commit.
    let updated = if stage.only {
        // The index keeps what was staged before, with the committed paths
        // set to the new HEAD: read from the commit, never the worktree.
        copy_index(&index, &next).and_then(|()| {
            let mut from = OsString::from("--pathspec-from-file=");
            from.push(&list);
            let args = ["reset".into(), "--quiet".into(), "HEAD".into(), from];
            git.with_index(&next)
                .run(&[&args[..], &["--pathspec-file-nul".into()]].concat())
                .map(drop)
        })
    } else {
        Ok(())
    };
    let _ = std::fs::remove_file(&list);
    let new_index = if stage.only { &next } else { &scratch };
    let result = updated.and_then(|()| {
        std::fs::rename(new_index, &index)
            .with_context(|| format!("renaming {}", new_index.display()))
    });
    if let Err(e) = result {
        eprintln!(
            "horch: committed {id}, but could not update {}: {e:#}. The index still shows the old HEAD.",
            index.display()
        );
    }
    let _ = std::fs::remove_file(&scratch);
    let _ = std::fs::remove_file(&next);
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
    let source = std::fs::File::open(index);
    if source
        .as_ref()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    let mut source = source.with_context(|| format!("reading {}", index.display()))?;
    let mut copy = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(scratch)
        .with_context(|| format!("creating {}", scratch.display()))?;
    std::io::copy(&mut source, &mut copy)
        .with_context(|| format!("copying {}", index.display()))?;
    Ok(())
}

/// Stage exactly `stage.add` (listed in the NUL-ended file `list`) into
/// `scratch`, check what is staged, and commit `scratch` with no pathspec:
/// a pathspec on `git commit` would stage from the worktree again, after
/// the checks. `git add -A` with no pathspec stages the whole tree and starts
/// a git in each gitlink, so with no path to add, git add does not run.
fn stage_and_commit(
    git: &Trusted,
    scratch: &Path,
    index: &Path,
    list: &Path,
    message: &str,
    stage: &Stage,
) -> Result<String> {
    // A ledger worktree always has a HEAD: a failed lookup is an error,
    // never an empty base that would commit the deletion of every file.
    let head = git
        .run(&[
            "rev-parse".into(),
            "--verify".into(),
            "--quiet".into(),
            "HEAD^{commit}".into(),
        ])
        .context("refused: cannot read the HEAD commit of the worktree")?;
    let staging = git.with_index(scratch);
    if stage.only {
        // Only the named paths, on top of HEAD: an earlier attempt's
        // staged paths stay out of this commit.
        staging.run(&["read-tree".into(), head.as_str().into()])?;
    } else {
        copy_index(index, scratch)?;
    }
    if !stage.add.is_empty() {
        let mut from = OsString::from("--pathspec-from-file=");
        from.push(list);
        staging.run(&[
            "add".into(),
            "-A".into(),
            from,
            "--pathspec-file-nul".into(),
        ])?;
    }
    let blobs = refuse_unchecked_entries(&staging, &head, &stage.allowed)?;
    // The checks before git add read the worktree, which can change after
    // them: judge what git staged, and the attributes once more.
    refuse_large_blobs(&staging, &blobs)?;
    refuse_required_filters(&staging, &stage.add, Attributes::Staged)?;
    // Quiet: the summary lists every file, past the bound of 1 record.
    staging.run(&[
        "commit".into(),
        "--quiet".into(),
        "-m".into(),
        message.into(),
    ])?;
    // The commit exists: an unread id is a warning, never a failure.
    match staging.run(&["rev-parse".into(), "HEAD".into()]) {
        Ok(id) => Ok(id),
        Err(e) => {
            eprintln!("horch: committed, but could not read the new commit id: {e:#}");
            Ok("HEAD".to_string())
        }
    }
}

/// Refused when the index of `git` differs from `base` at a path outside
/// `allowed`, or holds a gitlink (mode 160000) that `base` does not: the
/// worktree can change between the checks and `git add`, and a later git
/// outside the sandbox would start a git in a staged gitlink. Read from the
/// index and the tree only, so no git starts in a gitlink here. Returns the
/// object ids of the staged blobs.
fn refuse_unchecked_entries(
    git: &Trusted,
    base: &str,
    allowed: &HashSet<OsString>,
) -> Result<Vec<String>> {
    let args: [OsString; 7] = [
        "diff-index".into(),
        "--cached".into(),
        "-z".into(),
        "--raw".into(),
        "--no-renames".into(),
        // Over SAFE_CONFIG's `all`, which would hide every gitlink.
        "--ignore-submodules=none".into(),
        base.into(),
    ];
    let out = run_git(
        git.command(clean_git(&git.bin, git.tree), &args),
        2 * MAX_PATHS,
    )?;
    if !out.complete {
        bail!("{}", too_many_paths());
    }
    if !out.status.success() || out.records.len() % 2 != 0 {
        bail!("git diff-index failed: {}", out.status);
    }
    let mut blobs = Vec::new();
    for entry in out.records.as_chunks::<2>().0 {
        let path = os_name(&entry[1]);
        let fields: Vec<&[u8]> = entry[0].split(|b| *b == b' ').collect();
        let new_mode = fields.get(1).copied();
        if new_mode == Some(&b"160000"[..]) {
            bail!(
                "refused: {} became a gitlink (a nested repository) after the checks; \
                 nothing is committed.",
                Path::new(&path).display()
            );
        }
        if !allowed.contains(&path) {
            bail!(
                "refused: {} changed after the checks; nothing is committed.",
                Path::new(&path).display()
            );
        }
        // A deletion stages no blob.
        if new_mode != Some(&b"000000"[..]) {
            if let Some(oid) = fields.get(3) {
                blobs.push(String::from_utf8_lossy(oid).into_owned());
            }
        }
    }
    Ok(blobs)
}

/// Refused when a staged blob is longer than [`MAX_FILE_BYTES`], or all of
/// them than [`MAX_TOTAL_BYTES`]: the sizes git stored, not the worktree's.
fn refuse_large_blobs(git: &Trusted, blobs: &[String]) -> Result<()> {
    if blobs.is_empty() {
        return Ok(());
    }
    let input: Vec<u8> = blobs
        .iter()
        .flat_map(|o| format!("{o}\n").into_bytes())
        .collect();
    let args: [OsString; 2] = ["cat-file".into(), "--batch-check=%(objectsize)".into()];
    // 1 git for every blob: 1 size line each, bounded like every record.
    let out = run_records(
        git.command(clean_git(&git.bin, git.tree), &args),
        input,
        b'\n',
        blobs.len(),
        GIT_DEADLINE,
    )?;
    if !out.complete || !out.status.success() || out.records.len() != blobs.len() {
        bail!(
            "refused: git cat-file printed {} sizes for {} blobs ({})",
            out.records.len(),
            blobs.len(),
            out.status
        );
    }
    let mut total: u64 = 0;
    for record in &out.records {
        let Some(len) = std::str::from_utf8(record)
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
        else {
            bail!("refused: git cat-file printed a size that is not a number");
        };
        if len > MAX_FILE_BYTES {
            bail!(
                "refused: a staged file is {len} bytes; horch ledger commit stages at most \
                 {MAX_FILE_BYTES} bytes per file. Nothing is committed."
            );
        }
        total = total.saturating_add(len);
        if total > MAX_TOTAL_BYTES {
            bail!(
                "refused: the staged files are more than {MAX_TOTAL_BYTES} bytes. Nothing is \
                 committed; commit them in parts with -- <files>."
            );
        }
    }
    Ok(())
}

/// What 1 commit stages, after the checks.
#[derive(Debug, Default)]
struct Stage {
    /// The paths git adds, relative to the worktree root.
    add: Vec<OsString>,
    /// Every path where the staged index may differ from HEAD: `add`, with
    /// staged deletions and the old path of a staged rename.
    allowed: HashSet<OsString>,
    /// True for `-- <files>`: only those paths, on top of HEAD. False for
    /// every change, on top of the index.
    only: bool,
}

/// What to stage: `paths`, or every change `git status` lists when there
/// are none. At most [`MAX_PATHS`].
fn paths_to_stage(git: &Trusted, paths: &[PathBuf]) -> Result<Stage> {
    let stage = if paths.is_empty() {
        let stage = changed_paths(git)?;
        refuse_nested_repos(&git.root, &stage.add)?;
        stage
    } else {
        let add: Vec<OsString> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(&git.root)
                    .unwrap_or(p)
                    .as_os_str()
                    .to_owned()
            })
            .collect();
        Stage {
            allowed: add.iter().cloned().collect(),
            add,
            only: true,
        }
    };
    if stage.add.len().max(stage.allowed.len()) > MAX_PATHS {
        bail!("{}", too_many_paths());
    }
    Ok(stage)
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
fn refuse_required_filters(git: &Trusted, names: &[OsString], from: Attributes) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let required = required_filters(git)?;
    let refused: Vec<String> = path_filters(git, attribute_git(&git.bin), names, from)?
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

/// Refused when the staging command selects a filter for a path.
/// Attributes from the empty tree hide `.gitattributes` files, not
/// `$GIT_DIR/info/attributes` or `core.attributesFile`. git add runs no
/// filter ([`Trusted::filter_drivers`] sets each driver empty), so it would
/// store the raw content where the repository expects filtered content.
/// The 2 guards are separate: this one refuses a selected filter, and the
/// overrides keep a path that appears after this check unfiltered.
fn refuse_staging_filters(git: &Trusted, names: &[OsString]) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let selected: Vec<String> = path_filters(
        git,
        clean_git(&git.bin, git.tree),
        names,
        Attributes::Worktree,
    )?
    .into_iter()
    .map(|(path, filter)| format!("{} (filter={filter})", path.to_string_lossy()))
    .collect();
    if !selected.is_empty() {
        bail!(
            "refused: {} selected by $GIT_DIR/info/attributes or core.attributesFile. \
             horch ledger commit runs no filter, so git would store the raw content: ask the \
             orchestrator to remove that attribute, or for a Claude worker to commit these paths.",
            selected.join(", ")
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
/// A staged deletion is in neither the index nor the worktree, so git add
/// would refuse it: it is only allowed, and the index already holds it.
fn changed_paths(git: &Trusted) -> Result<Stage> {
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
    let mut stage = Stage::default();
    let mut records = out.records.iter().filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        let (Some(xy), Some(path)) = (record.get(..2), record.get(3..)) else {
            continue;
        };
        let name = os_name(path);
        if xy != b"D " {
            stage.add.push(name.clone());
        }
        stage.allowed.insert(name);
        // A rename or copy is followed by its old path.
        if xy.iter().any(|b| matches!(b, b'R' | b'C')) {
            if let Some(old) = records.next() {
                stage.allowed.insert(os_name(old));
            }
        }
    }
    Ok(stage)
}

/// Each of `names` with its `filter` attribute, when it has one. Read with
/// `git check-attr`, which reads attributes and never runs a filter.
/// `base` is the git that reads the attributes. The names go on stdin,
/// NUL-ended: on the command line, 10000 long paths pass the system's
/// argument limit.
fn path_filters(
    git: &Trusted,
    base: Command,
    names: &[OsString],
    from: Attributes,
) -> Result<Vec<(OsString, String)>> {
    let expected = 3 * names.len();
    let mut args: Vec<OsString> = vec!["check-attr".into()];
    if from == Attributes::Staged {
        args.push("--cached".into());
    }
    args.extend(["-z".into(), "--stdin".into(), "filter".into()]);
    let mut input = Vec::new();
    for name in names {
        input.extend_from_slice(name.as_encoded_bytes());
        input.push(0);
    }
    let out = run_until(git.command(base, &args), input, expected, GIT_DEADLINE)?;
    if !out.complete || !out.status.success() {
        bail!("git check-attr failed: {}", out.status);
    }
    filter_records(&out.records, names.len())
}

/// Where `git check-attr` reads `.gitattributes` files.
#[derive(Clone, Copy, PartialEq)]
enum Attributes {
    /// The worktree, which can change while horch runs.
    Worktree,
    /// The index of the git that runs: what is staged, after git add.
    Staged,
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
    let code = out.status.code();
    if code == Some(1) && out.records.is_empty() {
        return Ok(HashMap::new());
    }
    if code != Some(0) || !out.complete {
        bail!(
            "refused: cannot read filter.<name>.required from the {scope} git config ({}). \
             horch ledger commit does not guess whether a filter is required.",
            out.status
        );
    }
    out.records.iter().map(|r| config_entry(r, scope)).collect()
}

/// `filter.<name>.required\n<true|false>` as `(name, required)`.
fn config_entry(record: &[u8], scope: &str) -> Result<(String, bool)> {
    let text = String::from_utf8_lossy(record);
    let entry = text.split_once('\n').and_then(|(key, value)| {
        let name = key.strip_prefix("filter.")?.strip_suffix(".required")?;
        Some((name.to_string(), config_bool(value)?))
    });
    entry.with_context(|| format!("refused: unreadable {scope} git config entry {text:?}"))
}

/// A `--type=bool` config value, or `None` for anything else.
fn config_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// The 1 way horch ledger commit runs git: [`run_until`] with no input and
/// [`GIT_DEADLINE`].
fn run_git(cmd: Command, max: usize) -> Result<Records> {
    run_until(cmd, Vec::new(), max, GIT_DEADLINE)
}

/// The worktree's git dir by the shared rule
/// ([`horch_core::taskledger::git::worktree_gitdir`]), with the common dir
/// from `git rev-parse` in the main checkout the ledger recorded. Also
/// returns the empty tree id of the repository's object format.
fn trusted_gitdir(
    bin: &Path,
    repo_root: &Path,
    worktree: &Path,
    root: &Path,
) -> Result<(PathBuf, &'static str)> {
    let (common, tree) = repo_metadata(bin, repo_root)?;
    let gitdir = worktree_gitdir(&common, worktree, root)?;
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
        let Ok(real) = std::fs::canonicalize(&candidate) else {
            too_old.push(format!("{} (missing)", candidate.display()));
            continue;
        };
        if writable.iter().any(|w| real.starts_with(w)) {
            bail!(
                "refused: git {} is inside the worktree, the main checkout or the state root. \
                 Set HORCH_GIT_BIN to an absolute path of git 2.41 or later outside them",
                real.display()
            );
        }
        let version = git_version(&real);
        if version.is_some_and(|v| v >= (2, 41)) {
            return Ok(real);
        }
        too_old.push(format!("{} ({version:?})", real.display()));
    }
    bail!(
        "refused: horch ledger commit needs git 2.41 or later, at HORCH_GIT_BIN or \
         one of {}: older git has no attribute source, so a worker's .gitattributes \
         could select a filter that runs outside the sandbox. horch never searches PATH, \
         which a worker can change. Where git is elsewhere (Nix, a user toolchain), the \
         operator sets HORCH_GIT_BIN to its absolute path before spawning Codex workers. \
         Found: {}",
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
    /// The filter drivers the repository config names: each git call runs
    /// with their `clean` and `process` set empty, so no filter runs.
    filters: Vec<String>,
    /// The index file to use instead of the git dir's own.
    index: Option<PathBuf>,
}

impl Trusted {
    fn new(bin: &Path, root: &Path, gitdir: &Path, tree: &'static str) -> Result<Trusted> {
        let mut git = Trusted {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
            gitdir: gitdir.to_path_buf(),
            tree,
            identity: Vec::new(),
            filters: Vec::new(),
            index: None,
        };
        // `-c` wins over the repository config: pass only what it lacks.
        git.identity = global_identity(bin)
            .into_iter()
            .filter(|(key, _)| !git.repo_sets(key))
            .collect();
        git.filters = git.filter_drivers()?;
        Ok(git)
    }

    /// The names of the filter drivers the repository config defines. The
    /// checks ([`refuse_staging_filters`]) refuse a selected filter before
    /// git add, and these overrides keep one from running if the worktree
    /// changes after the checks.
    fn filter_drivers(&self) -> Result<Vec<String>> {
        let args = [
            "config".into(),
            "--null".into(),
            "--name-only".into(),
            "--get-regexp".into(),
            OsString::from(r"^filter\."),
        ];
        let out = run_git(
            self.command(clean_git(&self.bin, self.tree), &args),
            MAX_PATHS,
        )?;
        let code = out.status.code();
        if code == Some(1) && out.records.is_empty() {
            return Ok(Vec::new());
        }
        if code != Some(0) || !out.complete {
            bail!(
                "refused: cannot list the filter drivers of the repository config ({})",
                out.status
            );
        }
        filter_driver_names(&out.records)
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
        for pair in filter_overrides(&self.filters) {
            cmd.arg("-c").arg(pair);
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
