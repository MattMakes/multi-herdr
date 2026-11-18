//! The add and list executors: the only `git worktree add` and `git worktree list` calls.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" and "The kinds of job" (rows `worktree_add` and `worktree_list`):
//! each git call of custody is a job, never work inside a pass (FM-12).
//!
//! [`CliWorktreeGit`] runs git with the rights of fleetd and with no
//! sandbox, so git starts no hook and no program of the project (the fleetd
//! git rule, review F4 of T-607). Each of its calls goes through
//! `CliWorktreeGit::checked`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use horch_marketplace::git::{GitOutput, GitRunner};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeRow;
use crate::fleetd::worktree_custody::application::ports::{
    ListEntry, TreeState, WorktreeGitGateway,
};
use crate::taskledger::git::dir_bytes;

// ─── the input keys ─────────────────────────────────────────────────────

/// The input key of the `worktree_add` job of `row`: its path, its branch
/// and its base tip.
pub fn worktree_add_key(row: &WorktreeRow) -> String {
    serde_json::json!([row.path, row.branch, row.base_commit]).to_string()
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
/// of the row. A folder at the path is kept unless it holds no work, a
/// branch that exists already is kept, and the row is not moved.
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
    if git.branch_exists(repo, &row.branch)? {
        return Ok(failed(format!(
            "the branch {} exists already; the job keeps it",
            row.branch
        )));
    }
    git.add(repo, path, &row.branch, base)?;
    let state = git
        .tree(path)?
        .with_context(|| format!("git made no work tree at {}", row.path))?;
    Ok(passed(
        format!("made {} on {} at {base}", row.path, row.branch),
        add_result(row, state.head),
    ))
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

/// The `-c` keys of each git call, before its subcommand. The git runner
/// adds `core.hooksPath=/dev/null` too; it is here so that this list is the
/// whole rule.
const SAFE_CONFIG: [&str; 4] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgSign=false",
    "tag.gpgSign=false",
];

/// The environment of each git call.
const SAFE_ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
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

/// [`WorktreeGitGateway`] on the git binary.
#[derive(Debug, Clone)]
pub struct CliWorktreeGit {
    runner: GitRunner,
}

impl CliWorktreeGit {
    pub fn new(bin: impl Into<PathBuf>) -> CliWorktreeGit {
        let runner = SAFE_ENV
            .iter()
            .fold(GitRunner::new(bin), |r, (k, v)| r.with_env(k, v));
        CliWorktreeGit { runner }
    }

    /// This adapter, with the private index file `index` for each git call.
    /// The caller owns `index`, keeps it outside every project, and deletes
    /// the file when it is done.
    pub(crate) fn with_index(&self, index: &Path) -> Result<CliWorktreeGit> {
        let runner = self
            .runner
            .clone()
            .with_index_file(index)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(CliWorktreeGit { runner })
    }

    /// The 1 way this adapter runs a git command: first refuse a key of the
    /// project config that starts a program, then run `git <args>` in `dir`
    /// with the keys of [`SAFE_CONFIG`], whatever its exit code.
    pub(crate) fn checked(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        self.refuse_project_programs(dir)?;
        self.safe(dir, args)
    }

    /// [`CliWorktreeGit::checked`], where a non-zero exit is an error with
    /// the message of git.
    pub(crate) fn git(&self, dir: &Path, args: &[&str]) -> Result<String> {
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

    /// `git <args>` in `dir` with the keys of [`SAFE_CONFIG`], whatever its
    /// exit code.
    fn safe(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        let mut full: Vec<&str> = SAFE_CONFIG.iter().flat_map(|c| ["-c", c]).collect();
        full.extend_from_slice(args);
        self.runner
            .output(dir, &full)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("running git {} in {}", args.join(" "), dir.display()))
    }

    /// An error that names the first key of the repository or worktree
    /// config of `dir` that starts a program. The global config of the user
    /// is not the project's.
    fn refuse_project_programs(&self, dir: &Path) -> Result<()> {
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

/// The first key that starts a program in a project scope of the output of
/// `git config --list --show-origin --show-scope -z`, with its origin. Each
/// record is `<scope>\0<origin>\0<key>\n<value>\0`; a key with no value has
/// no `\n`.
fn project_program(text: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = text.split('\0').collect();
    parts.chunks_exact(3).find_map(|rec| {
        let (scope, origin, pair) = (rec[0], rec[1], rec[2]);
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
        let status = self.git(path, &["status", "--porcelain", "--untracked-files=all"])?;
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

    fn prune(&self, repo: &Path) -> Result<()> {
        self.git(repo, &["worktree", "prune"]).map(drop)
    }

    fn add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
        let path = path.to_str().context("the path of a tree is not UTF-8")?;
        self.git(repo, &["worktree", "add", "-b", branch, path, base])
            .map(drop)
    }
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
