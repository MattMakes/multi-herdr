use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::integration::adapters::git::CliGitGateway;
use crate::fleetd::integration::adapters::sqlite;
use crate::fleetd::integration::application::ports::{
    Candidate, IntegrationGitGateway, Member, RefMove,
};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;
use crate::fleetd::worktree_custody::adapters::git::{self as custody_git, CliWorktreeGit};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

const BUILDING: &str = "building";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Build {
    Candidate {
        candidate_ref: String,
        candidate_tip: String,
    },
    Conflict {
        member: String,
        files: Vec<String>,
    },
}

pub fn candidate_ref(bundle: i64) -> String {
    format!("refs/horch/bundle/{bundle}")
}

pub fn input_key(base_tip: &str, member_tips: &[&str]) -> String {
    std::iter::once(base_tip)
        .chain(member_tips.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_key(key: &str) -> Option<(&str, Vec<&str>)> {
    let mut tips = key.split(' ');
    let base = tips.next().filter(|t| !t.is_empty())?;
    Some((base, tips.collect()))
}

pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    let Ok(id) = row.subject.parse::<i64>() else {
        return Ok(failed(format!("'{}' is not a bundle id", row.subject)));
    };
    let Some(bundle) = sqlite::bundle(conn, id)? else {
        return Ok(failed(format!("no bundle {id} in the ledger")));
    };
    if bundle.state != BUILDING {
        return Ok(failed(format!(
            "bundle {id} is {}, not {BUILDING}; the job wrote no ref",
            bundle.state
        )));
    }
    let members = match members(&row.input_key, &bundle.base_tip, &bundle.members) {
        Ok(members) => members,
        Err(text) => return Ok(failed(format!("bundle {id}: {text}"))),
    };
    let Some(repo) = row.repo.as_deref().map(Path::new) else {
        return Ok(failed(format!(
            "the job of bundle {id} names no repository"
        )));
    };
    let git = CliWorktreeGit::new(
        git_bin(&BinOverrides::from_env(&ProcessEnv)),
        fleet_root(conn)?,
    );
    if let Some(ran) = custody_git::no_sandbox(&git) {
        return Ok(ran);
    }
    let git_dir = match custody_git::repo_git_dir(repo) {
        Ok(dir) => dir,
        Err(e) => return Ok(failed(format!("{e:#}"))),
    };
    let gw = CliGitGateway::new(git.reading(&[PathBuf::from(repo)]).writing(&[git_dir]));
    Ok(build(&gw, repo, id, &bundle.base_tip, &members)
        .unwrap_or_else(|e| failed(format!("bundle {id}: {e:#}"))))
}

fn members(key: &str, base: &str, names: &[String]) -> Result<Vec<Member>, String> {
    let (key_base, tips) =
        read_key(key).ok_or_else(|| format!("the input key '{key}' has no base tip"))?;
    if key_base != base || tips.len() != names.len() {
        return Err(format!(
            "the input key '{key}' is not the base tip {base} and {} member tips",
            names.len()
        ));
    }
    Ok(names
        .iter()
        .zip(tips)
        .map(|(name, tip)| Member {
            name: name.clone(),
            tip: tip.to_string(),
        })
        .collect())
}

fn build(
    gw: &dyn IntegrationGitGateway,
    repo: &Path,
    id: i64,
    base: &str,
    members: &[Member],
) -> Result<Ran> {
    let tip = match gw.build_candidate(repo, base, members)? {
        Candidate::Built { tip } => tip,
        Candidate::Conflict { member, files } => {
            let text = format!(
                "bundle {id}: the merge of {member} conflicts on {}",
                files.join(", ")
            );
            return Ok(ran(false, true, text, Build::Conflict { member, files }));
        }
    };
    let name = candidate_ref(id);
    match gw.update_ref(repo, &name, &tip, None)? {
        RefMove::Moved => {}
        RefMove::Mismatch { actual: Some(t) } if t == tip => {}
        RefMove::Mismatch { actual } => {
            return Ok(failed(format!(
                "bundle {id}: {name} exists at {}, not at the candidate {tip}",
                actual.as_deref().unwrap_or("no tip")
            )))
        }
    }
    let read = gw.tip(repo, &name)?;
    if read.as_deref() != Some(tip.as_str()) {
        return Ok(failed(format!(
            "bundle {id}: {name} reads back as {}, not as the candidate {tip}",
            read.as_deref().unwrap_or("no ref")
        )));
    }
    let text = format!("bundle {id}: {name} is at the candidate {tip}");
    let data = Build::Candidate {
        candidate_ref: name,
        candidate_tip: tip,
    };
    Ok(ran(true, true, text, data))
}

fn ran(passed: bool, verdict: bool, text: String, data: Build) -> Ran {
    Ran {
        outcome: Outcome {
            passed,
            verdict,
            text,
        },
        data: Some(serde_json::to_value(data).expect("a build serializes")),
    }
}

fn failed(text: String) -> Ran {
    Outcome {
        passed: false,
        verdict: false,
        text,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_key_is_the_base_then_each_member_tip() {
        assert_eq!(input_key("b", &["m1", "m2"]), "b m1 m2");
        assert_eq!(read_key("b m1 m2"), Some(("b", vec!["m1", "m2"])));
        assert_eq!(read_key(""), None);
    }

    #[test]
    fn members_pair_each_name_with_its_tip_in_queue_order() {
        let names = ["fleet/a".to_string(), "fleet/b".to_string()];
        let got = members("b t1 t2", "b", &names).unwrap();
        assert_eq!(got[0].name, "fleet/a");
        assert_eq!(got[0].tip, "t1");
        assert_eq!(got[1].tip, "t2");
        assert!(members("x t1 t2", "b", &names).is_err());
        assert!(members("b t1", "b", &names).is_err());
    }

    #[test]
    fn build_data_has_a_stable_form() {
        let data = Build::Conflict {
            member: "fleet/a".into(),
            files: vec!["x".into()],
        };
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"conflict": {"member": "fleet/a", "files": ["x"]}})
        );
        assert_eq!(candidate_ref(7), "refs/horch/bundle/7");
    }
}
