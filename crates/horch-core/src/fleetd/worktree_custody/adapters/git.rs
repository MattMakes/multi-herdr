use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use horch_marketplace::git::{GitError, GitOutput, GitRunner, GitSandbox, SandboxHost};
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

pub fn worktree_add_key(row: &WorktreeRow) -> String {
    serde_json::json!([row.path, row.branch, row.base_commit]).to_string()
}

pub const DETACHED: &str = "(detached)";

pub const GATE_PREFIX: &str = "_gate-";

pub fn is_gate(row: &WorktreeRow) -> bool {
    row.branch == DETACHED
        && row
            .mode
            .as_ref()
            .is_some_and(|m| m.scratch && m.name.starts_with(GATE_PREFIX))
}

pub fn worktree_list_key(clock: FleetSeconds, period_s: u64) -> String {
    let period = i64::try_from(period_s.max(1)).unwrap_or(i64::MAX);
    (clock.0 / period).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddResult {
    pub path: String,
    pub branch: String,
    pub head: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeFacts {
    pub path: String,
    pub worktree_id: Option<String>,
    pub head: Option<String>,
    pub dirty: Option<bool>,
    pub bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResult {
    pub entries: Vec<ListEntry>,
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

pub(crate) fn failed(text: String) -> Ran {
    Outcome {
        passed: false,
        verdict: false,
        text,
    }
    .into()
}

const HOLDS_WORK: &str = "the folder holds files or commits";

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
        if !is_gate(row) {
            return Ok(failed(format!(
                "only the tree of a run gate is added with no branch: {}",
                row.path
            )));
        }
        git.add(repo, path, DETACHED, base)?;
    } else if git.branch_exists(repo, &row.branch)? {
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

fn at_its_base(git: &dyn WorktreeGitGateway, row: &WorktreeRow) -> Result<bool> {
    let tip = git.branch_tip(Path::new(&row.repo_root), &row.branch)?;
    Ok(tip.is_some() && tip.as_deref() == row.base_commit.as_deref())
}

fn of_its_task_and_name(row: &WorktreeRow) -> bool {
    Path::new(&row.path)
        .file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| row.branch.ends_with(&format!("-{f}")))
}

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

enum Folder {
    Absent,
    Empty,
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

fn same_path(a: &Path, b: &Path) -> bool {
    canonical(a) == canonical(b)
}

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

const SAFE_CONFIG: [&str; 9] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgSign=false",
    "tag.gpgSign=false",
    "core.attributesFile=/dev/null",
    "core.excludesFile=/dev/null",
    "protocol.allow=never",
    "credential.helper=",
    "core.askPass=",
];

const SAFE_ENV: [(&str, &str); 7] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_OPTIONAL_LOCKS", "0"),
];

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

const PROJECT_SCOPES: [&str; 2] = ["local", "worktree"];

pub const NO_SANDBOX: &str = "no sandbox";

pub const FLEETD_GIT_LIMIT: Duration = Duration::from_secs(10);

type RaceFn = dyn Fn(&Path, &[&str]) + Send + Sync;

#[derive(Clone)]
struct Race(Arc<RaceFn>);

impl fmt::Debug for Race {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Race")
    }
}

#[derive(Debug, Clone)]
pub struct CliWorktreeGit {
    runner: GitRunner,
    fleet_root: PathBuf,
    writable: Vec<PathBuf>,
    readable: Vec<PathBuf>,
    home: Option<PathBuf>,
    host: SandboxHost,
    race: Option<Race>,
}

impl CliWorktreeGit {
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

    pub fn writing(mut self, paths: &[PathBuf]) -> CliWorktreeGit {
        self.writable.extend_from_slice(paths);
        self
    }

    pub fn with_env(mut self, key: &str, value: &str) -> CliWorktreeGit {
        self.runner = self.runner.with_env(key, value);
        self
    }

    pub fn reading(mut self, paths: &[PathBuf]) -> CliWorktreeGit {
        self.readable.extend_from_slice(paths);
        self
    }

    #[doc(hidden)]
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> CliWorktreeGit {
        self.home = Some(home.into()).filter(|home| home.is_absolute());
        self
    }

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

    pub fn with_time_limit(mut self, limit: Duration) -> CliWorktreeGit {
        self.runner = self.runner.with_time_limit(limit);
        self
    }

    #[doc(hidden)]
    pub fn with_sandbox_host(mut self, host: SandboxHost) -> CliWorktreeGit {
        self.host = host;
        self
    }

    #[doc(hidden)]
    pub fn with_race(
        mut self,
        race: impl Fn(&Path, &[&str]) + Send + Sync + 'static,
    ) -> CliWorktreeGit {
        self.race = Some(Race(Arc::new(race)));
        self
    }

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

    fn run_folder(&self) -> PathBuf {
        self.fleet_root.join("run")
    }

    fn denials(&self) -> Denials {
        let run = self.run_folder();
        Denials {
            read: vec![run.join("operator.token")],
            connect: vec![run.join("fleetd.sock")],
            write: vec![run.join("requests")],
        }
    }

    fn hidden(&self) -> Result<Vec<PathBuf>, NotSupported> {
        let home =
            self.home
                .clone()
                .ok_or(NotSupported::new("sandbox", std::env::consts::OS).because(
                "HOME is not an absolute path, so the sandbox cannot hide the files of the user",
            ))?;
        Ok(vec![self.run_folder(), home])
    }

    pub fn checked(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        self.refuse_project_programs(dir)?;
        if let Some(Race(race)) = &self.race {
            race(dir, args);
        }
        self.safe(dir, args)
    }

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

    fn safe(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        private_folder(&self.run_folder())?;
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
        GIT_GROUP.store(OWN_GROUP.load(Ordering::SeqCst), Ordering::SeqCst);
        let out = self.runner.clone().with_sandbox(sandbox).output(dir, &full);
        GIT_GROUP.store(0, Ordering::SeqCst);
        out.map_err(|e| match e {
            GitError::TimedOut { .. } => {
                anyhow::anyhow!("{e}; {}", locks_left(&self.fleet_root, &self.writable))
            }
            e => anyhow::anyhow!("{e}"),
        })
        .with_context(|| format!("running git {} in {}", args.join(" "), dir.display()))
    }

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

fn locks_left(fleet_root: &Path, writable: &[PathBuf]) -> String {
    let mut locks = Vec::new();
    let mut todo: Vec<PathBuf> = writable.to_vec();
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() && entry.file_name() != "objects" => todo.push(path),
                Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "lock") => {
                    locks.push(path.display().to_string());
                }
                _ => {}
            }
        }
    }
    if locks.is_empty() {
        return "no .lock file stays in the git folders that it could write".to_string();
    }
    locks.sort();
    let journal = match journal_locks(fleet_root, locks.iter().map(PathBuf::from)) {
        Ok(()) => String::new(),
        Err(e) => format!(" The lock journal of fleetd failed: {e:#}."),
    };
    format!(
        ".lock files in the git folders after the stop: {}. Each later git that takes 1 of \
         these locks fails at once with \"File exists\" until the operator removes the file \
         by hand, when no git runs in that repository.{journal}",
        locks.join(", ")
    )
}

pub fn lock_journal(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join(LOCK_JOURNAL)
}

const LOCK_JOURNAL: &str = "git-locks-left.json";

fn change_journal(fleet_root: &Path, change: impl FnOnce(&mut BTreeSet<PathBuf>)) -> Result<()> {
    let run = fleet_root.join("run");
    private_folder(&run)?;
    let _held = crate::fsx::DirLock::acquire(
        &run,
        LOCK_JOURNAL,
        Duration::from_secs(60),
        Duration::from_secs(10),
    )?;
    let mut paths: BTreeSet<PathBuf> = read_journal(fleet_root)?.into_iter().collect();
    let before = paths.clone();
    change(&mut paths);
    if paths == before {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&paths).context("the lock journal serializes")?;
    Ok(crate::fsx::write_atomic(
        &lock_journal(fleet_root),
        &bytes,
        0o600,
    )?)
}

fn read_journal(fleet_root: &Path) -> Result<Vec<PathBuf>> {
    let path = lock_journal(fleet_root);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("reading the lock journal {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("reading the lock journal {}", path.display())),
    }
}

fn journal_locks(fleet_root: &Path, locks: impl IntoIterator<Item = PathBuf>) -> Result<()> {
    change_journal(fleet_root, |paths| paths.extend(locks))
}

pub fn locks_left_after_stops(fleet_root: &Path) -> Result<Vec<PathBuf>> {
    read_journal(fleet_root)
}

pub fn forget_lock_left(fleet_root: &Path, path: &Path) -> Result<()> {
    change_journal(fleet_root, |paths| {
        paths.remove(path);
    })
}

static GIT_GROUP: AtomicI32 = AtomicI32::new(0);

static OWN_GROUP: AtomicI32 = AtomicI32::new(0);

pub fn kill_job_group_on_stop() {
    let me = std::process::id();
    let leads = matches!(horch_platform::process::group_of(me), Ok(Some(g)) if g == me);
    let group = if leads {
        i32::try_from(me).unwrap_or(0)
    } else {
        0
    };
    OWN_GROUP.store(group, Ordering::SeqCst);
    if group > 0 {
        let _ = horch_platform::signal::kill_group_on_termination(&GIT_GROUP);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoSandbox {
    pub no_sandbox: String,
}

fn install_folder(bin: &Path, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let found = if bin.components().count() > 1 {
        bin.to_path_buf()
    } else {
        which(path, None, &bin.to_string_lossy())?
    };
    let real = found.canonicalize().ok()?;
    real.parent()?.parent().map(Path::to_path_buf)
}

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

pub fn tree_writer(git: CliWorktreeGit, row: &WorktreeRow) -> Result<CliWorktreeGit> {
    let dir = repo_git_dir(Path::new(&row.repo_root))?;
    Ok(git.writing(&[dir, PathBuf::from(&row.path)]))
}

pub(crate) fn private_folder(folder: &Path) -> Result<()> {
    if folder.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    let _ = create_dir_mode(&mut builder, 0o700);
    builder
        .create(folder)
        .with_context(|| format!("making {}", folder.display()))
}

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
        self.refuse_project_programs(repo)?;
        tree_folder(path)?;
        let path = path.to_str().context("the path of a tree is not UTF-8")?;
        if branch == DETACHED {
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
        self.git(repo, &["worktree", "add", "--", path, branch])
            .map(drop)
    }
}

fn tree_folder(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("making {}", path.display()))
}

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
pub(crate) mod tests {
    use super::*;

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

    static SEEN: AtomicI32 = AtomicI32::new(0);

    fn seeing_host(
        cmd: horch_platform::start::Command,
        _reach: &Reach<'_>,
    ) -> Result<horch_platform::start::Command, NotSupported> {
        SEEN.store(GIT_GROUP.load(Ordering::SeqCst), Ordering::SeqCst);
        Ok(cmd)
    }

    #[cfg(unix)]
    #[test]
    fn git_call_holds_the_job_group_only_while_it_runs() {
        let group = i32::MAX;
        OWN_GROUP.store(group, Ordering::SeqCst);
        for _ in 0..9 {
            horch_platform::signal::kill_group_on_termination(&GIT_GROUP).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let git = CliWorktreeGit::new("git", dir.path().join("fleet"))
            .with_home(dir.path().join("home"))
            .with_sandbox_host(seeing_host);
        let out = git.safe(dir.path(), &["--version"]);
        OWN_GROUP.store(0, Ordering::SeqCst);
        assert_eq!(out.unwrap().status, 0);
        assert_eq!(
            SEEN.load(Ordering::SeqCst),
            group,
            "git runs in the job group"
        );
        assert_eq!(
            GIT_GROUP.load(Ordering::SeqCst),
            0,
            "the slot is free after git"
        );
    }

    fn same_host(
        cmd: horch_platform::start::Command,
        _reach: &Reach<'_>,
    ) -> Result<horch_platform::start::Command, NotSupported> {
        Ok(cmd)
    }

    fn one_commit(dir: &Path) -> (PathBuf, String) {
        let repo = dir.join("repo");
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(o.status.success(), "{o:?}");
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        std::fs::create_dir_all(&repo).unwrap();
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "one"]);
        let tip = git(&["rev-parse", "HEAD"]);
        (repo, tip)
    }

    #[cfg(unix)]
    fn locking_git(dir: &Path, git_dir: &Path, on_term: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("locking-git");
        let lock = git_dir.join("refs/heads/x.lock");
        let script = format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = update-ref ]; then\n    \
             lock='{}'\n    trap '{on_term}' TERM\n    : > \"$lock\"\n    sleep 600 &\n    \
             wait\n    exit 0\n  fi\ndone\nexec git \"$@\"\n",
            lock.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    fn limited(dir: &Path, bin: &Path, repo: &Path) -> CliWorktreeGit {
        CliWorktreeGit::new(bin, dir.join("fleet"))
            .with_home(dir.join("home"))
            .with_sandbox_host(same_host)
            .writing(&[repo.join(".git")])
            .with_time_limit(Duration::from_secs(1))
    }

    #[cfg(unix)]
    #[test]
    fn a_git_stopped_at_its_limit_by_term_leaves_no_lock() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, tip) = one_commit(dir.path());
        let git_dir = repo.join(".git");
        let bin = locking_git(dir.path(), &git_dir, "rm -f \"$lock\"; exit 143");
        let e = limited(dir.path(), &bin, &repo)
            .git(&repo, &["update-ref", "refs/heads/x", &tip])
            .unwrap_err();
        let text = format!("{e:#}");
        assert!(text.contains("ran past its time limit of 1 s"), "{text}");
        assert!(text.contains("no .lock file stays"), "{text}");
        assert!(!git_dir.join("refs/heads/x.lock").exists());
    }

    #[cfg(unix)]
    pub(crate) fn kill_a_git_that_holds_a_lock(dir: &Path, fleet_root: &Path) -> PathBuf {
        let (repo, tip) = one_commit(dir);
        let git_dir = repo.join(".git");
        let bin = locking_git(dir, &git_dir, "");
        CliWorktreeGit::new(&bin, fleet_root)
            .with_home(dir.join("home"))
            .with_sandbox_host(same_host)
            .writing(std::slice::from_ref(&git_dir))
            .with_time_limit(Duration::from_secs(1))
            .git(&repo, &["update-ref", "refs/heads/x", &tip])
            .unwrap_err();
        let lock = git_dir.join("refs/heads/x.lock");
        assert!(lock.exists(), "the KILL leaves the lock");
        lock
    }

    #[cfg(unix)]
    #[test]
    fn a_git_killed_at_its_limit_journals_its_lock_before_its_error_returns() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fleet_root = dir.path().join("fleet");
        let lock = kill_a_git_that_holds_a_lock(dir.path(), &fleet_root);
        assert_eq!(
            locks_left_after_stops(&fleet_root).unwrap(),
            std::slice::from_ref(&lock)
        );
        let mode = std::fs::metadata(lock_journal(&fleet_root))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "owner-only");
        journal_locks(&fleet_root, [lock.clone()]).unwrap();
        assert_eq!(
            locks_left_after_stops(&fleet_root).unwrap(),
            std::slice::from_ref(&lock)
        );
        forget_lock_left(&fleet_root, &lock).unwrap();
        assert!(locks_left_after_stops(&fleet_root).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_git_killed_at_its_limit_names_the_lock_that_stays() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, tip) = one_commit(dir.path());
        let git_dir = repo.join(".git");
        let lock = git_dir.join("refs/heads/x.lock");
        let bin = locking_git(dir.path(), &git_dir, "");
        let e = limited(dir.path(), &bin, &repo)
            .git(&repo, &["update-ref", "refs/heads/x", &tip])
            .unwrap_err();
        let text = format!("{e:#}");
        assert!(text.contains("SIGKILL ended its group"), "{text}");
        assert!(text.contains(&lock.display().to_string()), "{text}");
        assert!(text.contains("File exists"), "{text}");
        assert!(lock.exists(), "the KILL leaves the lock");

        let started = std::time::Instant::now();
        let e = limited(dir.path(), Path::new("git"), &repo)
            .git(&repo, &["update-ref", "refs/heads/x", &tip])
            .unwrap_err();
        let text = format!("{e:#}");
        assert!(text.contains("x.lock"), "{text}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "git does not wait for the lock"
        );
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
