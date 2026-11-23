use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::integration::adapters::sqlite as rows;
use crate::fleetd::integration::application::rebase::{
    gate_tree_busy, is_on, read_key, RebaseRefs, Rebased,
};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;
use crate::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit, NO_SANDBOX};
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::write_branch_rescue;
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use crate::fleetd::worktree_custody::application::gate_tree;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

const REBASE_CONFIG: [&str; 5] = [
    "user.name=horch fleetd",
    "user.email=fleetd@horch.invalid",
    "rebase.updateRefs=false",
    "rebase.autoStash=false",
    "rebase.autoSquash=false",
];

const MOVE_MESSAGE: &str = "fleetd: rebase";

pub struct GitRebaseRefs {
    git: CliWorktreeGit,
}

impl GitRebaseRefs {
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

    fn merge_base(&self, repo: &Path, a: &str, b: &str) -> Result<Option<String>> {
        let out = self.git.checked(repo, &["merge-base", a, b])?;
        match out.status {
            0 => Ok(Some(
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
            )),
            1 => Ok(None),
            s => bail!(
                "git merge-base {a} {b} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }
}

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

struct Job {
    branch: String,
    tip: String,
    onto: String,
    cut: Option<String>,
    repo: PathBuf,
    tree: PathBuf,
}

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

fn read_job(conn: &Connection, row: &JobRow) -> Result<Result<Job, String>> {
    let Some(key) = read_key(&row.input_key) else {
        return Ok(Err(format!("'{}' is not a rebase key", row.input_key)));
    };
    let ids = [Some(key.tip), Some(key.onto), key.cut];
    if let Err(e) = ids.into_iter().flatten().try_for_each(object_id) {
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
        tip: key.tip.to_string(),
        onto: key.onto.to_string(),
        cut: key.cut.map(str::to_string),
        repo,
        tree,
    }))
}

fn rebase(git: &CliWorktreeGit, job: &Job) -> Result<Ran> {
    let refs = GitRebaseRefs::new(git.clone());
    let on = |tip: &str| is_on(&refs, &job.repo, tip, &job.onto, job.cut.as_deref());
    let full = format!("refs/heads/{}", job.branch);
    let Some(now) = branch_tip(git, &job.repo, &full)? else {
        return Ok(ended(
            false,
            format!("git has no branch {}", job.branch),
            None,
        ));
    };
    if now != job.tip {
        if on(&now)? {
            let text = format!("{} is at {now}, which is on {}", job.branch, job.onto);
            return Ok(ended(true, text, Some(Rebased::Moved { tip: now })));
        }
        let text = format!(
            "{} is at {now}, not at {}; the tips moved, so the job changes nothing",
            job.branch, job.tip
        );
        return Ok(ended(false, text, None));
    }
    let rescue = write_branch_rescue(git, &job.repo, &job.branch, &job.tip)?;
    if on(&job.tip)? {
        let text = format!("{} at {} is on {}", job.branch, job.tip, job.onto);
        let tip = job.tip.clone();
        return Ok(ended(true, text, Some(Rebased::Moved { tip })));
    }
    git.checked(&job.tree, &["rebase", "--quit"])?;
    git.git(&job.tree, &["checkout", "--force", "--detach", &job.tip])?;
    let mut args: Vec<&str> = REBASE_CONFIG.iter().flat_map(|c| ["-c", c]).collect();
    args.extend(["rebase", "--no-verify", "--no-rerere-autoupdate"]);
    match job.cut.as_deref() {
        Some(cut) => args.extend(["--onto", &job.onto, cut]),
        None => args.push(&job.onto),
    }
    let out = git.checked(&job.tree, &args)?;
    if out.status != 0 {
        return conflict(git, job, &String::from_utf8_lossy(&out.stderr));
    }
    let new = git.git(&job.tree, &["rev-parse", "--verify", "HEAD"])?;
    let new = new.trim();
    if !on(new)? {
        bail!(
            "the gate tree is at {new} after the rebase, which is not on {}",
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
    let cut = job
        .cut
        .as_deref()
        .map(|c| format!(" without the commits of {c}"))
        .unwrap_or_default();
    let text = format!(
        "rebased {} from {} onto {}{cut} at {new}; the old tip is in {rescue}",
        job.branch, job.tip, job.onto
    );
    let tip = new.to_string();
    Ok(ended(true, text, Some(Rebased::Moved { tip })))
}

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
