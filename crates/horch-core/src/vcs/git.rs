//! A typed git client (phase B2). [`GitClient`] is the seam: the worktree
//! manager, the validator and the promotion code call it, and a test can put
//! a fake behind it. [`GitCli`] is the real one; every process it starts goes
//! through [`horch_marketplace::git::GitRunner`], which removes the API key
//! and the repo-locating `GIT_*` variables and turns hooks off.
//!
//! Every argument that names a ref, a revision or a range is refused when it
//! starts with `-`, so a label or a branch name can never become an option.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context};
use horch_marketplace::git::{GitOutput, GitRunner};
use sha2::{Digest as _, Sha256};

use crate::measure::digest::Digest;
use crate::measure::NumstatLine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryPick {
    Clean {
        head: String,
    },
    /// The pick stopped on these paths. It is aborted again, so the
    /// checkout is as it was before the call.
    Conflict {
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutLocation {
    NotCheckedOut,
    CheckedOut { path: PathBuf, clean: bool },
}

/// The author and committer of a commit horch makes. The same identity and
/// date over the same tree and parent give the same commit hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
    /// Any date git accepts; horch passes RFC 3339.
    pub date: String,
}

pub trait GitClient {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf>;
    fn head(&self, dir: &Path) -> anyhow::Result<String>;
    /// `None` on a detached HEAD.
    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>>;
    fn status_porcelain(&self, dir: &Path) -> anyhow::Result<String>;
    fn version(&self) -> anyhow::Result<String>;
    /// Check `branch` out at `path`; the branch is created at `base` when it
    /// does not exist yet.
    fn worktree_add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> anyhow::Result<()>;
    fn worktree_remove(&self, repo: &Path, path: &Path, force: bool) -> anyhow::Result<()>;
    /// Every worktree of `repo` with its short branch name (`None` when
    /// detached or bare).
    fn worktree_list(&self, repo: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>>;
    /// Hooks off, no gpg, deterministic identity and date. `None` when there
    /// is nothing to commit.
    fn commit_all(
        &self,
        dir: &Path,
        message: &str,
        id: &GitIdentity,
    ) -> anyhow::Result<Option<String>>;
    /// The full hash `rev` names, or `None` when it names nothing.
    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>>;
    fn rev_list(&self, dir: &Path, range: &str) -> anyhow::Result<Vec<String>>;
    fn diff_numstat(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Vec<NumstatLine>>;
    /// The patch, cut at `cap_bytes` on a UTF-8 boundary, and whether it was
    /// cut.
    fn diff_patch(
        &self,
        dir: &Path,
        base: &str,
        head: &str,
        cap_bytes: usize,
    ) -> anyhow::Result<(String, bool)>;
    /// sha256 of the whole patch [`GitClient::diff_patch`] would give
    /// uncapped. Deviation from the design's trait: freeze needs it, and the
    /// patch can be larger than memory should hold.
    fn diff_digest(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Digest>;
    fn is_ancestor(&self, dir: &Path, a: &str, b: &str) -> anyhow::Result<bool>;
    /// `update-ref <ref> <new> <expected_old>`: compare and swap. `false`
    /// when the ref does not hold `expected_old`.
    fn update_ref_cas(
        &self,
        repo: &Path,
        refname: &str,
        new: &str,
        expected_old: &str,
    ) -> anyhow::Result<bool>;
    fn cherry_pick(&self, dir: &Path, range: &str, id: &GitIdentity) -> anyhow::Result<CherryPick>;
    /// `read-tree -m -u <old> <new>` in the checkout `dir`: move its index
    /// and files from the tree of `old` to the tree of `new`. HEAD and refs
    /// do not move. A local change in a path that differs between the two
    /// makes git refuse before it writes anything; that is an error. The
    /// default refuses, so a fake client does not need it.
    fn read_tree_update(&self, dir: &Path, old: &str, new: &str) -> anyhow::Result<()> {
        let _ = (dir, old, new);
        bail!("this git client cannot update a checkout")
    }
    fn branch_checkout_location(
        &self,
        repo: &Path,
        branch: &str,
    ) -> anyhow::Result<CheckoutLocation>;
}

/// Wraps [`GitRunner`]. `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, `FORBIDDEN_ENV`
/// stripped. The caller passes the git binary (later from `RuntimeContext`,
/// `HORCH_GIT_BIN`); nothing here reads the environment.
#[derive(Debug, Clone)]
pub struct GitCli {
    runner: GitRunner,
}

/// The diff options every patch and digest uses, so the bytes do not depend
/// on the operator's diff config.
const PATCH_ARGS: [&str; 8] = [
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--full-index",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Makes the scratch patch file names unique within this process.
static SCRATCH: AtomicU64 = AtomicU64::new(0);

impl GitCli {
    pub fn new(git_bin: PathBuf) -> Self {
        Self {
            runner: GitRunner::new(git_bin)
                .with_env("GIT_TERMINAL_PROMPT", "0")
                .with_env("LC_ALL", "C"),
        }
    }

    /// Add one variable to every git child, for example `GIT_CONFIG_GLOBAL`
    /// in a hermetic test.
    pub fn with_env(self, key: &str, value: &str) -> Self {
        Self {
            runner: self.runner.with_env(key, value),
        }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> anyhow::Result<GitOutput> {
        Ok(self.runner.run(dir, args)?)
    }

    fn text(&self, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
        Ok(self.run(dir, args)?.stdout_text())
    }

    /// A runner that commits as `id`. The author is set too unless the
    /// command keeps the original author (cherry-pick).
    fn as_identity(&self, id: &GitIdentity, author: bool) -> GitRunner {
        let mut r = self
            .runner
            .clone()
            .with_env("GIT_COMMITTER_NAME", &id.name)
            .with_env("GIT_COMMITTER_EMAIL", &id.email)
            .with_env("GIT_COMMITTER_DATE", &id.date);
        if author {
            r = r
                .with_env("GIT_AUTHOR_NAME", &id.name)
                .with_env("GIT_AUTHOR_EMAIL", &id.email)
                .with_env("GIT_AUTHOR_DATE", &id.date);
        }
        r
    }

    /// Write the `base..head` patch to a scratch file inside the git dir, so
    /// it is read in pieces and never lands in the work tree.
    fn patch_file(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Scratch> {
        refuse_option(base)?;
        refuse_option(head)?;
        let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let name = format!("horch-diff-{}-{n}.patch", std::process::id());
        let rel = self.text(dir, &["rev-parse", "--git-path", &name])?;
        let scratch = Scratch(dir.join(rel));
        let output = format!("--output={}", path_str(&scratch.0)?);
        let mut args = PATCH_ARGS.to_vec();
        args.extend([output.as_str(), base, head]);
        self.run(dir, &args)?;
        Ok(scratch)
    }
}

/// A scratch file, removed on drop.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn refuse_option(arg: &str) -> anyhow::Result<()> {
    if arg.is_empty() || arg.starts_with('-') {
        bail!("'{arg}' is not a git revision or ref name");
    }
    Ok(())
}

fn path_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

/// The longest prefix of `s` that is at most `cap` bytes and ends on a char
/// boundary.
pub(crate) fn cut_utf8(s: &str, cap: usize) -> &str {
    if s.len() <= cap {
        return s;
    }
    let mut end = cap;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl GitClient for GitCli {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        Ok(PathBuf::from(
            self.text(dir, &["rev-parse", "--show-toplevel"])?,
        ))
    }

    fn head(&self, dir: &Path) -> anyhow::Result<String> {
        self.text(dir, &["rev-parse", "--verify", "HEAD"])
    }

    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>> {
        let out = self
            .runner
            .output(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        match out.status {
            0 => Ok(Some(out.stdout_text())),
            1 => Ok(None),
            s => bail!(
                "git symbolic-ref HEAD failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn status_porcelain(&self, dir: &Path) -> anyhow::Result<String> {
        self.text(dir, &["status", "--porcelain"])
    }

    fn version(&self) -> anyhow::Result<String> {
        self.text(Path::new("."), &["version"])
    }

    fn worktree_add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> anyhow::Result<()> {
        refuse_option(branch)?;
        refuse_option(base)?;
        let path = path_str(path)?;
        let local = format!("refs/heads/{branch}");
        if self.rev_parse(repo, &local)?.is_some() {
            self.run(repo, &["worktree", "add", path, branch])?;
        } else {
            self.run(repo, &["worktree", "add", "-b", branch, path, base])?;
        }
        Ok(())
    }

    fn worktree_remove(&self, repo: &Path, path: &Path, force: bool) -> anyhow::Result<()> {
        let path = path_str(path)?;
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path);
        self.run(repo, &args)?;
        Ok(())
    }

    fn worktree_list(&self, repo: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>> {
        let text = self.text(repo, &["worktree", "list", "--porcelain"])?;
        let mut out = Vec::new();
        for block in text.split("\n\n") {
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(p));
                } else if let Some(b) = line.strip_prefix("branch ") {
                    branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_owned());
                }
            }
            if let Some(path) = path {
                out.push((path, branch));
            }
        }
        Ok(out)
    }

    fn commit_all(
        &self,
        dir: &Path,
        message: &str,
        id: &GitIdentity,
    ) -> anyhow::Result<Option<String>> {
        let r = self.as_identity(id, true);
        r.run(dir, &["-c", "commit.gpgsign=false", "add", "-A"])?;
        let staged = r.output(dir, &["diff", "--cached", "--quiet"])?;
        match staged.status {
            0 => return Ok(None),
            1 => {}
            s => bail!(
                "git diff --cached failed ({s}): {}",
                String::from_utf8_lossy(&staged.stderr).trim()
            ),
        }
        r.run(
            dir,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--no-verify",
                "--allow-empty-message",
                "--cleanup=strip",
                "-m",
                message,
            ],
        )?;
        self.head(dir).map(Some)
    }

    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>> {
        refuse_option(rev)?;
        let out = self
            .runner
            .output(dir, &["rev-parse", "--verify", "--quiet", rev])?;
        match out.status {
            0 => Ok(Some(out.stdout_text())),
            1 => Ok(None),
            s => bail!(
                "git rev-parse {rev} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn rev_list(&self, dir: &Path, range: &str) -> anyhow::Result<Vec<String>> {
        refuse_option(range)?;
        let text = self.text(dir, &["rev-list", range])?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    fn diff_numstat(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Vec<NumstatLine>> {
        refuse_option(base)?;
        refuse_option(head)?;
        let out = self.run(
            dir,
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                base,
                head,
            ],
        )?;
        // `-z`: `<added>\t<deleted>\t<path>\0`, the path unquoted.
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = Vec::new();
        for record in text.split('\0').filter(|r| !r.is_empty()) {
            let mut parts = record.splitn(3, '\t');
            let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                bail!("unexpected git numstat record '{record}'");
            };
            let count = |s: &str| -> anyhow::Result<Option<u32>> {
                if s == "-" {
                    Ok(None)
                } else {
                    Ok(Some(s.parse().with_context(|| {
                        format!("bad numstat count in '{record}'")
                    })?))
                }
            };
            lines.push(NumstatLine {
                added: count(a)?,
                deleted: count(d)?,
                path: path.to_owned(),
            });
        }
        Ok(lines)
    }

    fn diff_patch(
        &self,
        dir: &Path,
        base: &str,
        head: &str,
        cap_bytes: usize,
    ) -> anyhow::Result<(String, bool)> {
        let scratch = self.patch_file(dir, base, head)?;
        let file = std::fs::File::open(&scratch.0)
            .with_context(|| format!("opening {}", scratch.0.display()))?;
        let total = file.metadata()?.len();
        let mut bytes = Vec::new();
        file.take(cap_bytes as u64).read_to_end(&mut bytes)?;
        let truncated = total > cap_bytes as u64;
        if truncated {
            // Drop a char the cap cut in half rather than mangle it.
            if let Err(e) = std::str::from_utf8(&bytes) {
                if e.error_len().is_none() {
                    bytes.truncate(e.valid_up_to());
                }
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        // A lossy replacement is longer than the byte it replaces.
        Ok((cut_utf8(&text, cap_bytes).to_owned(), truncated))
    }

    fn diff_digest(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Digest> {
        let scratch = self.patch_file(dir, base, head)?;
        let mut file = std::fs::File::open(&scratch.0)
            .with_context(|| format!("opening {}", scratch.0.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Digest(hasher.finalize().into()))
    }

    fn is_ancestor(&self, dir: &Path, a: &str, b: &str) -> anyhow::Result<bool> {
        refuse_option(a)?;
        refuse_option(b)?;
        let out = self
            .runner
            .output(dir, &["merge-base", "--is-ancestor", a, b])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            s => bail!(
                "git merge-base --is-ancestor {a} {b} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn update_ref_cas(
        &self,
        repo: &Path,
        refname: &str,
        new: &str,
        expected_old: &str,
    ) -> anyhow::Result<bool> {
        refuse_option(refname)?;
        refuse_option(new)?;
        refuse_option(expected_old)?;
        let out = self
            .runner
            .output(repo, &["update-ref", refname, new, expected_old])?;
        if out.status == 0 {
            return Ok(true);
        }
        // Tell a lost race from a real failure by what the ref holds now.
        let current = self.rev_parse(repo, refname)?;
        let held = match &current {
            None => expected_old.bytes().all(|c| c == b'0'),
            Some(c) => c == expected_old,
        };
        if held {
            bail!(
                "git update-ref {refname} failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(false)
    }

    fn cherry_pick(&self, dir: &Path, range: &str, id: &GitIdentity) -> anyhow::Result<CherryPick> {
        refuse_option(range)?;
        let r = self.as_identity(id, false);
        let out = r.output(dir, &["-c", "commit.gpgsign=false", "cherry-pick", range])?;
        if out.status == 0 {
            return Ok(CherryPick::Clean {
                head: self.head(dir)?,
            });
        }
        let unmerged = self.run(dir, &["diff", "--name-only", "--diff-filter=U", "-z"])?;
        let paths: Vec<String> = String::from_utf8_lossy(&unmerged.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        let abort = self.runner.output(dir, &["cherry-pick", "--abort"])?;
        if paths.is_empty() {
            bail!(
                "git cherry-pick {range} failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        if abort.status != 0 {
            bail!(
                "git cherry-pick --abort failed ({}): {}",
                abort.status,
                String::from_utf8_lossy(&abort.stderr).trim()
            );
        }
        Ok(CherryPick::Conflict { paths })
    }

    fn read_tree_update(&self, dir: &Path, old: &str, new: &str) -> anyhow::Result<()> {
        refuse_option(old)?;
        refuse_option(new)?;
        // Stale stat data would make read-tree call a clean file changed.
        // Exit 1 only means some file differs; read-tree decides on that.
        self.runner
            .output(dir, &["update-index", "-q", "--refresh"])?;
        self.run(dir, &["read-tree", "-m", "-u", old, new])?;
        Ok(())
    }

    fn branch_checkout_location(
        &self,
        repo: &Path,
        branch: &str,
    ) -> anyhow::Result<CheckoutLocation> {
        let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
        let found = self
            .worktree_list(repo)?
            .into_iter()
            .find(|(_, b)| b.as_deref() == Some(short));
        match found {
            None => Ok(CheckoutLocation::NotCheckedOut),
            Some((path, _)) => {
                let clean = self.status_porcelain(&path)?.is_empty();
                Ok(CheckoutLocation::CheckedOut { path, clean })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_utf8_stops_on_a_char_boundary() {
        assert_eq!(cut_utf8("abc", 5), "abc");
        assert_eq!(cut_utf8("abc", 2), "ab");
        // 'é' is 2 bytes; a cap inside it drops it whole.
        assert_eq!(cut_utf8("aé", 2), "a");
        assert_eq!(cut_utf8("aé", 3), "aé");
    }

    #[test]
    fn option_shaped_revisions_are_refused() {
        assert!(refuse_option("--upload-pack=x").is_err());
        assert!(refuse_option("").is_err());
        assert!(refuse_option("main").is_ok());
        assert!(refuse_option("a..b").is_ok());
    }
}
