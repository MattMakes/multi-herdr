//! The `worktree_remove` executor: the only `git worktree remove` call of
//! custody.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" (rows `landed`, `rescued`) and "The kinds of job" (row
//! `worktree_remove`): the job reads each fact again, asks the guard of
//! `domain::removal`, and only then runs `git worktree remove`. A refusal
//! ends the run `failed` with the reason of the guard and changes nothing
//! (RR-9, RR-13, FM-27).
//!
//! A landed tree is removed with no `--force`, so git refuses it too when a
//! file changed. A rescued tree is removed with 1 `--force` (never 2, so a
//! locked tree stays): git refuses a tree with a modified or untracked file
//! without it, and the guard has just shown that the rescue ref holds
//! exactly those files. Before that removal, 1 rename by fleetd and a
//! `git worktree repair` put the folder in its quarantine folder beside it, and the checks run
//! again there with a new snapshot (review F5 of T-287): a process that
//! writes through the old path cannot change the tree that is removed. A
//! run that ends between the move and the removal leaves the folder in
//! quarantine; the next run moves it back first. Ignored files are not on
//! the rescue ref (design.md, "Rescue before any cleanup"; MAP.md of
//! fleetd-p3, Q6) and are deleted with the tree; the result counts them.
//!
//! Each git call goes through `CliWorktreeGit::checked`, so git starts no
//! hook and no program of the project (the fleetd git rule).

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

/// The input key of a `worktree_remove` job: the path of the tree.
pub fn remove_key(path: &str) -> String {
    path.to_string()
}

/// What a passed removal did: the result data of the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: String,
    pub branch: String,
    /// `git worktree remove` got 1 `--force`: the tree was `rescued`.
    pub forced: bool,
    /// The count of ignored files that were deleted with the tree.
    pub ignored_files: u64,
    /// The branch was merged into the base of the row and is deleted.
    pub branch_deleted: bool,
}

/// The owner attempt of the tree, as the ledger records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// The row names no owner attempt.
    NoAttempt,
    /// The agent process of the owner attempt: its pid and start time.
    Process { pid: u32, start: Option<u64> },
    /// The ledger cannot name the process of the owner attempt.
    Unknown(String),
}

/// The I/O of a removal. Each call names its folder. A call that cannot
/// answer is an error, never a default fact. A test uses a fake.
pub trait RemovalGateway {
    /// The HEAD of the tree at `tree`.
    fn head(&self, tree: &Path) -> Result<String>;
    /// Whether `git status` lists a modified or untracked file in `tree`.
    fn dirty(&self, tree: &Path) -> Result<bool>;
    /// The object that `rev` names in the repository `repo`.
    fn rev(&self, repo: &Path, rev: &str) -> Result<String>;
    /// The tree of a new private index of the folder of `row` on `head`.
    fn disk_tree(&self, row: &WorktreeRow, head: &str) -> Result<String>;
    /// The lines of `git stash list` of the repository `repo`.
    fn stashes(&self, repo: &Path) -> Result<Vec<String>>;
    /// The count of ignored files in `tree`.
    fn ignored_files(&self, tree: &Path) -> Result<u64>;
    /// Whether the process `pid` with the start time `start` runs, or a
    /// process of the process group `pid` runs: a child of the owner can
    /// write into the tree from a folder outside it. A platform that cannot
    /// list a group answers `true`.
    fn alive(&self, pid: u32, start: Option<u64>) -> bool;
    /// The processes with their working folder in `tree`.
    fn cwd_pids(&self, tree: &Path) -> Result<Vec<u32>>;
    /// `git worktree move <from> <to>` in `repo`: 1 rename of the folder,
    /// then the record of git is changed to the new path.
    fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> Result<()>;
    /// `git worktree repair <tree>` in `repo`: the record of git names the
    /// folder at `tree` again after a rename that git did not record.
    fn repair(&self, repo: &Path, tree: &Path) -> Result<()>;
    /// `git worktree remove [--force] <tree>` in `repo`.
    fn remove(&self, repo: &Path, tree: &Path, force: bool) -> Result<()>;
    /// Whether the branch `branch` is merged into `base`.
    fn merged(&self, repo: &Path, branch: &str, base: &str) -> Result<bool>;
    /// `git branch -d <branch>`, which git checks against the upstream
    /// `base`.
    fn delete_branch(&self, repo: &Path, branch: &str, base: &str) -> Result<()>;
}

/// [`RemovalGateway`] on the git binary, the processes of the platform
/// and the private index files under `<fleet root>/run`.
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
    /// 1 `rev-parse`, with no `git status`, so the last read before the
    /// removal is short.
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

    /// fleetd renames the folder itself, then `git worktree repair` names
    /// the new folder in the record of git. So git in the sandbox writes
    /// only the 2 exact folders, never the folder that holds the trees: on
    /// Linux a new tree beside them stays out of reach. A refused scan
    /// renames nothing.
    fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> Result<()> {
        self.git.refuse_project_programs(repo)?;
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

    /// On Linux the sandbox binds the folder of the tree, so git removes
    /// all in it and its record, and then cannot remove the folder itself
    /// (`EBUSY`). When git fails, its record of the tree is gone and the
    /// folder is empty, fleetd removes that empty folder.
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
        // `branch -d` checks that the branch is merged into its upstream:
        // make that the base, so git itself checks it again. Never `-D`.
        self.git
            .git(repo, &["branch", "--set-upstream-to", base, "--", branch])?;
        self.git
            .git(repo, &["branch", "-d", "--", branch])
            .map(drop)
    }
}

/// Read the row of a worktree id from the ledger again.
pub type ReadRow<'a> = &'a dyn Fn(&str) -> Result<Option<WorktreeRow>>;

/// The `worktree_remove` job of the tree of `row`, with its landed commit
/// and its owner attempt from the ledger. `ledger` reads the row again at
/// the end of the checks. A run passes only when the tree is removed; its
/// data is the [`Removed`]. Each other end is `failed`, and the tree stays
/// as it was.
pub fn remove(
    gw: &dyn RemovalGateway,
    row: &WorktreeRow,
    landed_commit: Option<&str>,
    owner: &Owner,
    ledger: ReadRow<'_>,
) -> Ran {
    match run(gw, row, landed_commit, owner, ledger) {
        Ok(r) => Ran {
            outcome: Outcome {
                passed: true,
                verdict: true,
                text: format!(
                    "removed {} ({} ignored files deleted with it; branch {} {})",
                    r.path,
                    r.ignored_files,
                    r.branch,
                    if r.branch_deleted { "deleted" } else { "kept" }
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
    // The tree is gone: a branch that cannot be deleted stays, and the run
    // still passes.
    let branch_deleted = matches!(gw.merged(repo, &row.branch, &row.base), Ok(true))
        && gw.delete_branch(repo, &row.branch, &row.base).is_ok();
    Ok(Removed {
        path: row.path.clone(),
        branch: row.branch.clone(),
        forced: basis.force(),
        ignored_files,
        branch_deleted,
    })
}

/// The removal of a rescued tree whose checks passed. 1 rename moves the
/// folder to its quarantine folder, so a write through the old path cannot
/// change it. The checks run again on the quarantine folder, with a new
/// snapshot. Only when they pass is the folder removed, with 1 `--force`.
/// Else the folder moves back; when that fails, it stays in quarantine,
/// and the reason names the folder. Gives the count of ignored files that
/// were deleted.
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

/// Move a tree that an earlier run left in its quarantine folder back to
/// the path of `row`. A crash between the move and the removal leaves it
/// there; the rename can be ahead of the record of git, so the record is
/// repaired first. Nothing to do when no quarantine folder exists.
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

/// Read each fact of the removal of `row`, in the order of [`Facts`]. The
/// HEAD and the ledger row come last; for a rescued tree, the snapshot of
/// the folder is the last fact before them, so the least time passes
/// between it and the removal.
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
                Owner::NoAttempt => Ok(false),
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
