//! The executor of the `rebase` job (ticket 41).
//!
//! `docs/specs/fleetd/design.md`, "Between features" (Continuous rebase),
//! "The kinds of job" (`rebase`), I9 and I10; `docs/tickets/fleetd-p4/MAP.md`,
//! Q18 and Q26.
//!
//! The subject of a run is the branch, and its input key is the tip of the
//! branch and the tip that it moves onto (`application::rebase::rebase_key`).
//! The run writes no ledger row: a later pass reads its result.
//!
//! 1. The fleetd git rule (Q26): each git call goes through
//!    [`CliWorktreeGit`], so it runs in the platform sandbox with hooks,
//!    fsmonitor, signing, editors and prompts off. First the run reads the
//!    config of the repository and of the gate tree: a key that starts a
//!    program ends the run `failed`, with the key in its reason, and the run
//!    changes nothing. `git rebase` would start the `pre-rebase`,
//!    `post-checkout` and `post-rewrite` hooks, a checkout filter, a merge
//!    driver and the gpg program.
//! 2. When the branch is not at the tip of the key, the run moves nothing.
//!    A branch that contains the tip to move onto is a passed run: a run
//!    before it moved the branch and then lost its result.
//! 3. The rescue ref of the tip exists before the rebase (I10).
//! 4. In the gate tree of the feature (Q18), never a tree of a worker:
//!    `git checkout --detach <tip>` and `git rebase <onto>`. The branch is
//!    not checked out, so a conflict leaves it at its tip; the run aborts the
//!    rebase and gives the conflict files.
//! 5. `git update-ref refs/heads/<branch> <new> <tip>`: a compare-and-set
//!    on the tip of the key (I9).
//!
//! A gate tree that is busy or not made, a host with no sandbox, and a git
//! error end the run with an error, so the run ends `abandoned` and runs
//! again after its backoff.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::rebase::{
    gate_tree_busy, read_key, RebaseRefs, Rebased,
};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;
use crate::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit, NO_SANDBOX};
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::write_branch_rescue;
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use crate::fleetd::worktree_custody::application::gate_tree;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

/// The `-c` keys of the rebase, after the keys of the fleetd git rule. The
/// global config is not read, so git has no identity of the user: the
/// committer of each rebased commit is fleetd, and the author stays. The
/// rebase moves no other branch and stashes nothing.
const REBASE_CONFIG: [&str; 5] = [
    "user.name=horch fleetd",
    "user.email=fleetd@horch.invalid",
    "rebase.updateRefs=false",
    "rebase.autoStash=false",
    "rebase.autoSquash=false",
];

/// The reflog message of the compare-and-set of the branch.
const MOVE_MESSAGE: &str = "fleetd: rebase";

/// [`RebaseRefs`] through the git of fleetd: no hook, in the sandbox.
pub struct GitRebaseRefs {
    git: CliWorktreeGit,
}

impl GitRebaseRefs {
    /// The refs of `git`, an adapter that can write the git folder of the
    /// repository.
    pub fn new(git: CliWorktreeGit) -> GitRebaseRefs {
        GitRebaseRefs { git }
    }
}

impl RebaseRefs for GitRebaseRefs {
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
        contains(&self.git, repo, tip, ancestor)
    }

    fn rescue(&self, repo: &Path, branch: &str, tip: &str) -> Result<String> {
        write_branch_rescue(&self.git, repo, branch, tip)
    }
}

/// Whether `ancestor` is an ancestor of `tip`, or is `tip`.
fn contains(git: &CliWorktreeGit, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
    let out = git.checked(repo, &["merge-base", "--is-ancestor", ancestor, tip])?;
    match out.status {
        0 => Ok(true),
        1 => Ok(false),
        s => bail!(
            "git merge-base --is-ancestor {ancestor} {tip} failed ({s}): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}

/// An error unless `id` is a full object id, so git reads no revision
/// syntax and no option.
fn object_id(id: &str) -> Result<()> {
    if !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("'{id}' is not a full object id");
    }
    Ok(())
}

fn ended(passed: bool, text: String, data: Option<Rebased>) -> Ran {
    Ran {
        outcome: Outcome {
            passed,
            verdict: true,
            text,
        },
        data: data.map(|d| serde_json::to_value(d).expect("a result serializes")),
    }
}

/// What 1 run rebases: the branch, its tip, the tip to move onto, the
/// repository and the gate tree.
struct Job {
    branch: String,
    tip: String,
    onto: String,
    repo: PathBuf,
    tree: PathBuf,
}

/// Runs the job `row` with the git of this process.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    let job = match read_job(conn, row)? {
        Ok(job) => job,
        Err(text) => return Ok(ended(false, text, None)),
    };
    let git = CliWorktreeGit::new(
        git_bin(&BinOverrides::from_env(&ProcessEnv)),
        fleet_root(conn)?,
    )
    .reading(std::slice::from_ref(&job.repo))
    .writing(&[repo_git_dir(&job.repo)?, job.tree.clone()]);
    if let Some(gap) = git.sandbox_gap() {
        bail!("{NO_SANDBOX}: {gap}; fleetd runs git in a project only in the sandbox");
    }
    for dir in [&job.repo, &job.tree] {
        if let Err(e) = git.refuse_project_programs(dir) {
            return Ok(ended(false, format!("{e:#}"), None));
        }
    }
    rebase(&git, &job)
}

/// The job of the run `row`. The inner `Err` is a result: the row names no
/// rebase. An error is a state that a later run can find changed.
fn read_job(conn: &Connection, row: &JobRow) -> Result<Result<Job, String>> {
    let Some((tip, onto)) = read_key(&row.input_key) else {
        return Ok(Err(format!("'{}' is not a rebase key", row.input_key)));
    };
    if let Err(e) = object_id(tip).and_then(|()| object_id(onto)) {
        return Ok(Err(format!("{e:#}")));
    }
    let branch = row.subject.clone();
    let Some(branch_row) = rows::branch(conn, &branch)? else {
        return Ok(Err(format!("no branch {branch} in the ledger")));
    };
    let Some(repo) = row.repo.as_deref().map(PathBuf::from) else {
        return Ok(Err(format!(
            "the rebase job {} names no repository",
            row.id
        )));
    };
    let feature = branch_row.feature_id;
    if gate_tree_busy(conn, &feature)? {
        bail!("the gate tree of {feature} is busy: a bundle is gating or passed, or its full gate runs");
    }
    let tree = match gate_tree::of_feature(conn, &repo, &feature)? {
        Some(t) if t.state == WorktreeState::Created && Path::new(&t.path).is_dir() => {
            PathBuf::from(t.path)
        }
        _ => bail!("the gate tree of {feature} is not made yet"),
    };
    Ok(Ok(Job {
        branch,
        tip: tip.to_string(),
        onto: onto.to_string(),
        repo,
        tree,
    }))
}

/// Steps 2 to 5 of the module documentation.
fn rebase(git: &CliWorktreeGit, job: &Job) -> Result<Ran> {
    let full = format!("refs/heads/{}", job.branch);
    let Some(now) = branch_tip(git, &job.repo, &full)? else {
        return Ok(ended(
            false,
            format!("git has no branch {}", job.branch),
            None,
        ));
    };
    if now != job.tip {
        if contains(git, &job.repo, &now, &job.onto)? {
            let text = format!("{} is at {now}, which contains {}", job.branch, job.onto);
            return Ok(ended(true, text, Some(Rebased::Moved { tip: now })));
        }
        let text = format!(
            "{} is at {now}, not at {}; the tips moved, so the job changes nothing",
            job.branch, job.tip
        );
        return Ok(ended(false, text, None));
    }
    let rescue = write_branch_rescue(git, &job.repo, &job.branch, &job.tip)?;
    if contains(git, &job.repo, &job.tip, &job.onto)? {
        let text = format!("{} at {} contains {}", job.branch, job.tip, job.onto);
        let tip = job.tip.clone();
        return Ok(ended(true, text, Some(Rebased::Moved { tip })));
    }
    // A run that was stopped can leave its rebase in the tree.
    git.checked(&job.tree, &["rebase", "--quit"])?;
    git.git(&job.tree, &["checkout", "--force", "--detach", &job.tip])?;
    let mut args: Vec<&str> = REBASE_CONFIG.iter().flat_map(|c| ["-c", c]).collect();
    args.extend(["rebase", "--no-verify", "--no-rerere-autoupdate", &job.onto]);
    let out = git.checked(&job.tree, &args)?;
    if out.status != 0 {
        return conflict(git, job, &String::from_utf8_lossy(&out.stderr));
    }
    let new = git.git(&job.tree, &["rev-parse", "--verify", "HEAD"])?;
    let new = new.trim();
    if !contains(git, &job.repo, new, &job.onto)? {
        bail!(
            "the gate tree is at {new} after the rebase, which does not contain {}",
            job.onto
        );
    }
    let out = git.checked(
        &job.repo,
        &["update-ref", "-m", MOVE_MESSAGE, &full, new, &job.tip],
    )?;
    if out.status != 0 {
        let actual = branch_tip(git, &job.repo, &full)?;
        if actual.as_deref() == Some(job.tip.as_str()) {
            bail!(
                "git update-ref {full} failed with the ref at its tip: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let text = format!(
            "{} moved to {} during the rebase; the compare-and-set moved nothing",
            job.branch,
            actual.as_deref().unwrap_or("nothing")
        );
        return Ok(ended(false, text, None));
    }
    let text = format!(
        "rebased {} from {} onto {} at {new}; the old tip is in {rescue}",
        job.branch, job.tip, job.onto
    );
    let tip = new.to_string();
    Ok(ended(true, text, Some(Rebased::Moved { tip })))
}

/// The end of a rebase that stopped: the conflict files, then the abort of
/// the rebase. A stop with no conflict file is an error.
fn conflict(git: &CliWorktreeGit, job: &Job, stderr: &str) -> Result<Ran> {
    let out = git.git(
        &job.tree,
        &[
            "diff",
            "--name-only",
            "--diff-filter=U",
            "-z",
            "--no-ext-diff",
        ],
    )?;
    let files: Vec<String> = out
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect();
    git.git(&job.tree, &["rebase", "--abort"])
        .context("the abort of the rebase")?;
    if files.is_empty() {
        return Err(anyhow!("git rebase failed: {}", stderr.trim()));
    }
    let text = format!(
        "the rebase of {} onto {} conflicts in {}; the branch stays at {}",
        job.branch,
        job.onto,
        files.join(", "),
        job.tip
    );
    Ok(ended(false, text, Some(Rebased::Conflict { files })))
}

/// The commit that the full ref `full` names, or `None` when it does not
/// exist.
fn branch_tip(git: &CliWorktreeGit, repo: &Path, full: &str) -> Result<Option<String>> {
    let spec = format!("{full}^{{commit}}");
    let out = git.checked(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
    Ok((out.status == 0).then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rebase_keys_stop_other_moves() {
        for key in ["rebase.updateRefs=false", "rebase.autoStash=false"] {
            assert!(REBASE_CONFIG.contains(&key), "{key}");
        }
    }

    #[test]
    fn object_id_refuses_an_option_and_a_short_name() {
        assert!(object_id(&"a".repeat(40)).is_ok());
        assert!(object_id("HEAD").is_err());
        assert!(object_id(&format!("-{}", "a".repeat(39))).is_err());
    }
}
