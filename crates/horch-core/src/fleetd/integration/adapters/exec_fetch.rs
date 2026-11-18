//! The executor of the `fetch` job (ticket 37).
//!
//! The subject of a run is the repository, and its input key names the
//! remote (`application::remote::fetch_key`). The run does its network work
//! outside each transaction, then records it in 1 short transaction:
//!
//! 1. `git fetch` of each branch of the remote through
//!    [`crate::vcs::network::output`] (F11): its bound is 3 min,
//!    `GIT_TERMINAL_PROMPT=0`, no hooks, and it never retries. A failed
//!    fetch sets `stale_since`, and the run ends `failed`.
//! 2. The fetched tip of the primary branch and of the branch of each
//!    feature of the repository.
//! 3. With `pr = gh`, `gh pr view --json state` of each feature with a
//!    pull request, through the launcher. A missing `gh` or a failed read
//!    is named in the result of the run, and the tips are still recorded.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Result};
use horch_marketplace::git::GitRunner;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::integration::adapters::project_file;
use crate::fleetd::integration::application::remote::{self, remote_of_key, Fetched, PrRead};
use crate::fleetd::integration::domain::project::{Pr, ProjectFileReader, FILE_NAME};
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::process;
use crate::taskledger::runs;
use crate::vcs::network;

/// The programs of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub git: PathBuf,
    /// `gh` on PATH, or `None` when PATH has none.
    pub gh: Option<PathBuf>,
    /// The folder of the output file of each `gh` call.
    pub temp: PathBuf,
}

impl Tools {
    /// The programs that `env` names.
    pub fn from_env(env: &dyn EnvSource) -> Tools {
        let path = env.var_os("PATH");
        let pathext = env.var("PATHEXT");
        Tools {
            git: git_bin(&BinOverrides::from_env(env)),
            gh: process::which(path.as_deref(), pathext.as_deref(), "gh"),
            temp: env.temp_dir(),
        }
    }
}

/// Runs the job `row` with the programs of this process.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Outcome> {
    run_with(conn, row, &Tools::from_env(&ProcessEnv), &Launcher)
}

/// [`run`] with the programs `tools` and the launcher `host`.
pub fn run_with(
    conn: &Connection,
    row: &JobRow,
    tools: &Tools,
    host: &dyn ProcessHost,
) -> Result<Outcome> {
    let repo = Path::new(&row.subject);
    let remote = remote_of_key(&row.input_key);
    if let Some(why) = fetch(tools, repo, remote) {
        in_transaction(conn, |tx| remote::record_failed(tx, None, &row.subject))?;
        return Ok(Outcome {
            passed: false,
            verdict: true,
            text: format!("git fetch {remote} failed: {why}"),
        });
    }
    let features = features_of(conn, &row.subject)?;
    let tips = fetched_tips(tools, repo, remote, &features)?;
    let (prs, notes) = read_prs(tools, host, repo, row.id, &features);
    let mut text = format!("fetched {} tips from {remote}", tips.len());
    for note in &notes {
        text.push_str("; ");
        text.push_str(note);
    }
    let fetched = Fetched { tips, prs };
    in_transaction(conn, |tx| {
        remote::record_passed(tx, None, &NeedsYouList, &row.subject, &fetched)
    })?;
    Ok(Outcome {
        passed: true,
        verdict: true,
        text,
    })
}

/// Fetch each branch of `remote` into `refs/remotes/<remote>/`. The cause
/// of a failure, or `None`.
fn fetch(tools: &Tools, repo: &Path, remote: &str) -> Option<String> {
    let refspec = format!("+refs/heads/*:refs/remotes/{remote}/*");
    let args = [
        "fetch",
        "--prune",
        "--no-tags",
        "--no-recurse-submodules",
        remote,
        &refspec,
    ];
    match network::output(&tools.git, repo, &args) {
        Err(e) => Some(format!("{e:#}")),
        Ok(out) if out.status != 0 => Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Ok(_) => None,
    }
}

/// 1 `open` or `ready` feature of the repository.
struct Feature {
    id: String,
    branch: Option<String>,
    primary: String,
    pr: Option<String>,
}

/// The `open` and `ready` features of the repository `repo`. The branch
/// of the feature of a run is the run branch (MAP.md of fleetd-p4, Q5), and
/// the root of the project of the run is the repository.
fn features_of(conn: &Connection, repo: &str) -> Result<Vec<Feature>> {
    let runs = runs::of_project(conn, Path::new(repo))?;
    let branches: BTreeSet<&str> = runs
        .iter()
        .filter_map(|r| r.run_branch.as_deref())
        .collect();
    let mut stmt = conn.prepare(
        "SELECT id, branch, primary_branch, pr FROM features
         WHERE branch IS NOT NULL AND state IN ('open', 'ready') ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Feature {
            id: r.get(0)?,
            branch: r.get(1)?,
            primary: r.get(2)?,
            pr: r.get(3)?,
        })
    })?;
    let all: Vec<Feature> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(all
        .into_iter()
        .filter(|f| f.branch.as_deref().is_some_and(|b| branches.contains(b)))
        .collect())
}

/// The fetched tip of the primary branch and of the branch of each feature.
/// A branch that the remote does not have gets no tip.
fn fetched_tips(
    tools: &Tools,
    repo: &Path,
    remote: &str,
    features: &[Feature],
) -> Result<Vec<(String, String)>> {
    let wanted: BTreeSet<&str> = features
        .iter()
        .flat_map(|f| [Some(f.primary.as_str()), f.branch.as_deref()])
        .flatten()
        .collect();
    let prefix = format!("refs/remotes/{remote}/");
    let out = local_git(
        tools,
        repo,
        &["for-each-ref", "--format=%(refname) %(objectname)", &prefix],
    )?;
    Ok(out
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter_map(|(name, tip)| Some((name.strip_prefix(&prefix)?, tip)))
        .filter(|(branch, _)| wanted.contains(branch))
        .map(|(branch, tip)| (branch.to_string(), tip.to_string()))
        .collect())
}

/// The pull request state of each feature with a pull request and
/// `pr = gh`, and a note for each read that failed.
fn read_prs(
    tools: &Tools,
    host: &dyn ProcessHost,
    repo: &Path,
    job: i64,
    features: &[Feature],
) -> (Vec<(String, PrRead)>, Vec<String>) {
    let mut prs = Vec::new();
    let mut notes = Vec::new();
    for f in features {
        let Some(pr) = f.pr.as_deref() else {
            continue;
        };
        let read = uses_gh(tools, repo, f).and_then(|gh| match (gh, &tools.gh) {
            (false, _) => Ok(None),
            (true, None) => Err(anyhow!("gh is not on PATH")),
            (true, Some(bin)) => {
                pr_state(tools, host, bin, repo, pr, &format!("{job}-{}", f.id)).map(Some)
            }
        });
        match read {
            Ok(Some(state)) => prs.push((f.id.clone(), state)),
            Ok(None) => {}
            Err(e) => notes.push(format!("the pull request of {} was not read: {e:#}", f.id)),
        }
    }
    (prs, notes)
}

/// Whether the project sets `pr = gh`, in the project file at the tip of
/// the local feature branch, else of the local primary branch.
fn uses_gh(tools: &Tools, repo: &Path, f: &Feature) -> Result<bool> {
    let branch = f.branch.as_deref().unwrap_or(&f.primary);
    let reader = BlobReader { tools, repo };
    let settings = project_file::load(&reader, &format!("refs/heads/{branch}"))?;
    Ok(settings.pr == Pr::Gh)
}

/// The project file at a commit, read with `git cat-file`, which starts no
/// program of the project.
struct BlobReader<'a> {
    tools: &'a Tools,
    repo: &'a Path,
}

impl ProjectFileReader for BlobReader<'_> {
    fn read(&self, tip: &str) -> std::io::Result<Option<String>> {
        let path = format!("{tip}:{FILE_NAME}");
        let git = GitRunner::new(&self.tools.git);
        let found = git
            .output(self.repo, &["rev-parse", "--verify", "--quiet", &path])
            .map_err(std::io::Error::other)?;
        if found.status != 0 {
            return Ok(None);
        }
        local_git(
            self.tools,
            self.repo,
            &["cat-file", "blob", &found.stdout_text()],
        )
        .map(Some)
        .map_err(std::io::Error::other)
    }
}

/// `gh pr view <pr> --json state` through the launcher, bound by the
/// bound of a git network call (F11).
fn pr_state(
    tools: &Tools,
    host: &dyn ProcessHost,
    gh: &Path,
    repo: &Path,
    pr: &str,
    tag: &str,
) -> Result<PrRead> {
    let output = tools.temp.join(format!("horch-fetch-gh-{tag}.out"));
    let _ = std::fs::remove_file(&output);
    let spec = LaunchSpec {
        program: gh.to_path_buf(),
        args: ["pr", "view", pr, "--json", "state"]
            .iter()
            .map(Into::into)
            .collect(),
        cwd: Some(repo.to_path_buf()),
        env_rule: Default::default(),
        detach: false,
        output: Some(output.clone()),
    };
    let mut child = host.launch(&spec).map_err(|e| anyhow!("{e}"))?.child;
    let until = Instant::now() + network::OTHER_BOUND;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= until {
            horch_platform::group::kill_child_group(&mut child);
            let _ = child.wait();
            let _ = std::fs::remove_file(&output);
            return Err(anyhow!("gh ran past its bound and horch killed it"));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let text = std::fs::read_to_string(&output).unwrap_or_default();
    let _ = std::fs::remove_file(&output);
    if !status.success() {
        return Err(anyhow!("gh pr view failed ({status}): {}", text.trim()));
    }
    // The file has stdout and stderr: take the line with the JSON.
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| v.get("state")?.as_str().and_then(PrRead::parse))
        .ok_or_else(|| anyhow!("gh gave no pull request state: {}", text.trim()))
}

/// Run a local git command that reads, and give its output. A non-zero
/// exit is an error.
fn local_git(tools: &Tools, repo: &Path, args: &[&str]) -> Result<String> {
    let out = GitRunner::new(&tools.git)
        .output(repo, args)
        .map_err(|e| anyhow!("git {}: {e}", args.join(" ")))?;
    if out.status != 0 {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run `f` in 1 immediate transaction of `conn`.
fn in_transaction(conn: &Connection, f: impl FnOnce(&Transaction<'_>) -> Result<()>) -> Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    f(&tx)?;
    tx.commit()?;
    Ok(())
}
