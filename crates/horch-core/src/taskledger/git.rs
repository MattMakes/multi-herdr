//! The git calls the task ledger makes: run branch, worktrees, land, cleanup.
//!
//! Every call names its directory with `-C`; nothing depends on the current
//! directory. No call here ever forces: no `--force`, no `-D`, no `reset`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use super::bounded::{run_capped, run_until};

/// A git binary, maybe pinned to 1 verified linked worktree. Every call
/// runs with [`SAFE_CONFIG`] and no filter program unless the caller opts
/// in to the repository's own config ([`Git::with_repo_filters`],
/// [`Git::push`]).
#[derive(Debug, Clone)]
pub struct Git {
    bin: PathBuf,
    pinned: Option<Pinned>,
    repo: RepoConfig,
    /// [`CALL_DEADLINE`] and [`CALL_OUTPUT_CAP`]; tests make them small.
    deadline: Duration,
    output_cap: usize,
}

/// How much of the repository's own config a call may run.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RepoConfig {
    /// No hook, fsmonitor or filter program: the default.
    Safe,
    /// Its filters, no hook: the merge into the run branch.
    Filters,
    /// Its hooks and filters: the push, when its hooks are the operator's.
    Hooks,
}

/// A linked worktree whose `.git` file was verified, and its git dir.
#[derive(Debug, Clone)]
struct Pinned {
    worktree: PathBuf,
    gitdir: PathBuf,
    /// The filter drivers the repository config names.
    filters: Vec<String>,
}

/// Config pairs on every git call by default: no hook, no fsmonitor, no
/// signing program, no git started in a gitlink (it would read that
/// repository's worker-writable config), and no detached maintenance after
/// a commit.
pub const SAFE_CONFIG: [&str; 7] = [
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "commit.gpgsign=false",
    "gpg.program=/usr/bin/false",
    "diff.ignoreSubmodules=all",
    "submodule.recurse=false",
    "maintenance.auto=false",
];

/// The most bytes of a worktree's `.git` file: `gitdir: ` and a path.
const MAX_DOTGIT_BYTES: u64 = 8 + 4096 + 1;

/// The most filter-driver config keys horch reads: more is refused.
const MAX_DRIVER_KEYS: usize = 1000;

/// The longest the filter-driver lookup runs.
const LOOKUP_DEADLINE: Duration = Duration::from_secs(30);

/// The longest 1 ledger git call runs, and the most bytes it may print on
/// each pipe: more is an error.
const CALL_DEADLINE: Duration = Duration::from_secs(120);
const CALL_OUTPUT_CAP: usize = 16 << 20;

/// The longest a push, and the `git lfs push` before it, runs.
const PUSH_DEADLINE: Duration = Duration::from_secs(15 * 60);

/// The names of the filter drivers in `git config --null --name-only
/// --get-regexp '^filter\.'` records, sorted, each once. A name can hold
/// dots. Refused when a name holds `=`, a newline or bytes that are not
/// UTF-8: its `-c` override would set another key, and the driver would
/// keep its program.
pub fn filter_driver_names(records: &[Vec<u8>]) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for record in records {
        let Ok(key) = std::str::from_utf8(record) else {
            bail!("refused: a filter driver name in the git config is not UTF-8");
        };
        let Some((name, _)) = key.strip_prefix("filter.").and_then(|k| k.rsplit_once('.')) else {
            continue;
        };
        if name.contains(['=', '\n', '\r']) {
            bail!(
                "refused: the filter driver name {name:?} in the git config holds '=' or a newline"
            );
        }
        names.push(name.to_string());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// The `-c` pairs that set each driver's `clean`, `smudge` and `process`
/// empty, so git runs no filter program. `horch ledger commit` refuses a
/// path whose filter is required; the ledger's own git also passes
/// `filter.<name>.required=false`, so a call never fails on a filter it
/// did not run.
pub fn filter_overrides(names: &[String]) -> Vec<String> {
    names
        .iter()
        .flat_map(|n| {
            [
                format!("filter.{n}.clean="),
                format!("filter.{n}.smudge="),
                format!("filter.{n}.process="),
            ]
        })
        .collect()
}

/// What 1 git call printed.
#[derive(Debug, Clone)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Git {
    pub fn new(bin: impl Into<PathBuf>) -> Git {
        Git {
            bin: bin.into(),
            pinned: None,
            repo: RepoConfig::Safe,
            deadline: CALL_DEADLINE,
            output_cap: CALL_OUTPUT_CAP,
        }
    }

    /// This git, with every call in `worktree` pinned to `gitdir` (from
    /// [`Git::verify_worktree`]): git never reads the worktree's `.git` file,
    /// which the sandbox can rewrite after the check. A pinned call always
    /// runs safe, whatever the caller opted in to.
    pub fn pinned(&self, worktree: &Path, gitdir: &Path) -> Result<Git> {
        let mut git = Git {
            pinned: Some(Pinned {
                worktree: worktree.to_path_buf(),
                gitdir: gitdir.to_path_buf(),
                filters: Vec::new(),
            }),
            ..self.clone()
        };
        let filters = git.filter_drivers(worktree)?;
        if let Some(p) = git.pinned.as_mut() {
            p.filters = filters;
        }
        Ok(git)
    }

    /// This git with the repository's filters and no hook: for the merge
    /// into the run branch in the main checkout, whose `.git` no sandbox
    /// can write, so its filter config is the operator's.
    pub fn with_repo_filters(&self) -> Git {
        Git {
            repo: RepoConfig::Filters,
            ..self.clone()
        }
    }

    /// The pinned worktree that `dir` is in, if any.
    fn pin_of(&self, dir: &Path) -> Option<&Pinned> {
        self.pinned
            .as_ref()
            .filter(|p| dir.starts_with(&p.worktree))
    }

    /// git in `dir` with the pin and [`SAFE_CONFIG`], and no other option.
    fn base(&self, dir: &Path) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("-C").arg(dir);
        if let Some(p) = self.pin_of(dir) {
            cmd.env("GIT_DIR", &p.gitdir)
                .env("GIT_WORK_TREE", &p.worktree);
        }
        for pair in SAFE_CONFIG {
            cmd.args(["-c", pair]);
        }
        cmd.env_remove("ANTHROPIC_API_KEY");
        cmd
    }

    /// The filter drivers the config of `dir` names, read by the bounded
    /// runner: a hung git or a huge config fails, it never blocks finish.
    fn filter_drivers(&self, dir: &Path) -> Result<Vec<String>> {
        let mut cmd = self.base(dir);
        cmd.args([
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            r"^filter\.",
        ]);
        let out = run_until(cmd, Vec::new(), MAX_DRIVER_KEYS, LOOKUP_DEADLINE)
            .context("listing the filter drivers")?;
        // Exit 1 with no output: no driver is configured.
        if out.status.code() == Some(1) && out.records.is_empty() {
            return Ok(Vec::new());
        }
        if !out.status.success() || !out.complete {
            bail!(
                "refused: cannot list the filter drivers of the git config in {} ({})",
                dir.display(),
                out.status
            );
        }
        filter_driver_names(&out.records)
    }

    /// git `args` in `dir`, as safe as the opt-in allows; a pinned call is
    /// always safe.
    fn command(&self, dir: &Path, args: &[&str]) -> Result<Command> {
        let pin = self.pin_of(dir);
        let repo = if pin.is_some() {
            RepoConfig::Safe
        } else {
            self.repo
        };
        let mut cmd = match repo {
            RepoConfig::Hooks => {
                let mut cmd = Command::new(&self.bin);
                cmd.arg("-C").arg(dir).env_remove("ANTHROPIC_API_KEY");
                cmd
            }
            RepoConfig::Filters => self.base(dir),
            RepoConfig::Safe => self.safe(dir, pin)?,
        };
        cmd.args(args);
        Ok(cmd)
    }

    /// git in `dir` that runs no hook, fsmonitor or filter program: every
    /// driver the config names gets empty programs and is not required.
    fn safe(&self, dir: &Path, pin: Option<&Pinned>) -> Result<Command> {
        let filters = match pin {
            Some(p) => p.filters.clone(),
            None => self.filter_drivers(dir)?,
        };
        let mut cmd = self.base(dir);
        for pair in filter_overrides(&filters) {
            cmd.arg("-c").arg(pair);
        }
        for name in &filters {
            cmd.arg("-c").arg(format!("filter.{name}.required=false"));
        }
        Ok(cmd)
    }

    /// Push `refspec` from the main checkout `root` to `remote`. The push
    /// runs the repository's hooks (git-lfs uploads in `pre-push`) only when
    /// they are the operator's: `core.hooksPath` unset, or a real folder
    /// under the common git dir. Otherwise it runs none, the returned note
    /// says why, and git-lfs objects go first with `git lfs push`.
    pub fn push(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        args: &[&str],
    ) -> Result<(Out, Option<String>)> {
        let mut push = vec!["push"];
        push.extend_from_slice(args);
        push.push(remote);
        let refspec = format!("{branch}:{branch}");
        push.push(&refspec);
        let Some(skipped) = self.foreign_hooks(root)? else {
            let git = Git {
                repo: RepoConfig::Hooks,
                ..self.clone()
            };
            return Ok((git.push_out(root, &push), None));
        };
        let note = format!("hooks skipped: {skipped}");
        eprintln!("horch: {note}");
        if self.uses_lfs(root)? {
            let lfs = self.lfs_push(root, remote, branch)?;
            if !lfs.ok {
                return Ok((lfs, Some(note)));
            }
        }
        Ok((self.push_out(root, &push), Some(note)))
    }

    /// Run the push `args` in `root` within [`PUSH_DEADLINE`]: a timeout or
    /// too much output is a failed push, never a hung finish.
    fn push_out(&self, root: &Path, args: &[&str]) -> Out {
        self.try_run_within(root, args, PUSH_DEADLINE)
            .unwrap_or_else(|e| Out {
                ok: false,
                stdout: String::new(),
                stderr: format!("{e:#}"),
            })
    }

    /// Why the hooks of the main checkout `root` are not the operator's, or
    /// `None` when they are.
    fn foreign_hooks(&self, root: &Path) -> Result<Option<String>> {
        let Some(path) = self.config_value(root, "core.hooksPath")? else {
            return Ok(None);
        };
        let path = path.as_str();
        let root = std::fs::canonicalize(root)
            .with_context(|| format!("{} does not exist", root.display()))?;
        let common = self
            .run(
                &root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .ok()
            .and_then(|c| std::fs::canonicalize(c.trim()).ok());
        Ok(hooks_verdict(path, &root, common.as_deref()))
    }

    /// Whether the config of `root` sets a git-lfs filter.
    fn uses_lfs(&self, root: &Path) -> Result<bool> {
        for key in ["filter.lfs.process", "filter.lfs.clean"] {
            if self.config_value(root, key)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The value of `key` in the config of `dir`, or `None` when unset or
    /// empty. Read without [`SAFE_CONFIG`] and the filter overrides, which
    /// set some of these keys; reading config runs no program.
    fn config_value(&self, dir: &Path, key: &str) -> Result<Option<String>> {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("-C")
            .arg(dir)
            .args(["config", "--null", "--get", key])
            .env_remove("ANTHROPIC_API_KEY");
        let out = run_until(cmd, Vec::new(), 1, LOOKUP_DEADLINE)
            .with_context(|| format!("reading {key}"))?;
        let value = String::from_utf8_lossy(&out.records.concat()).into_owned();
        Ok((out.status.success() && !value.is_empty()).then_some(value))
    }

    /// `git lfs push <remote> <branch>` in `root`, bounded; its output goes
    /// to stderr, which is kept for the error.
    fn lfs_push(&self, root: &Path, remote: &str, branch: &str) -> Result<Out> {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "exec \"$@\" >&2", "sh"])
            .arg(&self.bin)
            .arg("-C")
            .arg(root)
            .args(["lfs", "push", remote, branch])
            .env_remove("ANTHROPIC_API_KEY");
        let out = run_until(cmd, Vec::new(), 1, PUSH_DEADLINE).context("running git lfs push")?;
        Ok(Out {
            ok: out.status.success() && out.complete,
            stdout: String::new(),
            stderr: format!(
                "git lfs push {remote} {branch}: {}",
                String::from_utf8_lossy(&out.stderr).trim_end()
            ),
        })
    }

    /// Run git in `dir`; never fails on a non-zero exit.
    pub fn try_run(&self, dir: &Path, args: &[&str]) -> Result<Out> {
        self.try_run_within(dir, args, self.deadline)
    }

    /// [`Git::try_run`] that may take `limit`; more, or more output than
    /// the cap on a pipe, is an error.
    fn try_run_within(&self, dir: &Path, args: &[&str], limit: Duration) -> Result<Out> {
        debug_assert!(
            !args
                .iter()
                .any(|a| *a == "--force" || *a == "-D" || *a == "-f"),
            "the task ledger never forces git"
        );
        let out = run_capped(self.command(dir, args)?, limit, self.output_cap)
            .with_context(|| format!("running {} {}", self.bin.display(), args.join(" ")))?;
        Ok(Out {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
        })
    }

    /// Run git in `dir`; a non-zero exit is an error with git's message.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = self.try_run(dir, args)?;
        if !out.ok {
            bail!(
                "git {} failed in {}: {}",
                args.join(" "),
                dir.display(),
                if out.stderr.is_empty() {
                    &out.stdout
                } else {
                    &out.stderr
                }
            );
        }
        Ok(out.stdout)
    }

    /// The main checkout of the repository `dir` is in (also from inside a
    /// linked worktree), or `None` outside a repository.
    pub fn main_root(&self, dir: &Path) -> Option<PathBuf> {
        let common = self
            .try_run(
                dir,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .ok()
            .filter(|o| o.ok)?
            .stdout;
        let common = PathBuf::from(common.trim());
        let root = if common.file_name().is_some_and(|n| n == ".git") {
            common.parent()?.to_path_buf()
        } else {
            common
        };
        Some(root.canonicalize().unwrap_or(root))
    }

    /// The git dir of the linked worktree `worktree` of the main checkout
    /// `repo_root`, from trusted metadata only (see [`worktree_gitdir`]).
    /// Refused when the worktree's `.git` was changed: no other git may run
    /// there, since a planted git dir sets the config git runs with.
    pub fn verify_worktree(&self, repo_root: &Path, worktree: &Path) -> Result<PathBuf> {
        let common = self.run(
            repo_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let root = std::fs::canonicalize(worktree).with_context(|| {
            format!(
                "refused: the worktree {} does not exist",
                worktree.display()
            )
        })?;
        worktree_gitdir(Path::new(common.trim()), worktree, &root)
    }

    /// The branch checked out in `dir`, if any.
    pub fn current_branch(&self, dir: &Path) -> Option<String> {
        self.try_run(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok()
            .filter(|o| o.ok && !o.stdout.is_empty())
            .map(|o| o.stdout)
    }

    /// Whether `name` is a local branch.
    pub fn branch_exists(&self, dir: &Path, name: &str) -> bool {
        self.try_run(
            dir,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ],
        )
        .map(|o| o.ok)
        .unwrap_or(false)
    }

    /// Whether `remote` is configured.
    pub fn has_remote(&self, dir: &Path, remote: &str) -> bool {
        self.try_run(dir, &["remote", "get-url", remote])
            .map(|o| o.ok)
            .unwrap_or(false)
    }

    /// The commit `rev` names.
    pub fn rev(&self, dir: &Path, rev: &str) -> Result<String> {
        self.run(
            dir,
            &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        )
    }

    /// The tree `HEAD` holds in `dir`: what a finish step judges.
    pub fn tree(&self, dir: &Path) -> Result<String> {
        self.run(dir, &["rev-parse", "--verify", "HEAD^{tree}"])
    }

    /// Whether `ancestor` is reachable from `of`.
    pub fn is_ancestor(&self, dir: &Path, ancestor: &str, of: &str) -> bool {
        self.try_run(dir, &["merge-base", "--is-ancestor", ancestor, of])
            .map(|o| o.ok)
            .unwrap_or(false)
    }

    /// The files `git status --porcelain` lists in `dir`: modified, staged and
    /// untracked (ignored files are not listed).
    pub fn dirty(&self, dir: &Path) -> Result<Vec<String>> {
        let out = self.run(dir, &["status", "--porcelain", "--untracked-files=all"])?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    /// The stash entries whose message names `branch`.
    pub fn stashes_on(&self, dir: &Path, branch: &str) -> Result<Vec<String>> {
        let out = self.run(dir, &["stash", "list"])?;
        let needle_on = format!("On {branch}:");
        let needle_wip = format!("WIP on {branch}:");
        Ok(out
            .lines()
            .filter(|l| l.contains(&needle_on) || l.contains(&needle_wip))
            .map(|l| l.to_string())
            .collect())
    }

    /// The commits on `branch` that `base` does not contain.
    pub fn commits_missing_from(
        &self,
        dir: &Path,
        branch: &str,
        base: &str,
    ) -> Result<Vec<String>> {
        let out = self.run(dir, &["rev-list", &format!("{base}..{branch}")])?;
        Ok(out.lines().map(|l| l.to_string()).collect())
    }
}

/// The worktree's git dir, from trusted metadata only: the 1 entry under
/// `<common>/worktrees` whose `gitdir` file names the recorded `worktree`
/// (`worktree_canonical` is its canonical path, never the main checkout). Not its folder name: git appends a number
/// when that name is taken. Refused unless the worktree's `.git` is a file
/// that says exactly `gitdir: <that entry>`.
pub fn worktree_gitdir(
    common: &Path,
    worktree: &Path,
    worktree_canonical: &Path,
) -> Result<PathBuf> {
    let gitdir = worktree_entry(common, worktree, worktree_canonical)?;
    check_dotgit(worktree_canonical, &gitdir)?;
    Ok(gitdir)
}

/// The 1 entry under `<common>/worktrees` whose `gitdir` file names the
/// recorded worktree.
fn worktree_entry(common: &Path, worktree: &Path, worktree_canonical: &Path) -> Result<PathBuf> {
    let recorded = format!("{}/.git", worktree.display());
    let names_worktree = |entry: &Path| {
        std::fs::read_to_string(entry.join("gitdir")).is_ok_and(|t| {
            let t = t.trim();
            // With `worktree.useRelativePaths`, git writes the path relative
            // to the entry; `join` keeps an absolute path as it is.
            t == recorded
                || std::fs::canonicalize(entry.join(t))
                    .is_ok_and(|p| p == worktree_canonical.join(".git"))
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
fn check_dotgit(worktree_canonical: &Path, gitdir: &Path) -> Result<()> {
    let dotgit = worktree_canonical.join(".git");
    // At most a path's length: a huge file fails here, never in memory.
    let text = std::fs::symlink_metadata(&dotgit)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_DOTGIT_BYTES)
        .and_then(|_| std::fs::read_to_string(&dotgit).ok())
        .filter(|t| t.len() as u64 <= MAX_DOTGIT_BYTES)
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
    if relative
        .and_then(|r| lexical_join(worktree_canonical, r))
        .is_some_and(|p| p == canonical)
    {
        return Ok(());
    }
    bail!(
        "refused: {} was changed; it must say exactly: gitdir: {}",
        dotgit.display(),
        gitdir.display()
    )
}

/// `relative` joined to the canonical `base` with no symlink followed: only
/// leading `..` components, then plain names. The result names a real path
/// only when it equals a canonical one, which has no symlink in it.
fn lexical_join(base: &Path, relative: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = base.to_path_buf();
    let mut names = false;
    for c in relative.components() {
        match c {
            Component::ParentDir if !names => {
                out.pop();
            }
            Component::Normal(n) => {
                names = true;
                out.push(n);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Why `core.hooksPath` `path` of the main checkout `root` (canonical) is
/// not the operator's, or `None` when it is a real folder under the
/// canonical common git dir `common`. A common dir that could not be
/// resolved trusts no hooks path: an empty path would prefix every path.
fn hooks_verdict(path: &str, root: &Path, common: Option<&Path>) -> Option<String> {
    let Some(common) = common.filter(|c| c.is_absolute()) else {
        return Some(format!(
            "core.hooksPath {path} is set and the git dir cannot be resolved"
        ));
    };
    // Relative to the top of the working tree, as git runs hooks.
    let lexical = normalize(&root.join(path));
    let real = std::fs::canonicalize(&lexical).ok();
    if real.as_ref() == Some(&lexical) && lexical.starts_with(common) {
        return None;
    }
    let place = if lexical.starts_with(root) && !lexical.starts_with(common) {
        "is inside the working tree"
    } else {
        "is not a real folder under the git dir"
    };
    Some(format!("core.hooksPath {path} {place}"))
}

/// `path` with `.` and `..` resolved by name, no symlink followed.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The size of a directory tree in bytes, following no links.
pub fn dir_bytes(path: &Path) -> u64 {
    fn walk(p: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return 0;
        };
        if meta.is_dir() {
            std::fs::read_dir(p)
                .map(|rd| rd.flatten().map(|e| walk(&e.path())).sum())
                .unwrap_or(0)
        } else {
            meta.len()
        }
    }
    walk(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn marker_script(path: &Path, marker: &Path, then: &str) {
        std::fs::write(
            path,
            format!("#!/bin/sh\n: > '{}'\n{then}\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A main checkout with 1 commit and a linked worktree on branch `f`.
    fn repo(dir: &Path) -> (PathBuf, PathBuf) {
        let main = dir.join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "a"]);
        let wt = dir.join("wt");
        git(
            &main,
            &["worktree", "add", "-q", "-b", "f", &wt.to_string_lossy()],
        );
        (main.canonicalize().unwrap(), wt.canonicalize().unwrap())
    }

    #[test]
    fn a_call_in_a_worktree_with_no_pin_runs_no_hook_fsmonitor_or_filter() {
        let dir = tempfile::tempdir().unwrap();
        let (main, wt) = repo(dir.path());
        let ran = dir.path().join("ran");
        let prog = dir.path().join("prog.sh");
        marker_script(&prog, &ran, "cat");
        let prog = prog.to_string_lossy().into_owned();
        // A dotted driver name, and every program a status or commit runs.
        git(&main, &["config", "filter.a.b.clean", &prog]);
        git(&main, &["config", "filter.a.b.process", &prog]);
        git(&main, &["config", "core.fsmonitor", &prog]);
        let hooks = main.join(".git/hooks");
        marker_script(&hooks.join("pre-commit"), &ran, "");
        std::fs::write(wt.join(".gitattributes"), "*.txt filter=a.b\n").unwrap();
        std::fs::write(wt.join("x.txt"), "x\n").unwrap();
        let g = Git::new("git");
        g.run(&wt, &["add", ".gitattributes", "x.txt"]).unwrap();
        g.run(&wt, &["commit", "-q", "-m", "x"]).unwrap();
        g.run(&wt, &["status", "--porcelain"]).unwrap();
        assert!(!ran.exists(), "no hook, fsmonitor or filter program ran");
    }

    #[test]
    fn a_git_dir_that_cannot_be_resolved_trusts_no_hooks_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let common = root.join(".git");
        std::fs::create_dir_all(common.join("hooks")).unwrap();
        assert_eq!(hooks_verdict(".git/hooks", &root, Some(&common)), None);
        for unresolved in [None, Some(Path::new(""))] {
            let why = hooks_verdict(".git/hooks", &root, unresolved).unwrap();
            assert!(why.contains("the git dir cannot be resolved"), "{why}");
        }
    }

    #[test]
    fn a_filter_driver_name_that_would_break_its_override_is_refused() {
        let ok = filter_driver_names(&[b"filter.a.b.clean".to_vec(), b"filter.x.smudge".to_vec()]);
        assert_eq!(ok.unwrap(), vec!["a.b".to_string(), "x".to_string()]);
        for bad in [
            &b"filter.a=b.clean"[..],
            b"filter.a\nb.clean",
            b"filter.a\xffb.clean",
        ] {
            assert!(filter_driver_names(&[bad.to_vec()]).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_ledger_git_call_that_hangs_or_floods_fails_inside_its_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("git");
        let mut g = Git::new(&stub);
        g.deadline = Duration::from_millis(300);
        g.output_cap = 1024;
        // Outside a repository: the filter lookup gets no output and exit 1.
        marker_script(
            &stub,
            &dir.path().join("ran"),
            "case \"$*\" in *config*) exit 1;; esac\nsleep 30",
        );
        let started = std::time::Instant::now();
        let e = g.try_run(dir.path(), &["status"]).unwrap_err();
        assert!(format!("{e:#}").contains("ran longer than"), "{e:#}");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        marker_script(
            &stub,
            &dir.path().join("ran"),
            "case \"$*\" in *config*) exit 1;; esac\nhead -c 100000 /dev/zero",
        );
        let e = g.try_run(dir.path(), &["status"]).unwrap_err();
        assert!(format!("{e:#}").contains("more than 1024 bytes"), "{e:#}");
    }

    #[test]
    fn a_pinned_rebase_runs_no_smudge_program() {
        let dir = tempfile::tempdir().unwrap();
        let (main, wt) = repo(dir.path());
        // main gets a file whose attributes select a driver with a smudge.
        std::fs::write(main.join(".gitattributes"), "*.txt filter=x\n").unwrap();
        std::fs::write(main.join("y.txt"), "y\n").unwrap();
        git(&main, &["add", ".gitattributes", "y.txt"]);
        git(&main, &["commit", "-q", "-m", "y"]);
        let ran = dir.path().join("smudged");
        let prog = dir.path().join("smudge.sh");
        marker_script(&prog, &ran, "cat");
        git(
            &main,
            &["config", "filter.x.smudge", &prog.to_string_lossy()],
        );
        let g = Git::new("git");
        let gitdir = g.verify_worktree(&main, &wt).unwrap();
        let pinned = g.pinned(&wt, &gitdir).unwrap();
        pinned.run(&wt, &["rebase", "main"]).unwrap();
        assert!(wt.join("y.txt").exists(), "the rebase checked y.txt out");
        assert!(!ran.exists(), "the smudge program never runs");
    }
}
