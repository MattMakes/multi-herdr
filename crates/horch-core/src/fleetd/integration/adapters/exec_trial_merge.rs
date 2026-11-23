use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::integration::adapters::git::CliGitGateway;
use crate::fleetd::integration::application::ports::{Candidate, IntegrationGitGateway, Member};
use crate::fleetd::integration::application::trial_merges::{TrialMergeData, TrialMergeGit};
use crate::fleetd::integration::domain::trial_merge;
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;
use crate::fleetd::worktree_custody::adapters::git::{no_sandbox, repo_git_dir, CliWorktreeGit};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    let Some((left, right)) = trial_merge::tips(&row.input_key) else {
        return Ok(failed(format!(
            "the input key {:?} is not 2 tips",
            row.input_key
        )));
    };
    let Some(repo) = row.repo.as_deref().map(Path::new) else {
        return Ok(failed("the job has no repository".to_string()));
    };
    let git_dir = match repo_git_dir(repo) {
        Ok(dir) => dir,
        Err(e) => return Ok(failed(format!("{e:#}"))),
    };
    let git = CliWorktreeGit::new(
        git_bin(&BinOverrides::from_env(&ProcessEnv)),
        fleet_root(conn)?,
    )
    .writing(&[git_dir]);
    if let Some(ran) = no_sandbox(&git) {
        return Ok(ran);
    }
    let member = Member {
        name: row.subject.clone(),
        tip: right.to_string(),
    };
    Ok(
        match CliGitGateway::new(git).build_candidate(repo, left, &[member]) {
            Ok(Candidate::Built { .. }) => Ran {
                outcome: Outcome {
                    passed: true,
                    verdict: true,
                    text: format!("{}: clean", row.subject),
                },
                data: Some(data(Vec::new())),
            },
            Ok(Candidate::Conflict { files, .. }) => Ran {
                outcome: Outcome {
                    passed: false,
                    verdict: true,
                    text: format!("{}: conflict in {}", row.subject, files.join(", ")),
                },
                data: Some(data(files)),
            },
            Err(e) => failed(format!("{e:#}")),
        },
    )
}

fn data(conflict_files: Vec<String>) -> serde_json::Value {
    serde_json::to_value(TrialMergeData { conflict_files }).expect("a result serializes")
}

fn failed(text: String) -> Ran {
    Outcome {
        passed: false,
        verdict: false,
        text,
    }
    .into()
}

#[derive(Debug, Clone)]
pub struct CliTrialMergeGit {
    git: CliWorktreeGit,
}

impl CliTrialMergeGit {
    pub fn new(git: CliWorktreeGit) -> CliTrialMergeGit {
        CliTrialMergeGit { git }
    }
}

impl TrialMergeGit for CliTrialMergeGit {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        CliGitGateway::new(self.git.clone()).tip(repo, name)
    }

    fn changed_files(&self, repo: &Path, base: &str, tip: &str) -> Result<BTreeSet<String>> {
        let range = format!("{base}...{tip}");
        let out = self
            .git
            .git(repo, &["diff", "--name-only", "-z", "--no-renames", &range])
            .with_context(|| format!("reading the files that {tip} changed from {base}"))?;
        Ok(out
            .split('\0')
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .collect())
    }
}
