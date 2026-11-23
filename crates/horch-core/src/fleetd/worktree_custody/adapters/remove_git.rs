use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use horch_platform::fs::rename_noreplace;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::worktree_custody::adapters::git::{failed, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::rescue_git::{self, PrivateIndex};
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeRow;
use crate::fleetd::worktree_custody::application::ports::WorktreeGitGateway;
use crate::fleetd::worktree_custody::domain::removal::{self, Basis, Fact, Facts, Record, Saved};

const KEEP_BRANCH: &str = "keep-branch:";

pub fn remove_key(path: &str, keep_branch: bool) -> String {
    if keep_branch {
        format!("{KEEP_BRANCH}{path}")
    } else {
        path.to_string()
    }
}

pub fn read_remove_key(key: &str) -> (&str, bool) {
    match key.strip_prefix(KEEP_BRANCH) {
        Some(path) => (path, true),
        None => (key, false),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: String,
    pub branch: String,
    pub forced: bool,
    pub ignored_files: u64,
    pub branch_deleted: bool,
    #[serde(default)]
    pub keep_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    NoAttempt,
    Ended,
    Process { pid: u32, start: Option<u64> },
    Unknown(String),
}

pub trait RemovalGateway {
    fn head(&self, tree: &Path) -> Result<String>;
    fn dirty(&self, tree: &Path) -> Result<bool>;
    fn rev(&self, repo: &Path, rev: &str) -> Result<String>;
    fn disk_tree(&self, row: &WorktreeRow, head: &str) -> Result<String>;
    fn stashes(&self, repo: &Path) -> Result<Vec<String>>;
    fn ignored_files(&self, tree: &Path) -> Result<u64>;
    fn alive(&self, pid: u32, start: Option<u64>) -> bool;
    fn cwd_pids(&self, tree: &Path) -> Result<Vec<u32>>;
    fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> Result<()>;
    fn repair(&self, repo: &Path, tree: &Path) -> Result<()>;
    fn remove(&self, repo: &Path, tree: &Path, force: bool) -> Result<()>;
    fn merged(&self, repo: &Path, branch: &str, base: &str) -> Result<bool>;
    fn delete_branch(&self, repo: &Path, branch: &str, base: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct CliRemoval {
    git: CliWorktreeGit,
    fleet_root: PathBuf,
}

impl CliRemoval {
    pub fn new(git: CliWorktreeGit, fleet_root: PathBuf) -> CliRemoval {
        CliRemoval { git, fleet_root }
    }
}

impl RemovalGateway for CliRemoval {
    fn head(&self, tree: &Path) -> Result<String> {
        let out = self
            .git
            .git(tree, &["rev-parse", "--show-toplevel", "--verify", "HEAD"])?;
        let mut lines = out.lines();
        let top = lines.next().map(Path::new);
        let canonical = tree
            .canonicalize()
            .with_context(|| format!("reading {}", tree.display()))?;
        if top.and_then(|t| t.canonicalize().ok()).as_deref() != Some(canonical.as_path()) {
            bail!("no work tree at {}", tree.display());
        }
        lines
            .next()
            .map(str::to_string)
            .with_context(|| format!("git gives no HEAD in {}", tree.display()))
    }

    fn dirty(&self, tree: &Path) -> Result<bool> {
        self.git
            .tree(tree)?
            .map(|s| s.dirty)
            .with_context(|| format!("no work tree at {}", tree.display()))
    }

    fn rev(&self, repo: &Path, rev: &str) -> Result<String> {
        rescue_git::rev(&self.git, repo, rev)
    }

    fn disk_tree(&self, row: &WorktreeRow, head: &str) -> Result<String> {
        let id = &row.worktree_id;
        let run = rescue_git::run_folder(&self.fleet_root, row)?;
        let index = PrivateIndex::new(run.join(format!("remove-{id}.index")))?;
        rescue_git::snapshot(
            &self.git.with_index(&index.path)?,
            Path::new(&row.path),
            head,
        )
    }

    fn stashes(&self, repo: &Path) -> Result<Vec<String>> {
        let out = self.git.git(repo, &["stash", "list"])?;
        Ok(out.lines().map(str::to_string).collect())
    }

    fn ignored_files(&self, tree: &Path) -> Result<u64> {
        let args = [
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
        ];
        let out = self.git.git(tree, &args)?;
        Ok(out.split('\0').filter(|f| !f.is_empty()).count() as u64)
    }

    fn alive(&self, pid: u32, start: Option<u64>) -> bool {
        crate::procid::alive(pid, start)
            || horch_platform::process::group_members(pid).map_or(true, |m| !m.is_empty())
    }

    fn cwd_pids(&self, tree: &Path) -> Result<Vec<u32>> {
        let tree = tree
            .canonicalize()
            .with_context(|| format!("reading {}", tree.display()))?;
        match horch_platform::process::cwd_pids_under(&tree) {
            Ok(pids) => pids.context("listing the processes"),
            Err(e) => bail!("{e}"),
        }
    }

    fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> Result<()> {
        let real = from
            .canonicalize()
            .with_context(|| format!("reading {}", from.display()))?;
        let locked = self.git.list(repo)?.into_iter().find_map(|entry| {
            let same = Path::new(&entry.path).canonicalize().ok().as_ref() == Some(&real);
            entry.locked.filter(|_| same)
        });
        if let Some(reason) = locked {
            bail!(
                "cannot move a locked working tree {} (lock reason: {reason:?})",
                from.display()
            );
        }
        rename_noreplace(from, to)
            .with_context(|| format!("renaming {} to {}", from.display(), to.display()))?;
        self.repair(repo, to)
    }

    fn repair(&self, repo: &Path, tree: &Path) -> Result<()> {
        let tree = tree.to_str().context("the path of a tree is not UTF-8")?;
        self.git
            .git(repo, &["worktree", "repair", "--", tree])
            .map(drop)
    }

    fn remove(&self, repo: &Path, tree: &Path, force: bool) -> Result<()> {
        let path = tree.to_str().context("the path of a tree is not UTF-8")?;
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.extend(["--", path]);
        let failed = match self.git.git(repo, &args) {
            Ok(_) => return Ok(()),
            Err(e) => e,
        };
        let empty = std::fs::read_dir(tree).is_ok_and(|mut entries| entries.next().is_none());
        let real = tree.canonicalize().ok();
        let listed = self
            .git
            .list(repo)?
            .iter()
            .any(|entry| Path::new(&entry.path).canonicalize().ok() == real);
        if !empty || listed {
            return Err(failed);
        }
        std::fs::remove_dir(tree).with_context(|| format!("removing {}", tree.display()))
    }

    fn merged(&self, repo: &Path, branch: &str, base: &str) -> Result<bool> {
        let tip = format!("refs/heads/{branch}");
        let out = self
            .git
            .checked(repo, &["merge-base", "--is-ancestor", &tip, base])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            code => bail!(
                "git merge-base --is-ancestor {tip} {base} failed ({code}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn delete_branch(&self, repo: &Path, branch: &str, base: &str) -> Result<()> {
        self.git
            .git(repo, &["branch", "--set-upstream-to", base, "--", branch])?;
        self.git
            .git(repo, &["branch", "-d", "--", branch])
            .map(drop)
    }
}

pub type ReadRow<'a> = &'a dyn Fn(&str) -> Result<Option<WorktreeRow>>;

pub fn remove(
    gw: &dyn RemovalGateway,
    row: &WorktreeRow,
    landed_commit: Option<&str>,
    owner: &Owner,
    ledger: ReadRow<'_>,
    keep_branch: bool,
) -> Ran {
    match run(gw, row, landed_commit, owner, ledger, keep_branch) {
        Ok(r) => Ran {
            outcome: Outcome {
                passed: true,
                verdict: true,
                text: format!(
                    "removed {} ({} ignored files deleted with it; branch {} {})",
                    r.path,
                    r.ignored_files,
                    r.branch,
                    match (r.branch_deleted, r.keep_branch) {
                        (true, _) => "deleted",
                        (false, true) => "kept, as the job input asks",
                        (false, false) => "kept",
                    }
                ),
            },
            data: Some(serde_json::to_value(&r).expect("a removal serializes")),
        },
        Err(why) => failed(format!(
            "the removal of {} is refused, and the tree stays: {why}",
            row.worktree_id
        )),
    }
}

fn run(
    gw: &dyn RemovalGateway,
    row: &WorktreeRow,
    landed_commit: Option<&str>,
    owner: &Owner,
    ledger: ReadRow<'_>,
    keep_branch: bool,
) -> Result<Removed, String> {
    let id = row.worktree_id.as_str();
    let basis = removal::basis(
        id,
        row.state.as_str(),
        landed_commit,
        row.rescue_ref.as_deref(),
    )?;
    let repo = Path::new(&row.repo_root);
    if basis.force() {
        restore(gw, row)?;
    }
    let facts = read_facts(gw, row, &basis, owner, ledger);
    removal::decide(&facts)?;
    let ignored_files = if basis.force() {
        quarantine_and_remove(gw, row, &basis, owner, ledger)?
    } else {
        gw.remove(repo, Path::new(&row.path), false)
            .map_err(|e| format!("{e:#}"))?;
        facts.ignored_files.unwrap_or(0)
    };
    let branch_deleted = !keep_branch
        && matches!(gw.merged(repo, &row.branch, &row.base), Ok(true))
        && gw.delete_branch(repo, &row.branch, &row.base).is_ok();
    Ok(Removed {
        path: row.path.clone(),
        branch: row.branch.clone(),
        forced: basis.force(),
        ignored_files,
        branch_deleted,
        keep_branch,
    })
}

fn quarantine_and_remove(
    gw: &dyn RemovalGateway,
    row: &WorktreeRow,
    basis: &Basis,
    owner: &Owner,
    ledger: ReadRow<'_>,
) -> Result<u64, String> {
    let repo = Path::new(&row.repo_root);
    let tree = Path::new(&row.path);
    let quarantine = removal::quarantine_path(tree, &row.worktree_id);
    gw.move_tree(repo, tree, &quarantine).map_err(|e| {
        format!(
            "the move to the quarantine folder {} failed; the next run moves back \
             a moved folder: {e:#}",
            quarantine.display()
        )
    })?;
    let mut moved = row.clone();
    moved.path = quarantine.display().to_string();
    let facts = read_facts(gw, &moved, basis, owner, ledger);
    if let Err(why) = removal::decide(&facts) {
        return Err(match gw.move_tree(repo, &quarantine, tree) {
            Ok(()) => format!("in quarantine, {why}"),
            Err(e) => format!(
                "in quarantine, {why}; the tree stays in the quarantine folder {}, \
                 because the move back failed: {e:#}",
                quarantine.display()
            ),
        });
    }
    gw.remove(repo, &quarantine, true).map_err(|e| {
        format!(
            "the removal of the quarantine folder {} failed: {e:#}",
            quarantine.display()
        )
    })?;
    Ok(facts.ignored_files.unwrap_or(0))
}

fn restore(gw: &dyn RemovalGateway, row: &WorktreeRow) -> Result<(), String> {
    let repo = Path::new(&row.repo_root);
    let tree = Path::new(&row.path);
    let quarantine = removal::quarantine_path(tree, &row.worktree_id);
    if !exists(&quarantine)? {
        return Ok(());
    }
    if exists(tree)? {
        return Err(format!(
            "both {} and its quarantine folder {} exist; the job changes nothing",
            tree.display(),
            quarantine.display()
        ));
    }
    gw.repair(repo, &quarantine)
        .and_then(|()| gw.move_tree(repo, &quarantine, tree))
        .map_err(|e| {
            format!(
                "the tree stays in the quarantine folder {}, because the move back failed: {e:#}",
                quarantine.display()
            )
        })
}

fn exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("reading {}: {e}", path.display())),
    }
}

fn read_facts(
    gw: &dyn RemovalGateway,
    row: &WorktreeRow,
    basis: &Basis,
    owner: &Owner,
    ledger: ReadRow<'_>,
) -> Facts {
    let tree = Path::new(&row.path);
    let repo = Path::new(&row.repo_root);
    let head_before = fact(gw.head(tree));
    let stashes = fact(gw.stashes(repo)).map(|lines| {
        lines
            .into_iter()
            .filter(|l| removal::names_branch(l, &row.branch))
            .collect()
    });
    let ignored_files = fact(gw.ignored_files(tree));
    let saved = match basis {
        Basis::Landed { landed_commit } => Saved::Landed {
            landed_commit: landed_commit.clone(),
            dirty: fact(gw.dirty(tree)),
        },
        Basis::Rescued { rescue_ref } => {
            let read_back = fact(gw.rev(repo, rescue_ref));
            let of_commit = |suffix: &str| match &read_back {
                Ok(c) => fact(gw.rev(repo, &format!("{c}{suffix}"))),
                Err(e) => Err(e.clone()),
            };
            let commit_tree = of_commit("^{tree}");
            let commit_parent = of_commit("^");
            let owner_live = match owner {
                Owner::NoAttempt | Owner::Ended => Ok(false),
                Owner::Process { pid, start } => Ok(gw.alive(*pid, *start)),
                Owner::Unknown(why) => Err(why.clone()),
            };
            let cwd_pids = fact(gw.cwd_pids(tree));
            let disk_tree = match &head_before {
                Ok(head) => fact(gw.disk_tree(row, head)),
                Err(e) => Err(e.clone()),
            };
            Saved::Rescued {
                rescue_ref: rescue_ref.clone(),
                read_back,
                commit_tree,
                commit_parent,
                owner_live,
                cwd_pids,
                disk_tree,
            }
        }
    };
    let head_after = fact(gw.head(tree));
    let record_after = match ledger(&row.worktree_id) {
        Ok(Some(now)) => Ok(record(&now)),
        Ok(None) => Err("the ledger has no row of the tree".to_string()),
        Err(e) => Err(format!("{e:#}")),
    };
    Facts {
        record_before: record(row),
        head_before,
        stashes,
        ignored_files,
        saved,
        head_after,
        record_after,
    }
}

fn record(row: &WorktreeRow) -> Record {
    Record {
        state: row.state.as_str().to_string(),
        owner_attempt: row.owner_attempt.clone(),
        rescue_ref: row.rescue_ref.clone(),
    }
}

fn fact<T>(r: Result<T>) -> Fact<T> {
    r.map_err(|e| format!("{e:#}"))
}
