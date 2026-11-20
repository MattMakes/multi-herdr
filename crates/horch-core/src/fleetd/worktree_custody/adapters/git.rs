//! The add and list executors: the only `git worktree add` and `git worktree list` calls.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" and "The kinds of job" (rows `worktree_add` and `worktree_list`):
//! each git call of custody is a job, never work inside a pass (FM-12).
//!
//! [`CliWorktreeGit`] runs each git call in the platform sandbox, and with
//! a config that the project cannot change, so a program of the project
//! never runs with the rights of fleetd (the fleetd git rule, review F4 of
//! T-607, T-691). Each of its calls goes through `CliWorktreeGit::checked`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use horch_marketplace::git::{GitOutput, GitRunner, GitSandbox, SandboxHost};
use horch_platform::fs::create_dir_mode;
use horch_platform::sandbox::{sandboxed_hiding, Denials, Network, Reach};
use horch_platform::NotSupported;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeRow;
use crate::fleetd::worktree_custody::application::ports::{
    ListEntry, TreeState, WorktreeGitGateway,
};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::paths::home_dir;
use crate::runtime::process::which;
use crate::taskledger::git::dir_bytes;

// ─── the input keys ─────────────────────────────────────────────────────

/// The input key of the `worktree_add` job of `row`: its path, its branch
/// and its base tip.
pub fn worktree_add_key(row: &WorktreeRow) -> String {
    serde_json::json!([row.path, row.branch, row.base_commit]).to_string()
}

/// The branch of the tree of a run gate: it has none (row W11).
pub const DETACHED: &str = "(detached)";

/// The start of the folder name and of the mode name of the tree of a run
/// gate: `_gate-<run slug>`.
pub const GATE_PREFIX: &str = "_gate-";

/// Whether `row` is the tree of a run gate: a scratch tree `_gate-<slug>`
/// on no branch. Only such a tree is added detached.
pub fn is_gate(row: &WorktreeRow) -> bool {
    row.branch == DETACHED
        && row
            .mode
            .as_ref()
            .is_some_and(|m| m.scratch && m.name.starts_with(GATE_PREFIX))
}

/// The input key of the `worktree_list` job at `clock`: the number of the
/// period of `period_s` seconds on the fleet clock. So 1 run is made for
/// each period.
pub fn worktree_list_key(clock: FleetSeconds, period_s: u64) -> String {
    let period = i64::try_from(period_s.max(1)).unwrap_or(i64::MAX);
    (clock.0 / period).to_string()
}

// ─── the results ────────────────────────────────────────────────────────

/// The result of a passed `worktree_add` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddResult {
    pub path: String,
    pub branch: String,
    pub head: String,
}

/// The facts of 1 tree in a `worktree_list` result. A fact that git or
/// the disk cannot give is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeFacts {
    pub path: String,
    /// The row of the tree, when a row has its path.
    pub worktree_id: Option<String>,
    pub head: Option<String>,
    pub dirty: Option<bool>,
    /// The size of the folder in bytes. The main tree of the repository
    /// has no size: it is not a tree of custody.
    pub bytes: Option<u64>,
    /// The error of git when it could not read the tree. A path that is
    /// absent, or that is not the top of a work tree, has no git fact and no
    /// error.
    pub error: Option<String>,
}

/// The result of a passed `worktree_list` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResult {
    /// Each entry of `git worktree list --porcelain`, in its order.
    pub entries: Vec<ListEntry>,
    /// The facts of each entry that is not bare, then of each row with no
    /// entry.
    pub trees: Vec<TreeFacts>,
}

fn passed(text: String, data: impl Serialize) -> Ran {
    Ran {
        outcome: Outcome {
            passed: true,
            verdict: true,
            text,
        },
        data: Some(serde_json::to_value(data).expect("a result serializes")),
    }
}

/// A run that ends `failed` with the reason `text`.
pub(crate) fn failed(text: String) -> Ran {
    Outcome {
        passed: false,
        verdict: false,
        text,
    }
    .into()
}

// ─── the worktree_add executor ──────────────────────────────────────────

/// The reason of a `worktree_add` run that keeps a folder.
const HOLDS_WORK: &str = "the folder holds files or commits";

/// The `worktree_add` job of the `reserved` row `row`, in the project root
/// of the row. A folder at the path is kept unless it holds no work. A
/// branch that exists already is attached when it is the branch of the
/// task and name of the row and no other tree holds it. Else it is refused.
/// A branch is never deleted. The row is not moved.
pub fn worktree_add(git: &dyn WorktreeGitGateway, row: &WorktreeRow) -> Ran {
    add(git, row).unwrap_or_else(|e| failed(format!("{e:#}")))
}

fn add(git: &dyn WorktreeGitGateway, row: &WorktreeRow) -> Result<Ran> {
    let repo = Path::new(&row.repo_root);
    let path = Path::new(&row.path);
    let Some(base) = row.base_commit.as_deref() else {
        return Ok(failed(format!(
            "the worktree {} has no base tip",
            row.worktree_id
        )));
    };
    let entries = git.list(repo)?;
    let entry = entries.iter().find(|e| same_path(Path::new(&e.path), path));
    match folder(path)? {
        Folder::Absent | Folder::Empty => {
            if entry.is_some_and(|e| e.prunable.is_some()) {
                git.prune(repo)?;
            }
        }
        Folder::Full => {
            return Ok(match made_already(git, row, entry, base)? {
                Some(head) => passed(
                    format!("the tree {} exists already, at its base tip", row.path),
                    add_result(row, head),
                ),
                None => failed(format!("{HOLDS_WORK}: {}", row.path)),
            });
        }
    }
    if row.branch == DETACHED {
        // The tree of a run gate: `git worktree add --detach` at its base
        // tip, with no branch.
        if !is_gate(row) {
            return Ok(failed(format!(
                "only the tree of a run gate is added with no branch: {}",
                row.path
            )));
        }
        git.add(repo, path, DETACHED, base)?;
    } else if git.branch_exists(repo, &row.branch)? {
        // The branch of an earlier run of this task and name, or the
        // branch at the base tip of the row (a restore): the job attaches
        // it, with each of its commits, when no other tree holds it. A
        // branch is never deleted (review F1 of T-288).
        if !of_its_task_and_name(row) && !at_its_base(git, row)? {
            return Ok(refused(
                row,
                None,
                format!(
                    "the branch {} exists, and it is not the branch of the task and name of {}",
                    row.branch, row.path
                ),
            ));
        }
        let full = format!("refs/heads/{}", row.branch);
        let others: Vec<&ListEntry> = entries
            .iter()
            .filter(|e| e.branch.as_deref() == Some(full.as_str()))
            .filter(|e| !same_path(Path::new(&e.path), path))
            .collect();
        if let Some(e) = others.iter().find(|e| e.prunable.is_none()) {
            return Ok(refused(
                row,
                Some(&e.path),
                format!(
                    "the branch {} is checked out in {}; the job attaches only a branch \
                     that no other worktree holds",
                    row.branch, e.path
                ),
            ));
        }
        if !others.is_empty() {
            git.prune(repo)?;
        }
        git.attach(repo, path, &row.branch)?;
    } else {
        git.add(repo, path, &row.branch, base)?;
    }
    let state = git
        .tree(path)?
        .with_context(|| format!("git made no work tree at {}", row.path))?;
    Ok(passed(
        format!("made {} on {} at {base}", row.path, row.branch),
        add_result(row, state.head),
    ))
}

/// Whether the branch of `row` holds exactly the base tip that the row
/// records. A restore records the tip of its archive or trash ref, so the
/// job attaches the archived branch only while it is at that commit.
fn at_its_base(git: &dyn WorktreeGitGateway, row: &WorktreeRow) -> Result<bool> {
    let tip = git.branch_tip(Path::new(&row.repo_root), &row.branch)?;
    Ok(tip.is_some() && tip.as_deref() == row.base_commit.as_deref())
}

/// Whether the branch of `row` is the branch that its task and name give.
/// The folder of a task tree is `T<seq>-<name>` on the branch
/// `<run branch>-T<seq>-<name>`, and the folder of a scratch tree is
/// `<name>` on `fleet-scratch-<run>-<name>`: the branch ends with
/// `-<folder>`.
fn of_its_task_and_name(row: &WorktreeRow) -> bool {
    Path::new(&row.path)
        .file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| row.branch.ends_with(&format!("-{f}")))
}

/// The data of a `worktree_add` run that refused the existing branch of
/// its row: the tree at `held_by` has it checked out, or, when `held_by`
/// is `None`, it is not the branch of the task and name of the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRefused {
    pub branch: String,
    pub held_by: Option<String>,
}

fn refused(row: &WorktreeRow, held_by: Option<&str>, text: String) -> Ran {
    let data = BranchRefused {
        branch: row.branch.clone(),
        held_by: held_by.map(str::to_string),
    };
    Ran {
        data: Some(serde_json::to_value(data).expect("a result serializes")),
        ..failed(text)
    }
}

fn add_result(row: &WorktreeRow, head: String) -> AddResult {
    AddResult {
        path: row.path.clone(),
        branch: row.branch.clone(),
        head,
    }
}

/// What is at the path of a tree.
enum Folder {
    Absent,
    Empty,
    /// A folder with an entry, or a file.
    Full,
}

fn folder(path: &Path) -> Result<Folder> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Folder::Absent),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if !meta.is_dir() {
        return Ok(Folder::Full);
    }
    let mut read =
        std::fs::read_dir(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(match read.next() {
        None => Folder::Empty,
        Some(_) => Folder::Full,
    })
}

/// The HEAD of the folder at the path of `row` when it is the tree that the
/// job makes, with no work: a tree that git lists, on the branch of the
/// row, at the base tip, with no modified or untracked file. Then an
/// earlier run made it and its result was lost. `None` for each other
/// folder.
fn made_already(
    git: &dyn WorktreeGitGateway,
    row: &WorktreeRow,
    entry: Option<&ListEntry>,
    base: &str,
) -> Result<Option<String>> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let branch = format!("refs/heads/{}", row.branch);
    if entry.prunable.is_some() || entry.branch.as_deref() != Some(branch.as_str()) {
        return Ok(None);
    }
    Ok(git
        .tree(Path::new(&row.path))?
        .filter(|t| t.head == base && !t.dirty)
        .map(|t| t.head))
}

/// Whether `a` and `b` name 1 folder. Git prints canonical paths; a row can
/// hold another form of the same path.
fn same_path(a: &Path, b: &Path) -> bool {
    canonical(a) == canonical(b)
}

/// The canonical form of `path`, also when the last part does not exist.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

// ─── the worktree_list executor ─────────────────────────────────────────

/// The `worktree_list` job of the repository `repo`: each entry of `git
/// worktree list --porcelain`, and the HEAD, the dirty flag and the size of
/// each entry and of each row in `rows` (the rows of `repo` that are not
/// `removed`). It changes nothing.
pub fn worktree_list(git: &dyn WorktreeGitGateway, repo: &Path, rows: &[WorktreeRow]) -> Ran {
    list(git, repo, rows).unwrap_or_else(|e| failed(format!("{e:#}")))
}

fn list(git: &dyn WorktreeGitGateway, repo: &Path, rows: &[WorktreeRow]) -> Result<Ran> {
    let entries = git.list(repo)?;
    let row_of = |path: &Path| rows.iter().find(|r| same_path(Path::new(&r.path), path));
    let mut trees = Vec::new();
    for (n, e) in entries.iter().enumerate() {
        if e.bare {
            continue;
        }
        let path = Path::new(&e.path);
        let id = row_of(path).map(|r| r.worktree_id.clone());
        // The first entry is the main tree of the repository.
        trees.push(facts(git, path, id, n > 0));
    }
    for r in rows {
        let path = Path::new(&r.path);
        if !entries.iter().any(|e| same_path(Path::new(&e.path), path)) {
            trees.push(facts(git, path, Some(r.worktree_id.clone()), true));
        }
    }
    let text = format!(
        "{} entries, {} trees in {}",
        entries.len(),
        trees.len(),
        repo.display()
    );
    Ok(passed(text, ListResult { entries, trees }))
}

/// The facts of the tree at `path`. A folder that is not the top of a work
/// tree has a size and no git fact.
fn facts(
    git: &dyn WorktreeGitGateway,
    path: &Path,
    worktree_id: Option<String>,
    size: bool,
) -> TreeFacts {
    let mut f = TreeFacts {
        path: path.display().to_string(),
        worktree_id,
        head: None,
        dirty: None,
        bytes: None,
        error: None,
    };
    if !matches!(std::fs::symlink_metadata(path), Ok(m) if m.is_dir()) {
        return f;
    }
    if size {
        f.bytes = Some(dir_bytes(path));
    }
    match git.tree(path) {
        Ok(Some(t)) => {
            f.head = Some(t.head);
            f.dirty = Some(t.dirty);
        }
        Ok(None) => {}
        Err(e) => f.error = Some(format!("{e:#}")),
    }
    f
}

// ─── the git adapter ────────────────────────────────────────────────────

/// The `-c` keys of each git call, before its subcommand. The command line
/// is the last scope that git reads, so each key wins over the config of
/// the project. The git runner adds `core.hooksPath=/dev/null` too; it is
/// here so that this list is the whole rule.
const SAFE_CONFIG: [&str; 9] = [
    // Hooks.
    "core.hooksPath=/dev/null",
    // The fsmonitor program.
    "core.fsmonitor=false",
    // The gpg program.
    "commit.gpgSign=false",
    "tag.gpgSign=false",
    // The attributes file of the user, which can select a filter driver.
    "core.attributesFile=/dev/null",
    // The ignore file of the user, below HOME, which git cannot read.
    "core.excludesFile=/dev/null",
    // Each transport: the ssh command, a remote helper, `ext::`.
    "protocol.allow=never",
    // An empty value empties the list of credential helpers.
    "credential.helper=",
    "core.askPass=",
];

/// The environment of each git call. The global and system config and the
/// system attributes file are not read: the global config of the user can
/// define a filter driver (for example git-lfs) that a `.gitattributes` of
/// the project selects (review F4 of T-287). No git call takes an
/// optional lock: a `git status` does not write the index of a worker
/// tree, so a worker commit never finds an `index.lock` of fleetd (T-290).
const SAFE_ENV: [(&str, &str); 7] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_OPTIONAL_LOCKS", "0"),
];

/// The config keys that start a program, as `<section>.<name>`.
const PROGRAM_KEYS: [&str; 8] = [
    "core.fsmonitor",
    "core.sshcommand",
    "core.askpass",
    "core.editor",
    "sequence.editor",
    "credential.helper",
    "gpg.program",
    "diff.external",
];

/// The config keys with a subsection that start a program, as
/// `<section>.<name>`: `filter.<driver>.clean` is `filter.clean`.
const PROGRAM_SUBKEYS: [&str; 8] = [
    "filter.clean",
    "filter.smudge",
    "filter.process",
    "merge.driver",
    "diff.textconv",
    "diff.command",
    "credential.helper",
    "gpg.program",
];

/// The config scopes that the project controls.
const PROJECT_SCOPES: [&str; 2] = ["local", "worktree"];

/// The start of the text of a job that did not run git because this host
/// has no sandbox.
pub const NO_SANDBOX: &str = "no sandbox";

/// A test seam: it runs after the scan of the project config and before
/// git starts, with the folder and the arguments of the git call.
type RaceFn = dyn Fn(&Path, &[&str]) + Send + Sync;

#[derive(Clone)]
struct Race(Arc<RaceFn>);

impl fmt::Debug for Race {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Race")
    }
}

/// [`WorktreeGitGateway`] on the git binary.
///
/// Each git call runs in the platform sandbox (way A of T-691): git, and
/// each program that git starts, can write only the paths of the job
/// ([`CliWorktreeGit::writing`] and the folder of a private index),
/// cannot read, write or connect below `<fleet root>/run` and HOME (but
/// the paths of the job and the git install folder), has no network, and
/// gets only the environment of
/// `horch_marketplace::git::SANDBOX_KEPT_ENV` and of the runner. The keys of
/// `SAFE_CONFIG` and `SAFE_ENV` are the second layer (way B): they stop
/// hooks, the fsmonitor, gpg, each transport, credential helpers, askpass
/// and the global filter drivers. A filter, merge or diff driver of the
/// project config has a free name, and `$GIT_DIR/info/attributes` can
/// select it, so only the sandbox stops it when it comes after the scan.
/// So does `diff.external`: an empty value makes git run an empty program.
#[derive(Debug, Clone)]
pub struct CliWorktreeGit {
    runner: GitRunner,
    fleet_root: PathBuf,
    writable: Vec<PathBuf>,
    /// The paths below HOME that git can read: the git install folder and
    /// the paths of the job.
    readable: Vec<PathBuf>,
    /// HOME, hidden from git. `None` when HOME is not an absolute path:
    /// then no git runs.
    home: Option<PathBuf>,
    host: SandboxHost,
    race: Option<Race>,
}

impl CliWorktreeGit {
    /// The adapter of a job of the fleet at `fleet_root`. It can write no
    /// path until [`CliWorktreeGit::writing`] names one.
    pub fn new(bin: impl Into<PathBuf>, fleet_root: impl Into<PathBuf>) -> CliWorktreeGit {
        let bin = bin.into();
        let install = install_folder(&bin, ProcessEnv.var_os("PATH").as_deref());
        let home = Some(home_dir(&ProcessEnv)).filter(|home| home.is_absolute());
        let runner = SAFE_ENV
            .iter()
            .fold(GitRunner::new(bin), |r, (k, v)| r.with_env(k, v));
        CliWorktreeGit {
            runner,
            fleet_root: fleet_root.into(),
            writable: Vec::new(),
            readable: install.into_iter().collect(),
            home,
            host: sandboxed_hiding,
            race: None,
        }
    }

    /// This adapter, and git can also write below each path of `paths`.
    pub fn writing(mut self, paths: &[PathBuf]) -> CliWorktreeGit {
        self.writable.extend_from_slice(paths);
        self
    }

    /// This adapter, and each git call also gets `value` in the environment
    /// key `key`.
    pub fn with_env(mut self, key: &str, value: &str) -> CliWorktreeGit {
        self.runner = self.runner.with_env(key, value);
        self
    }

    /// This adapter, and git can also read below each path of `paths`, also
    /// when it is below HOME. Give paths of the ledger, never a path that
    /// the project names.
    pub fn reading(mut self, paths: &[PathBuf]) -> CliWorktreeGit {
        self.readable.extend_from_slice(paths);
        self
    }

    /// A test seam: `home` takes the place of HOME, which git cannot read.
    #[doc(hidden)]
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> CliWorktreeGit {
        self.home = Some(home.into()).filter(|home| home.is_absolute());
        self
    }

    /// This adapter, with the private index file `index` for each git call.
    /// Git can write in the folder of `index`. The caller owns `index`,
    /// keeps it outside every project, and deletes the file when it is
    /// done.
    pub(crate) fn with_index(&self, index: &Path) -> Result<CliWorktreeGit> {
        let folder = index.parent().context("a private index has a folder")?;
        let runner = self
            .runner
            .clone()
            .with_index_file(index)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(CliWorktreeGit {
            runner,
            ..self.clone()
        }
        .writing(&[folder.to_path_buf()]))
    }

    /// A test seam: `host` takes the place of the sandbox of the platform.
    #[doc(hidden)]
    pub fn with_sandbox_host(mut self, host: SandboxHost) -> CliWorktreeGit {
        self.host = host;
        self
    }

    /// A test seam: `race` runs after the scan of the project config and
    /// before git starts, with the folder and the arguments of the git call.
    #[doc(hidden)]
    pub fn with_race(
        mut self,
        race: impl Fn(&Path, &[&str]) + Send + Sync + 'static,
    ) -> CliWorktreeGit {
        self.race = Some(Race(Arc::new(race)));
        self
    }

    /// Why this host cannot run git in the sandbox, or `None` when it can.
    pub fn sandbox_gap(&self) -> Option<NotSupported> {
        let probe = horch_platform::start::command(self.runner.bin());
        let hidden = match self.hidden() {
            Ok(hidden) => hidden,
            Err(gap) => return Some(gap),
        };
        let reach = Reach {
            writable: &self.writable,
            readable: &self.readable,
            hidden: &hidden,
            denied: &self.denials(),
            network: Network::Denied,
        };
        (self.host)(probe, &reach).err()
    }

    /// `<fleet root>/run`, with the operator token and the command channel.
    fn run_folder(&self) -> PathBuf {
        self.fleet_root.join("run")
    }

    /// The operator acts that git and each program it starts cannot do,
    /// as a worker cannot (SR-8): read the operator token, connect to the
    /// command channel, write a request file. The run folder is hidden
    /// too; the denials keep the 3 paths denied whatever is writable.
    fn denials(&self) -> Denials {
        let run = self.run_folder();
        Denials {
            read: vec![run.join("operator.token")],
            connect: vec![run.join("fleetd.sock")],
            write: vec![run.join("requests")],
        }
    }

    /// The paths that git cannot read: [`CliWorktreeGit::run_folder`], and
    /// HOME with the files of the user (keys, tokens, the shell history).
    /// With no absolute HOME, the folder of the user is not known, so no
    /// git runs: a gap of the sandbox.
    fn hidden(&self) -> Result<Vec<PathBuf>, NotSupported> {
        let home =
            self.home
                .clone()
                .ok_or(NotSupported::new("sandbox", std::env::consts::OS).because(
                "HOME is not an absolute path, so the sandbox cannot hide the files of the user",
            ))?;
        Ok(vec![self.run_folder(), home])
    }

    /// The 1 way this adapter runs a git command: first refuse a key of the
    /// project config that starts a program, then run `git <args>` in `dir`
    /// in the sandbox, with the keys of `SAFE_CONFIG`, whatever its exit
    /// code.
    pub fn checked(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        self.refuse_project_programs(dir)?;
        if let Some(Race(race)) = &self.race {
            race(dir, args);
        }
        self.safe(dir, args)
    }

    /// [`CliWorktreeGit::checked`], where a non-zero exit is an error with
    /// the message of git.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = self.checked(dir, args)?;
        if out.status != 0 {
            bail!(
                "git {} failed in {}: {}",
                args.join(" "),
                dir.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `git <args>` in `dir` in the sandbox, with the keys of
    /// [`SAFE_CONFIG`], whatever its exit code.
    fn safe(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        // On Linux the sandbox covers a hidden folder only when it exists.
        private_folder(&self.run_folder())?;
        // Git reads the folder of the call: a path of the caller.
        let mut readable = self.readable.clone();
        readable.push(dir.to_path_buf());
        let sandbox = GitSandbox {
            writable: self.writable.clone(),
            readable,
            hidden: self
                .hidden()
                .map_err(|gap| anyhow::anyhow!("{NO_SANDBOX}: {gap}"))?,
            denied: self.denials(),
            network: Network::Denied,
            host: self.host,
        };
        let mut full: Vec<&str> = SAFE_CONFIG.iter().flat_map(|c| ["-c", c]).collect();
        full.extend_from_slice(args);
        self.runner
            .clone()
            .with_sandbox(sandbox)
            .output(dir, &full)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("running git {} in {}", args.join(" "), dir.display()))
    }

    /// An error that names the first key of the repository or worktree
    /// config of `dir` that starts a program. The global config of the user
    /// is not read.
    pub(crate) fn refuse_project_programs(&self, dir: &Path) -> Result<()> {
        let args = ["config", "--list", "--show-origin", "--show-scope", "-z"];
        let out = self.safe(dir, &args)?;
        if out.status != 0 {
            bail!(
                "git config --list failed in {}: {}",
                dir.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some((key, origin)) = project_program(&text) {
            bail!(
                "the project config sets {key} ({origin}), which starts a program; \
                 fleetd runs no program of the project, so the job changes nothing"
            );
        }
        Ok(())
    }
}

/// The result data of a job that ran no git because this host has no
/// sandbox: the reason of the platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoSandbox {
    pub no_sandbox: String,
}

/// The install folder of the git binary `bin`, found on `path` when it is a
/// bare name: the folder above its `bin` folder, after each link, so git can
/// read its own files when they are below HOME.
fn install_folder(bin: &Path, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let found = if bin.components().count() > 1 {
        bin.to_path_buf()
    } else {
        which(path, None, &bin.to_string_lossy())?
    };
    let real = found.canonicalize().ok()?;
    real.parent()?.parent().map(Path::to_path_buf)
}

/// The run of a job that does not run git because this host has no
/// sandbox, or `None` when it has one. The text starts with [`NO_SANDBOX`],
/// and the data is [`NoSandbox`].
pub fn no_sandbox(git: &CliWorktreeGit) -> Option<Ran> {
    git.sandbox_gap().map(|gap| {
        let data = NoSandbox {
            no_sandbox: gap.to_string(),
        };
        Ran {
            data: Some(serde_json::to_value(data).expect("a result serializes")),
            ..failed(format!(
                "{NO_SANDBOX}: {gap}; fleetd runs git in a project only in the sandbox, \
                 so the job ran no git"
            ))
        }
    })
}

/// The git folder `<repo root>/.git` of the repository at `repo_root`,
/// which git of a job can write. It must be a folder: a `.git` file can
/// name any folder.
pub fn repo_git_dir(repo_root: &Path) -> Result<PathBuf> {
    let dir = repo_root.join(".git");
    if !dir.is_dir() {
        bail!(
            "{} is not a folder; fleetd runs git only in a repository with its own .git folder",
            dir.display()
        );
    }
    Ok(dir)
}

/// `git`, and it can also write the git folder of the repository of `row`
/// and the folder of its tree.
pub fn tree_writer(git: CliWorktreeGit, row: &WorktreeRow) -> Result<CliWorktreeGit> {
    let dir = repo_git_dir(Path::new(&row.repo_root))?;
    Ok(git.writing(&[dir, PathBuf::from(&row.path)]))
}

/// Make `folder` and its parents when it does not exist. The new folders
/// have mode 0700.
pub(crate) fn private_folder(folder: &Path) -> Result<()> {
    if folder.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    // A host with no modes (Windows) makes the folder with its default rights.
    let _ = create_dir_mode(&mut builder, 0o700);
    builder
        .create(folder)
        .with_context(|| format!("making {}", folder.display()))
}

/// The first key that starts a program in a project scope of the output of
/// `git config --list --show-origin --show-scope -z`, with its origin. Each
/// record is `<scope>\0<origin>\0<key>\n<value>\0`; a key with no value has
/// no `\n`.
pub(crate) fn project_program(text: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = text.split('\0').collect();
    parts
        .as_chunks::<3>()
        .0
        .iter()
        .find_map(|&[scope, origin, pair]| {
            let (key, value) = pair.split_once('\n').unwrap_or((pair, "true"));
            let starts = PROJECT_SCOPES.contains(&scope) && starts_program(key, value);
            starts.then(|| (key.to_string(), origin.to_string()))
        })
}

/// Whether the config key `key` with `value` starts a program.
fn starts_program(key: &str, value: &str) -> bool {
    let key = key.to_ascii_lowercase();
    let section = key.split('.').next().unwrap_or("");
    let name = key.rsplit('.').next().unwrap_or("");
    let short = format!("{section}.{name}");
    let listed = if key.matches('.').count() >= 2 {
        PROGRAM_SUBKEYS.contains(&short.as_str())
    } else {
        PROGRAM_KEYS.contains(&key.as_str())
    };
    let off = key == "core.fsmonitor"
        && matches!(
            value.to_ascii_lowercase().as_str(),
            "false" | "no" | "off" | "0"
        );
    listed && !off
}

impl WorktreeGitGateway for CliWorktreeGit {
    fn list(&self, repo: &Path) -> Result<Vec<ListEntry>> {
        let out = self.git(repo, &["worktree", "list", "--porcelain", "-z"])?;
        Ok(parse_list(&out))
    }

    fn tree(&self, path: &Path) -> Result<Option<TreeState>> {
        if !path.is_dir() {
            return Ok(None);
        }
        // A folder in the project that is not a tree of its own is in the
        // tree of the project: git would answer for that tree.
        let top = self.git(path, &["rev-parse", "--show-toplevel"])?;
        if !same_path(Path::new(top.trim_end()), path) {
            return Ok(None);
        }
        let head = self.git(path, &["rev-parse", "--verify", "HEAD"])?;
        let status = self.git(
            path,
            &["--no-optional-locks", "status", "--porcelain", "-uall"],
        )?;
        Ok(Some(TreeState {
            head: head.trim().to_string(),
            dirty: !status.trim().is_empty(),
        }))
    }

    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool> {
        let full = format!("refs/heads/{branch}");
        let out = self.checked(repo, &["show-ref", "--verify", "--quiet", &full])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            code => bail!(
                "git show-ref of {full} failed in {} ({code}): {}",
                repo.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn branch_tip(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        if !self.branch_exists(repo, branch)? {
            return Ok(None);
        }
        let full = format!("refs/heads/{branch}");
        Ok(Some(
            self.git(repo, &["rev-parse", "--verify", &full])?
                .trim()
                .to_string(),
        ))
    }

    fn prune(&self, repo: &Path) -> Result<()> {
        self.git(repo, &["worktree", "prune"]).map(drop)
    }

    fn add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
        // A refused add makes no folder.
        self.refuse_project_programs(repo)?;
        tree_folder(path)?;
        let path = path.to_str().context("the path of a tree is not UTF-8")?;
        if branch == DETACHED {
            // The tree of a run gate (`is_gate`); `--` ends the options.
            return self
                .git(repo, &["worktree", "add", "--detach", "--", path, base])
                .map(drop);
        }
        self.git(repo, &["worktree", "add", "-b", branch, path, base])
            .map(drop)
    }

    fn attach(&self, repo: &Path, path: &Path, branch: &str) -> Result<()> {
        self.refuse_project_programs(repo)?;
        tree_folder(path)?;
        let path = path.to_str().context("the path of a tree is not UTF-8")?;
        // `--` ends the options: no path or branch name reads as a flag.
        self.git(repo, &["worktree", "add", "--", path, branch])
            .map(drop)
    }
}

/// Make the empty folder of the tree at `path`, and the folders that hold
/// it. Git in the sandbox can write only the tree itself, and on Linux the
/// sandbox binds only a folder that exists, so fleetd makes it first. Git
/// adds a tree in an empty folder.
fn tree_folder(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("making {}", path.display()))
}

/// The entries of `git worktree list --porcelain -z`: attributes end with
/// `\0`, and an entry ends with 1 more `\0`.
fn parse_list(out: &str) -> Vec<ListEntry> {
    let mut entries = Vec::new();
    let mut cur: Option<ListEntry> = None;
    for line in out.split('\0') {
        if line.is_empty() {
            entries.extend(cur.take());
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        if word == "worktree" {
            entries.extend(cur.take());
            cur = Some(ListEntry {
                path: rest.to_string(),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let Some(e) = cur.as_mut() else { continue };
        match word {
            "HEAD" => e.head = Some(rest.to_string()),
            "branch" => e.branch = Some(rest.to_string()),
            "detached" => e.detached = true,
            "bare" => e.bare = true,
            "locked" => e.locked = Some(rest.to_string()),
            "prunable" => e.prunable = Some(rest.to_string()),
            _ => {}
        }
    }
    entries.extend(cur);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with 2 commits on `main` and the branch `feat` at the
    /// first; the row of the folder `.worktrees/feat` on `feat`, whose base
    /// tip is `base`.
    fn restore_row(base: impl Fn(&str, &str) -> String) -> (tempfile::TempDir, WorktreeRow) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        std::fs::create_dir_all(&repo).unwrap();
        git(&["init", "--quiet"]);
        git(&["commit", "--quiet", "--allow-empty", "-m", "one"]);
        git(&["branch", "feat"]);
        let one = git(&["rev-parse", "HEAD"]);
        git(&["commit", "--quiet", "--allow-empty", "-m", "two"]);
        let two = git(&["rev-parse", "HEAD"]);
        let row = WorktreeRow {
            worktree_id: "w1".into(),
            task_id: None,
            worker_id: None,
            repo_root: repo.display().to_string(),
            path: repo.join(".worktrees").join("feat").display().to_string(),
            branch: "feat".into(),
            base: "feat".into(),
            base_commit: Some(base(&one, &two)),
            status: crate::taskledger::model::WorktreeStatus::Open,
            state: crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState::Reserved,
            owner_attempt: None,
            head_sha: None,
            dirty: false,
            bytes_on_disk: None,
            rescue_ref: None,
            seen_at: None,
            mode: None,
        };
        (dir, row)
    }

    /// The git of the `worktree_add` job of `row`, with the fleet root in
    /// `dir`: it can write the git folder and the tree.
    fn add_git(dir: &tempfile::TempDir, row: &WorktreeRow) -> CliWorktreeGit {
        tree_writer(CliWorktreeGit::new("git", dir.path().join("fleet")), row).unwrap()
    }

    #[test]
    fn worktree_add_attaches_an_existing_branch_that_is_at_the_base_tip_of_its_row() {
        let (dir, row) = restore_row(|one, _| one.to_string());
        let ran = worktree_add(&add_git(&dir, &row), &row);
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        let head = add_git(&dir, &row)
            .git(Path::new(&row.path), &["branch", "--show-current"])
            .unwrap();
        assert_eq!(head.trim(), "feat");
    }

    #[test]
    fn worktree_add_refuses_an_existing_branch_that_is_not_at_the_base_tip_of_its_row() {
        let (dir, row) = restore_row(|_, two| two.to_string());
        let ran = worktree_add(&add_git(&dir, &row), &row);
        assert!(!ran.outcome.passed, "{}", ran.outcome.text);
        assert!(
            ran.outcome
                .text
                .contains("it is not the branch of the task and name"),
            "{}",
            ran.outcome.text
        );
        assert!(!Path::new(&row.path).exists());
    }

    #[test]
    fn parse_list_reads_each_attribute() {
        let out = "worktree /r\0HEAD abc\0branch refs/heads/main\0\0\
                   worktree /w1\0HEAD abc\0detached\0prunable gitdir file points to non-existent location\0\0\
                   worktree /w2\0HEAD abc\0branch refs/heads/b\0locked why\ntwo\0\0";
        let e = parse_list(out);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(e[1].detached);
        assert!(e[1].prunable.is_some());
        assert_eq!(e[2].locked.as_deref(), Some("why\ntwo"));
        assert_eq!(e[2].prunable, None);
    }

    #[test]
    fn project_program_names_each_listed_key_of_a_project_scope() {
        let rec = |scope: &str, kv: &str| format!("{scope}\0file:x\0{kv}\0");
        for kv in [
            "core.fsmonitor\n/x",
            "core.fsmonitor",
            "core.sshcommand\nssh",
            "filter.lfs.smudge\nx",
            "filter.My.Driver.process\nx",
            "merge.ours.driver\nx",
            "diff.pdf.textconv\nx",
            "credential.https://h.helper\nx",
            "gpg.ssh.program\nx",
        ] {
            for scope in ["local", "worktree"] {
                let text = rec(scope, kv);
                assert!(project_program(&text).is_some(), "{scope} {kv:?}");
            }
            assert_eq!(project_program(&rec("global", kv)), None, "{kv:?}");
        }
        for kv in [
            "core.fsmonitor\nfalse",
            "core.bare\nfalse",
            "filter.lfs.required\ntrue",
        ] {
            assert_eq!(project_program(&rec("local", kv)), None, "{kv:?}");
        }
    }

    #[test]
    fn worktree_list_key_is_the_number_of_the_period() {
        assert_eq!(worktree_list_key(FleetSeconds(0), 60), "0");
        assert_eq!(worktree_list_key(FleetSeconds(119), 60), "1");
        assert_eq!(worktree_list_key(FleetSeconds(120), 60), "2");
        assert_eq!(worktree_list_key(FleetSeconds(5), 0), "5");
    }
}
