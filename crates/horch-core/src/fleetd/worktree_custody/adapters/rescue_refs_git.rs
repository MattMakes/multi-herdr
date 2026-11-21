use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::fleetd::integration::adapters::git::CliGitGateway;
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use crate::fleetd::worktree_custody::adapters::rescue_git::rev;
use crate::fleetd::worktree_custody::domain::rescue::{self, REF_PREFIX};
use crate::fleetd::worktree_custody::domain::rescue_class::{
    self as class, Owner, Parsed, RefOwner,
};

pub const DATA_KEY: &str = "rescue_refs";

pub const ERROR_KEY: &str = "rescue_refs_error";

const REF_TRIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefFacts {
    pub name: String,
    pub tip: String,
    pub branch: Option<String>,
    pub feature: Option<String>,
    pub bytes: Option<u64>,
    pub in_newer_tip: Option<bool>,
    pub in_landed: Option<bool>,
}

pub fn facts_of(data: &serde_json::Value) -> Option<Vec<RefFacts>> {
    serde_json::from_value(data.get(DATA_KEY)?.clone()).ok()
}

pub fn write_branch_rescue(
    git: &CliWorktreeGit,
    repo: &Path,
    branch: &str,
    tip: &str,
) -> Result<String> {
    let Some(id) = class::branch_ref_id(branch) else {
        bail!("the branch name {branch:?} cannot name a rescue ref folder");
    };
    let commit = rev(git, repo, &format!("{tip}^{{commit}}"))?;
    if commit != tip {
        bail!("{tip} is not the full id of a commit; git names {commit}");
    }
    let folder = rescue::ref_folder(&id);
    for _ in 0..REF_TRIES {
        let refs = refs_below(git, repo, &folder)?;
        let names: Vec<String> = refs.keys().cloned().collect();
        let n = rescue::next_n(&id, &names);
        if n > 1 {
            let newest = rescue::ref_name(&id, n - 1);
            if refs.get(&newest).map(String::as_str) == Some(tip) {
                return Ok(newest);
            }
        }
        let name = rescue::ref_name(&id, n);
        let message = format!("horch rescue of branch {branch}");
        let out = git.checked(repo, &["update-ref", "-m", &message, &name, tip, ""])?;
        if out.status == 0 {
            let back = rev(git, repo, &name)?;
            if back != tip {
                bail!("the rescue ref {name} reads back {back}, not {tip}");
            }
            return Ok(name);
        }
        if !refs_below(git, repo, &name)?.contains_key(&name) {
            bail!(
                "git update-ref {name} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    bail!("each of {REF_TRIES} new rescue refs of {branch} existed before its write")
}

fn refs_below(
    git: &CliWorktreeGit,
    repo: &Path,
    pattern: &str,
) -> Result<BTreeMap<String, String>> {
    let out = git.git(
        repo,
        &["for-each-ref", "--format=%(objectname) %(refname)", pattern],
    )?;
    Ok(out
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(tip, name)| (name.to_string(), tip.to_string()))
        .collect())
}

pub fn read_facts(
    git: &CliWorktreeGit,
    repo: &Path,
    owner_of: &dyn Fn(&RefOwner) -> Option<Owner>,
) -> Result<Vec<RefFacts>> {
    let gw = CliGitGateway::new(git.clone());
    let refs = refs_below(git, repo, REF_PREFIX)?;
    let parsed: Vec<(&String, &String, Option<Parsed>)> = refs
        .iter()
        .map(|(name, tip)| (name, tip, class::parse(name)))
        .collect();
    let mut newest: BTreeMap<&RefOwner, (u64, &String)> = BTreeMap::new();
    for (_, tip, p) in &parsed {
        if let Some(p) = p {
            let e = newest.entry(&p.owner).or_insert((p.n, tip));
            if p.n > e.0 {
                *e = (p.n, tip);
            }
        }
    }
    let mut out = Vec::new();
    for (name, tip, p) in &parsed {
        let owner = p.as_ref().and_then(|p| owner_of(&p.owner));
        let mut newer = Vec::new();
        if let Some((n, t)) = p.as_ref().and_then(|p| newest.get(&p.owner)) {
            if Some(*n) != p.as_ref().map(|p| p.n) {
                newer.push(Some((*t).clone()));
            }
        }
        let mut landed = Vec::new();
        if let Some(o) = &owner {
            newer.push(branch_tip(&gw, repo, &o.branch));
            if let Some(f) = &o.feature {
                if let Some(b) = &f.branch {
                    landed.push(branch_tip(&gw, repo, b));
                }
                landed.push(branch_tip(&gw, repo, &f.primary));
            }
        }
        newer.retain(|t| t.as_deref() != Some(tip.as_str()));
        out.push(RefFacts {
            name: (*name).clone(),
            tip: (*tip).clone(),
            branch: owner.as_ref().map(|o| o.branch.clone()),
            feature: owner
                .as_ref()
                .and_then(|o| Some(o.feature.as_ref()?.id.clone())),
            bytes: only_bytes(git, repo, tip).ok(),
            in_newer_tip: any_holds(&gw, repo, tip, &newer),
            in_landed: any_holds(&gw, repo, tip, &landed),
        });
    }
    Ok(out)
}

fn branch_tip(gw: &CliGitGateway, repo: &Path, name: &str) -> Option<String> {
    gw.tip(repo, &format!("refs/heads/{name}")).ok().flatten()
}

fn any_holds(
    gw: &CliGitGateway,
    repo: &Path,
    tip: &str,
    targets: &[Option<String>],
) -> Option<bool> {
    let mut known = true;
    for target in targets.iter().flatten() {
        match gw.changes_in(repo, tip, target, target) {
            Ok(true) => return Some(true),
            Ok(false) => {}
            Err(_) => known = false,
        }
    }
    known.then_some(false)
}

fn only_bytes(git: &CliWorktreeGit, repo: &Path, tip: &str) -> Result<u64> {
    let out = git.git(
        repo,
        &[
            "rev-list",
            "--objects",
            "--disk-usage",
            tip,
            "--not",
            "--branches",
        ],
    )?;
    Ok(out.trim().parse()?)
}
