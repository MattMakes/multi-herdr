//! The integration git gateway (ticket 23): [`CliGitGateway`], the 1
//! adapter of [`IntegrationGitGateway`].
//!
//! It runs each git call through [`CliWorktreeGit::checked`], the fleetd
//! git rule (Q26, review F4 of T-607): the platform sandbox, hooks,
//! fsmonitor, signing, editors and prompts off, and a stop when the project
//! config sets a key that starts a program. `git merge-tree` can start a
//! merge driver of the project, so the candidate build needs that rule.
//! The caller gives an adapter that can write the git folder of the
//! repository (`writing(&[repo_git_dir(..)])`).
//!
//! No call here uses the network (F11), a working tree or the index.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Result};

use crate::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use crate::fleetd::integration::application::stacks::StackGit;
use crate::fleetd::worktree_custody::adapters::git::CliWorktreeGit;

/// The reflog message of each ref move of fleetd.
const MOVE_MESSAGE: &str = "fleetd: compare-and-set";

/// The identity of each candidate merge commit. The global config is not
/// read, so git has no identity of the user.
const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=horch fleetd",
    "-c",
    "user.email=fleetd@horch.invalid",
];

/// The settings that fleetd sets in each repository and worktree that it
/// manages. A key with another text, also `yes` for `true`, is set again.
const SETTINGS: [(&str, &str); 2] = [
    ("rerere.enabled", "true"),
    ("merge.conflictStyle", "zdiff3"),
];

/// The branches of the fleet.
const FLEET_BRANCHES: &str = "refs/heads/fleet/";

/// Each local branch.
const LOCAL_BRANCHES: &str = "refs/heads/";

/// [`IntegrationGitGateway`] on the git binary.
#[derive(Debug, Clone)]
pub struct CliGitGateway {
    git: CliWorktreeGit,
}

impl CliGitGateway {
    pub fn new(git: CliWorktreeGit) -> CliGitGateway {
        CliGitGateway { git }
    }

    /// Each ref that `pattern` matches (`git for-each-ref` rules), with its
    /// tip, sorted by name.
    fn refs(&self, repo: &Path, pattern: &str) -> Result<Vec<(String, String)>> {
        let out = self.git.git(
            repo,
            &["for-each-ref", "--format=%(objectname) %(refname)", pattern],
        )?;
        Ok(out
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(tip, name)| (name.to_string(), tip.to_string()))
            .collect())
    }

    /// The outcome of a failed `git update-ref` on `name`: a mismatch when
    /// the ref is not at `expected_old`, else the error of git.
    fn failed_move(
        &self,
        repo: &Path,
        name: &str,
        expected_old: Option<&str>,
        stderr: &[u8],
    ) -> Result<RefMove> {
        let actual = self.tip(repo, name)?;
        if actual.as_deref() == expected_old {
            bail!(
                "git update-ref {name} failed with the ref at the expected tip: {}",
                String::from_utf8_lossy(stderr).trim()
            );
        }
        Ok(RefMove::Mismatch { actual })
    }

    /// The newest committer date of the commits `a` and `b`, as git reads
    /// a date (`<seconds> +0000`). A merge commit of the candidate gets it,
    /// so a candidate tip depends only on its inputs, and a repeated build
    /// gives the same tip.
    fn newest_date(&self, repo: &Path, a: &str, b: &str) -> Result<String> {
        let out = self.git.git(repo, &["show", "-s", "--format=%ct", a, b])?;
        let mut newest: u64 = 0;
        for line in out.lines() {
            let seconds: u64 = line
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("git show printed '{line}', not a date"))?;
            newest = newest.max(seconds);
        }
        Ok(format!("{newest} +0000"))
    }

    /// The paths that differ between the commits `a` and `b`.
    fn changed_paths(&self, repo: &Path, a: &str, b: &str) -> Result<BTreeSet<String>> {
        let out = self.git.git(
            repo,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--name-only",
                "--no-renames",
                "--no-ext-diff",
                a,
                b,
            ],
        )?;
        Ok(nul_fields(&out).map(str::to_string).collect())
    }
}

/// The fields of a `-z` output of git that are not empty.
fn nul_fields(out: &str) -> impl Iterator<Item = &str> {
    out.split('\0').filter(|f| !f.is_empty())
}

/// An error unless `name` is a full ref name. A short name lets git choose
/// the ref, and a name that starts with `-` is an option.
fn full_ref(name: &str) -> Result<()> {
    if !name.starts_with("refs/") || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("'{name}' is not a full ref name");
    }
    Ok(())
}

/// An error unless `id` is a full object id, so git reads no revision
/// syntax and no option.
fn object_id(id: &str) -> Result<()> {
    if !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("'{id}' is not a full object id");
    }
    Ok(())
}

/// The branch that 1 subject of `git stash list` names: `WIP on <branch>:`
/// or `On <branch>:`. A branch name holds no `:`.
fn stash_branch(subject: &str) -> Option<&str> {
    let rest = subject
        .strip_prefix("WIP on ")
        .or_else(|| subject.strip_prefix("On "))?;
    let (branch, _) = rest.split_once(':')?;
    (branch != "(no branch)" && !branch.is_empty()).then_some(branch)
}

impl IntegrationGitGateway for CliGitGateway {
    fn is_ancestor(&self, repo: &Path, a: &str, b: &str) -> Result<bool> {
        let out = self
            .git
            .checked(repo, &["merge-base", "--is-ancestor", a, b])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            s => bail!(
                "git merge-base --is-ancestor {a} {b} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        full_ref(name)?;
        Ok(self
            .refs(repo, name)?
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, tip)| tip))
    }

    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove> {
        full_ref(name)?;
        object_id(new)?;
        if let Some(old) = expected_old {
            object_id(old)?;
        }
        // An empty old value: the ref must not exist.
        let old = expected_old.unwrap_or("");
        let out = self
            .git
            .checked(repo, &["update-ref", "-m", MOVE_MESSAGE, name, new, old])?;
        if out.status == 0 {
            return Ok(RefMove::Moved);
        }
        self.failed_move(repo, name, expected_old, &out.stderr)
    }

    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove> {
        full_ref(name)?;
        object_id(expected_old)?;
        let args = ["update-ref", "-m", MOVE_MESSAGE, "-d", name, expected_old];
        let out = self.git.checked(repo, &args)?;
        if out.status == 0 {
            return Ok(RefMove::Moved);
        }
        self.failed_move(repo, name, Some(expected_old), &out.stderr)
    }

    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
        for id in [tip, ancestor] {
            object_id(id)?;
        }
        self.is_ancestor(repo, ancestor, tip)
    }

    fn changes_in(
        &self,
        repo: &Path,
        branch_tip: &str,
        base: &str,
        target_tip: &str,
    ) -> Result<bool> {
        for id in [branch_tip, base, target_tip] {
            object_id(id)?;
        }
        if self.is_ancestor(repo, branch_tip, target_tip)? {
            return Ok(true);
        }
        let merge_base = self.git.git(repo, &["merge-base", branch_tip, base])?;
        let changed = self.changed_paths(repo, merge_base.trim(), branch_tip)?;
        if changed.is_empty() {
            return Ok(true);
        }
        let differ = self.changed_paths(repo, branch_tip, target_tip)?;
        Ok(changed.is_disjoint(&differ))
    }

    fn build_candidate(
        &self,
        repo: &Path,
        base_tip: &str,
        members: &[Member],
    ) -> Result<Candidate> {
        object_id(base_tip)?;
        let mut head = base_tip.to_string();
        for member in members {
            object_id(&member.tip)?;
            // A member that is already in the candidate, as a branch that the
            // old path landed, adds no merge commit: the candidate stays at
            // its tip, and a bundle of only such members lands at the base
            // tip with no ref move.
            if self.is_ancestor(repo, &member.tip, &head)? {
                continue;
            }
            let args = [
                "merge-tree",
                "--write-tree",
                "-z",
                "--name-only",
                "--no-messages",
                &head,
                &member.tip,
            ];
            let out = self.git.checked(repo, &args)?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut fields = nul_fields(&text);
            match out.status {
                0 => {}
                1 => {
                    let files: BTreeSet<&str> = fields.skip(1).collect();
                    return Ok(Candidate::Conflict {
                        member: member.name.clone(),
                        files: files.into_iter().map(str::to_string).collect(),
                    });
                }
                s => bail!(
                    "git merge-tree of {} failed ({s}): {}",
                    member.name,
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            }
            let Some(tree) = fields.next() else {
                bail!("git merge-tree of {} printed no tree", member.name);
            };
            let message = format!("Merge {} into the candidate", member.name);
            let mut args = IDENTITY.to_vec();
            args.extend(["commit-tree", tree, "-p", &head, "-p", &member.tip]);
            args.extend(["-m", &message]);
            let date = self.newest_date(repo, &head, &member.tip)?;
            let git = self
                .git
                .clone()
                .with_env("GIT_AUTHOR_DATE", &date)
                .with_env("GIT_COMMITTER_DATE", &date);
            head = git.git(repo, &args)?.trim().to_string();
        }
        Ok(Candidate::Built { tip: head })
    }

    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>> {
        Ok(self
            .refs(repo, FLEET_BRANCHES)?
            .into_iter()
            .filter_map(|(name, tip)| {
                let name = name.strip_prefix("refs/heads/")?.to_string();
                Some(BranchTip { name, tip })
            })
            .collect())
    }

    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>> {
        // A glob `*` of `for-each-ref` stops at a `/`, so the filter is here.
        let prefix = format!("refs/heads/{run_branch}-T");
        Ok(self
            .refs(repo, LOCAL_BRANCHES)?
            .into_iter()
            .filter(|(name, _)| name.starts_with(&prefix))
            .filter_map(|(name, tip)| {
                let name = name.strip_prefix("refs/heads/")?.to_string();
                Some(BranchTip { name, tip })
            })
            .collect())
    }

    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>> {
        let out = self.git.git(repo, &["stash", "list", "--format=%gs"])?;
        let names: BTreeSet<&str> = out.lines().filter_map(stash_branch).collect();
        Ok(names.into_iter().map(str::to_string).collect())
    }

    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>> {
        object_id(tip)?;
        // The blob id first, so a commit with no fleet.toml is `None`. Then
        // the plain bytes of the blob: `git show <tip>:fleet.toml` with no
        // text conversion.
        let path = format!("{tip}:fleet.toml");
        let out = self
            .git
            .checked(repo, &["rev-parse", "--verify", "--quiet", &path])?;
        if out.status != 0 {
            return Ok(None);
        }
        let blob = out.stdout_text();
        Ok(Some(self.git.git(repo, &["cat-file", "blob", &blob])?))
    }

    fn ensure_settings(&self, dir: &Path) -> Result<()> {
        for (key, value) in SETTINGS {
            let out = self.git.checked(dir, &["config", "--get", key])?;
            if out.status == 0 && out.stdout_text() == value {
                continue;
            }
            self.git.git(dir, &["config", key, value])?;
        }
        Ok(())
    }
}

/// The id of the empty blob of a repository with the object format
/// `format`. git knows it with no object in the repository.
fn empty_blob(format: &str) -> Result<&'static str> {
    match format {
        "sha1" => Ok("e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        "sha256" => Ok("473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813"),
        other => bail!("git has the object format {other}, which fleetd does not know"),
    }
}

/// [`StackGit`] on [`CliGitGateway`]: the git reads of the stacks.
/// `git merge-file -p` prints the merge and writes no object.
#[derive(Debug, Clone)]
pub struct CliStackGit {
    gateway: CliGitGateway,
}

impl CliStackGit {
    pub fn new(git: CliWorktreeGit) -> CliStackGit {
        CliStackGit {
            gateway: CliGitGateway::new(git),
        }
    }

    /// The object of `file` at `rev`, or `None` when `rev` has no `file`.
    fn blob(&self, repo: &Path, rev: &str, file: &str) -> Result<Option<String>> {
        let spec = format!("{rev}:{file}");
        let out = self
            .gateway
            .git
            .checked(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
        Ok((out.status == 0).then(|| out.stdout_text()))
    }
}

impl StackGit for CliStackGit {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        self.gateway.tip(repo, name)
    }

    fn conflict(
        &self,
        repo: &Path,
        lower_tip: &str,
        upper_tip: &str,
        file: &str,
    ) -> Result<String> {
        object_id(lower_tip)?;
        object_id(upper_tip)?;
        let git = &self.gateway.git;
        let format = git.git(repo, &["rev-parse", "--show-object-format"])?;
        let empty = empty_blob(format.trim())?.to_string();
        let base = git.checked(repo, &["merge-base", lower_tip, upper_tip])?;
        let base = (base.status == 0).then(|| base.stdout_text());
        let blob = |rev: Option<&str>| -> Result<String> {
            let found = match rev {
                Some(rev) => self.blob(repo, rev, file)?,
                None => None,
            };
            Ok(found.unwrap_or_else(|| empty.clone()))
        };
        let ours = blob(Some(upper_tip))?;
        let theirs = blob(Some(lower_tip))?;
        let base_blob = blob(base.as_deref())?;
        let out = git.checked(
            repo,
            &[
                "merge-file",
                "-p",
                "--zdiff3",
                "-L",
                upper_tip,
                "-L",
                "base",
                "-L",
                lower_tip,
                "--object-id",
                &ours,
                &base_blob,
                &theirs,
            ],
        )?;
        // The exit code is the count of conflicts; a negative one is an
        // error.
        if !(0..=127).contains(&out.status) {
            bail!(
                "git merge-file of {file} failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stash_subject_names_its_branch() {
        assert_eq!(stash_branch("WIP on fleet/x: abc1234 msg"), Some("fleet/x"));
        assert_eq!(stash_branch("On main: keep: this"), Some("main"));
        assert_eq!(stash_branch("WIP on (no branch): abc msg"), None);
        assert_eq!(stash_branch("autostash"), None);
    }

    #[test]
    fn a_ref_must_be_full_and_a_tip_must_be_an_id() {
        assert!(full_ref("refs/heads/fleet/x").is_ok());
        assert!(full_ref("fleet/x").is_err());
        assert!(full_ref("--stdin").is_err());
        assert!(full_ref("refs/heads/a b").is_err());
        assert!(object_id(&"a".repeat(40)).is_ok());
        assert!(object_id(&"0".repeat(64)).is_ok());
        assert!(object_id("HEAD").is_err());
        assert!(object_id(&format!("-{}", "a".repeat(39))).is_err());
    }
}
