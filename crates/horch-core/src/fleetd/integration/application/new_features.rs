use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, FeatureFact, NewBranch, NewFeature,
};
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::taskledger::model::EventKind;
use crate::taskledger::runs;

pub const FEATURE_NAME_KIND: &str = "feature_name";

pub fn feature_name_key(idea: &str) -> String {
    format!("{FEATURE_NAME_KIND}:{idea}")
}

pub const MAX_NAME: usize = 64;

const MAX_TITLE_NAME: usize = MAX_NAME - 8;

const FALLBACK_NAME: &str = "feature";

const BRANCH_PREFIX: &str = "fleet/";

const BRANCH_OWNER: &str = "fleetd";

const BRANCH_LIMIT_S: i64 = 3600;

#[derive(Debug, Clone, Copy)]
pub struct Project<'a> {
    pub root: &'a Path,
    pub primary_branch: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub enum NameSource<'a> {
    SubOrchestrator { idea: &'a str, name: &'a str },
    Direct { title: &'a str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRef {
    pub repo: PathBuf,
    pub name: String,
    pub tip: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    Accepted {
        name: String,
        branch: String,
        effect: BranchRef,
    },
    Refused {
        reason: String,
    },
}

pub fn is_kebab_case(name: &str) -> bool {
    name.len() <= MAX_NAME
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

pub fn name_from_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let mut name = String::new();
    for word in words {
        let extra = usize::from(!name.is_empty()) + word.len();
        if name.len() + extra > MAX_TITLE_NAME {
            if name.is_empty() {
                name.push_str(&word[..MAX_TITLE_NAME]);
            }
            break;
        }
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(word);
    }
    if name.is_empty() {
        FALLBACK_NAME.to_string()
    } else {
        name
    }
}

pub fn open_feature(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    project: &Project<'_>,
    cause: Cause<'_>,
) -> Result<String> {
    let id = runs::create_feature_run(tx, project.root, project.primary_branch)?;
    let new = NewFeature {
        id: &id,
        name: None,
        branch: None,
        primary_branch: project.primary_branch,
    };
    rows::insert_feature(tx, ctx.guard, &new, cause)?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub fn name_feature_branch(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    git: &dyn IntegrationGitGateway,
    feature: &str,
    source: NameSource<'_>,
    primary_tip: &str,
    clock: FleetSeconds,
    cause: Cause<'_>,
) -> Result<Named> {
    let Some(row) = rows::feature(tx, feature)? else {
        bail!("the ledger has no feature {feature}");
    };
    if !matches!(row.state.as_str(), "open" | "ready") {
        bail!(
            "feature {feature} is {}: only an open or ready feature gets a branch",
            row.state
        );
    }
    if let Some(branch) = &row.branch {
        bail!("feature {feature} already has the branch {branch}");
    }
    let Some(run) = runs::feature_run(tx, feature)? else {
        bail!("the task ledger has no internal run of feature {feature}");
    };
    let repo = PathBuf::from(&run.project_root);
    let git_branches: Vec<String> = git
        .fleet_branches(&repo)?
        .into_iter()
        .map(|b| b.name)
        .collect();
    let taken = |name: &str| -> Result<bool> {
        let branch = branch_of(name);
        let below = format!("{branch}/");
        Ok(rows::name_taken(tx, name, &branch)?
            || git_branches
                .iter()
                .any(|b| *b == branch || b.starts_with(&below)))
    };
    let name = match source {
        NameSource::Direct { title } => {
            let base = name_from_title(title);
            let mut name = base.clone();
            let mut n = 1;
            while taken(&name)? {
                n += 1;
                name = format!("{base}-{n}");
            }
            name
        }
        NameSource::SubOrchestrator { idea, name } => {
            let reason = if !is_kebab_case(name) {
                Some(format!(
                    "the feature name \"{name}\" is not kebab-case: use a-z, 0-9 and single \
                     '-', at most {MAX_NAME} characters"
                ))
            } else if taken(name)? {
                Some(format!(
                    "the feature name \"{name}\" exists: give a name that no feature or fleet \
                     branch has"
                ))
            } else {
                None
            };
            if let Some(reason) = reason {
                refuse(tx, ctx, feature, idea, &reason, clock, cause)?;
                return Ok(Named::Refused { reason });
            }
            ctx.needs_you.resolve_item(tx, &feature_name_key(idea))?;
            name.to_string()
        }
    };
    let branch = branch_of(&name);
    rows::set_feature_fact(tx, ctx.guard, feature, FeatureFact::Name(&name), cause)?;
    rows::set_feature_fact(tx, ctx.guard, feature, FeatureFact::Branch(&branch), cause)?;
    let new = NewBranch {
        name: &branch,
        feature_id: feature,
        task_id: None,
        owner: BRANCH_OWNER,
        due_at: clock.0.saturating_add(BRANCH_LIMIT_S),
    };
    rows::insert_branch(tx, ctx.guard, &new, cause)?;
    rows::set_branch_fact(
        tx,
        ctx.guard,
        &branch,
        BranchFact::SeenTip(primary_tip),
        cause,
    )?;
    runs::set_feature_run_branch(tx, feature, &branch)?;
    let effect = BranchRef {
        repo,
        name: format!("refs/heads/{branch}"),
        tip: primary_tip.to_string(),
    };
    Ok(Named::Accepted {
        name,
        branch,
        effect,
    })
}

fn branch_of(name: &str) -> String {
    format!("{BRANCH_PREFIX}{name}")
}

fn refuse(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    feature: &str,
    idea: &str,
    reason: &str,
    clock: FleetSeconds,
    cause: Cause<'_>,
) -> Result<()> {
    let key = feature_name_key(idea);
    let item = NewItem {
        key: &key,
        kind: FEATURE_NAME_KIND,
        entity: Entity::Idea,
        entity_id: idea,
        text: reason,
    };
    ctx.needs_you.open_item(tx, &item)?;
    let data = json!({
        "fleet_clock": clock.0,
        "owner": format!("idea-lead:{feature}"),
        "text": reason,
    });
    let bell = Cause {
        actor: cause.actor,
        text: reason,
    };
    record(
        tx,
        ctx.guard,
        EventKind::Doorbell,
        Entity::Idea,
        idea,
        bell,
        &data,
    )?;
    Ok(())
}

pub fn join_feature(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    feature: &str,
    cause: Cause<'_>,
) -> Result<String> {
    let row = match rows::feature(tx, feature)? {
        Some(row) => row,
        None => match rows::feature_named(tx, feature)? {
            Some(row) => row,
            None => bail!("the ledger has no feature {feature}"),
        },
    };
    match row.state.as_str() {
        "open" => {}
        "ready" => {
            let refusals: &dyn RecordRefusal = ctx.needs_you;
            transition(
                tx,
                ctx.guard,
                Entity::Feature,
                &row.id,
                "open",
                cause,
                refusals,
            )?;
        }
        state => bail!(
            "feature {} is {state}: an idea joins only an open or ready feature; add the idea \
             with no --to to start a new feature",
            row.id
        ),
    }
    Ok(row.id)
}

pub fn create_ref(git: &dyn IntegrationGitGateway, r: &BranchRef) -> Result<bool> {
    match git.update_ref(&r.repo, &r.name, &r.tip, None)? {
        RefMove::Moved => Ok(true),
        RefMove::Mismatch { actual: Some(_) } => Ok(false),
        RefMove::Mismatch { actual: None } => {
            bail!("git did not create {} in {}", r.name, r.repo.display())
        }
    }
}

pub fn pending_refs(conn: &Connection, git: &dyn IntegrationGitGateway) -> Result<Vec<BranchRef>> {
    let mut out = Vec::new();
    for (feature, branch, tip) in rows::live_feature_branches(conn)? {
        let Some(run) = runs::feature_run(conn, &feature)? else {
            continue;
        };
        let repo = PathBuf::from(&run.project_root);
        let name = format!("refs/heads/{branch}");
        if git.tip(&repo, &name)?.is_none() {
            out.push(BranchRef { repo, name, tip });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_case_names() {
        for good in ["a", "retry-sync", "v2", "retry-sync-2"] {
            assert!(is_kebab_case(good), "{good}");
        }
        let long = "a".repeat(MAX_NAME + 1);
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "A",
            "a_b",
            "a b",
            "é",
            long.as_str(),
        ] {
            assert!(!is_kebab_case(bad), "{bad}");
        }
    }

    #[test]
    fn names_from_titles() {
        assert_eq!(name_from_title("Retry sync!"), "retry-sync");
        assert_eq!(
            name_from_title("  15: New features --to  "),
            "15-new-features-to"
        );
        assert_eq!(name_from_title("¿?"), FALLBACK_NAME);
        let long = name_from_title(&"word ".repeat(40));
        assert!(
            long.len() <= MAX_TITLE_NAME && is_kebab_case(&long),
            "{long}"
        );
        let one = name_from_title(&"x".repeat(200));
        assert_eq!(one.len(), MAX_TITLE_NAME);
    }
}
