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

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

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

/// Config pairs on every git call: no hook, no fsmonitor, no signing program.
const SAFE_CONFIG: [&str; 4] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgsign=false",
    "gpg.program=/usr/bin/false",
];

/// git's empty tree id in a SHA-1 repository, and in a SHA-256 one.
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const EMPTY_TREE_SHA256: &str = "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321";

/// The most paths 1 commit takes: a bound on the `git status` and
/// `check-attr` output that horch reads outside the sandbox.
const MAX_PATHS: usize = 10_000;

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
    refuse_required_filters(&git, &paths)?;
    let id = commit(&git, &args.message, &paths)?;
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

/// Stage and commit `paths` (every change when empty); the new commit id.
/// git stages into a copy of the index, which replaces the index only when
/// the commit succeeds: a failed commit leaves the index as it was.
fn commit(git: &Trusted, message: &str, paths: &[PathBuf]) -> Result<String> {
    let index = git.gitdir.join("index");
    let scratch = git
        .gitdir
        .join(format!("index.horch-{}", std::process::id()));
    if index.exists() {
        std::fs::copy(&index, &scratch).with_context(|| format!("copying {}", index.display()))?;
    }
    let staged = git.with_index(&scratch);
    let result = stage_and_commit(&staged, message, paths);
    match result {
        Ok(_) => std::fs::rename(&scratch, &index)
            .with_context(|| format!("replacing {}", index.display()))?,
        Err(_) => {
            let _ = std::fs::remove_file(&scratch);
        }
    }
    result
}

fn stage_and_commit(git: &Trusted, message: &str, paths: &[PathBuf]) -> Result<String> {
    let named = paths.iter().map(|p| p.as_os_str().to_owned());
    let mut add: Vec<OsString> = vec!["add".into()];
    let mut commit: Vec<OsString> = vec!["commit".into(), "-m".into(), message.into()];
    if paths.is_empty() {
        add.push("-A".into());
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

/// Refused when a path to commit has a required `filter` attribute (Git
/// LFS, for one): with attributes from the empty tree, git would store its
/// raw content. With no paths, the paths are every change `git status`
/// lists, read as bytes: a name that is not UTF-8 stays the same name.
fn refuse_required_filters(git: &Trusted, paths: &[PathBuf]) -> Result<()> {
    let mut names: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
    if names.is_empty() {
        let status = git.run_bytes(&[
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
            "--untracked-files=all".into(),
        ])?;
        let mut entries = status.split(|b| *b == 0).filter(|e| !e.is_empty());
        while let Some(entry) = entries.next() {
            if let Some(path) = entry.get(3..) {
                names.push(os_name(path));
            }
            // A rename or copy is followed by its old path.
            if matches!(entry.first(), Some(b'R' | b'C')) {
                entries.next();
            }
        }
    }
    if names.is_empty() {
        return Ok(());
    }
    if names.len() > MAX_PATHS {
        bail!(
            "refused: {} paths to commit; horch ledger commit takes at most {MAX_PATHS}. \
             Commit them in parts with -- <files>.",
            names.len()
        );
    }
    let mut args: Vec<OsString> = vec![
        "check-attr".into(),
        "-z".into(),
        "filter".into(),
        "--".into(),
    ];
    args.extend(names);
    let out = git.run_with_attributes(&args)?;
    let fields: Vec<&[u8]> = out.split(|b| *b == 0).collect();
    // Only a required filter refuses: git itself fails without it, and
    // `git lfs install` sets `filter.lfs.required`. Another filter's path is
    // committed unfiltered. 1 config lookup per filter, not per path.
    let mut required: HashMap<String, bool> = HashMap::new();
    let mut filtered = Vec::new();
    for c in fields.chunks(3).filter(|c| c.len() == 3) {
        let filter = String::from_utf8_lossy(c[2]).into_owned();
        if matches!(filter.as_str(), "unspecified" | "unset") {
            continue;
        }
        let is_required = *required
            .entry(filter.clone())
            .or_insert_with(|| required_filter(git, &filter));
        if is_required {
            filtered.push(format!(
                "{} (filter={filter})",
                String::from_utf8_lossy(c[0])
            ));
        }
    }
    if !filtered.is_empty() {
        bail!(
            "refused: horch ledger commit runs no git filters, so it cannot commit {}, \
             whose filter is required. Ask the orchestrator for a Claude worker to commit \
             these paths.",
            filtered.join(", ")
        );
    }
    Ok(())
}

/// Whether `filter.<name>.required` is true in the repository config or the
/// operator's global config (where `git lfs install` writes it).
fn required_filter(git: &Trusted, name: &str) -> bool {
    let key = format!("filter.{name}.required");
    let repo = git
        .run(&["config".into(), "--get".into(), key.as_str().into()])
        .ok();
    let value = repo.or_else(|| global_config(&git.bin, &key));
    value.is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
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
    let out = cmd
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--show-object-format",
        ])
        .output()
        .with_context(|| format!("running {}", bin.display()))?;
    if !out.status.success() {
        bail!("refused: {} is not a git checkout", repo_root.display());
    }
    let text = String::from_utf8_lossy(&out.stdout);
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
            t == recorded || std::fs::canonicalize(t).is_ok_and(|p| p == root.join(".git"))
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
/// `gitdir: <gitdir>`.
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
/// there is no git 2.41 or later at `HORCH_GIT_BIN` or a [`TRUSTED_GITS`] path.
pub fn commit_git(ctx: &RuntimeContext) -> Result<PathBuf> {
    trusted_git(&ctx.bins.harness.git, &[ctx.paths.state_root.as_path()])
}

/// `(major, minor)` of the git at `bin`, run with a clean environment.
fn git_version(bin: &Path) -> Option<(u32, u32)> {
    let out = git_without_env(bin, Some(EMPTY_TREE_SHA1))
        .arg("--version")
        .output()
        .ok()?;
    parse_git_version(&String::from_utf8_lossy(&out.stdout))
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
        Trusted {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
            gitdir: gitdir.to_path_buf(),
            tree,
            identity: global_identity(bin),
            index: None,
        }
    }

    /// This git, with `index` as its index file.
    fn with_index(&self, index: &Path) -> Trusted {
        Trusted {
            index: Some(index.to_path_buf()),
            ..self.clone()
        }
    }

    fn run(&self, args: &[OsString]) -> Result<String> {
        let out = self.run_bytes(args)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// `run`, with stdout as git printed it: `-z` output keeps its bytes.
    fn run_bytes(&self, args: &[OsString]) -> Result<Vec<u8>> {
        self.output(clean_git(&self.bin, self.tree), args)
    }

    /// `run_bytes`, with the attributes of the worktree: only for
    /// `check-attr`, which reads attributes and never runs a filter.
    fn run_with_attributes(&self, args: &[OsString]) -> Result<Vec<u8>> {
        self.output(attribute_git(&self.bin), args)
    }

    fn output(&self, mut cmd: Command, args: &[OsString]) -> Result<Vec<u8>> {
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
        let out = cmd
            .args(args)
            .output()
            .with_context(|| format!("running {}", self.bin.display()))?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.first()
                    .map(|a| a.to_string_lossy())
                    .unwrap_or_default(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
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
    let out = git_without_env(bin, Some(EMPTY_TREE_SHA1))
        .current_dir("/")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !value.is_empty()).then_some(value)
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
