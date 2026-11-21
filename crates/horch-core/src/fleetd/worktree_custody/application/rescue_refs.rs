//! The use cases of the rescue refs and their classes (ticket 24).
//!
//! `docs/specs/fleetd/design.md`, "No work is lost" (Rescue refs have 2
//! classes, and fleetd owns both), I10, FM-40, RR-16; MAP.md of fleetd-p4,
//! Q16 and Q17:
//! - [`record_branch_rescue`]: `branches.rescue_ref` names the last rescue
//!   ref of the branch.
//! - [`collect`]: a pass part with no git call. It decides the class of each
//!   rescue ref from the facts of the last `worktree_list` run, opens or
//!   ends the needs-you item of the `unique` budget, and plans the deletion
//!   of each `superseded` ref whose feature ended `retention_days` ago.
//! - [`delete`]: the short effect after the commit, `git update-ref -d
//!   <ref> <old>`. The next `worktree_list` run shows it; a ref that is still
//!   there is planned again by the next pass.
//!
//! - [`plan_drop`], [`record_drop`] and [`drop_ref`]: `horch rescue drop
//!   <ref>` (ticket 43). You plan the drop of 1 ref, its event commits, and
//!   then the ref is deleted and read back.
//!
//! No use case of fleetd deletes a `unique` ref: only your drop does (I10).
//! Over the budget, fleetd deletes nothing (FM-40).

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, last_moves_to, Fact};
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::RefFacts;
use crate::fleetd::worktree_custody::adapters::sqlite as store;
use crate::fleetd::worktree_custody::domain::rescue::REF_PREFIX;
use crate::fleetd::worktree_custody::domain::rescue_class::{
    self as rules, Class, Content, FeatureRefs, Owner, RefOwner, BUDGET_KEY, ENDED, NEEDS_YOU_KEY,
};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::store::one_snapshot;

/// The rescue ref facts of 1 repository, from its last passed
/// `worktree_list` run. `refs` is `None` when the run could not list them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub repo: String,
    pub refs: Option<Vec<RefFacts>>,
}

/// The count and the bytes of 1 class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub count: u64,
    pub bytes: u64,
}

/// 1 planned deletion: `git update-ref -d <name> <tip>` in `repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub repo: String,
    pub name: String,
    pub tip: String,
}

/// What 1 call of [`collect`] found and planned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collection {
    pub unique: Tally,
    pub superseded: Tally,
    /// The `unique` class is over `rescue_unique_budget_mb`.
    pub over_budget: bool,
    /// The deletions to run after the commit.
    pub deletions: Vec<Deletion>,
}

/// What 1 deletion did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deleted {
    /// git deleted the ref.
    Deleted,
    /// The ref did not exist: an earlier effect deleted it.
    Gone,
    /// The ref names another tip, so it stays.
    Moved(String),
}

/// Record `rescue_ref` as the last rescue ref of the branch row `branch`,
/// with 1 event, in `tx`. `false` when the ledger has no row of the branch.
/// A row that already names `rescue_ref` does not change.
pub fn record_branch_rescue(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    branch: &str,
    rescue_ref: &str,
) -> Result<bool> {
    let Some(row) = rows::branch(tx, branch)? else {
        return Ok(false);
    };
    if row.rescue_ref.as_deref() != Some(rescue_ref) {
        let cause = Cause {
            actor: Actor::Horch,
            text: "the rescue ref of the old tip is written and read back",
        };
        rows::set_branch_fact(
            tx,
            ctx.guard,
            branch,
            BranchFact::RescueRef(rescue_ref),
            cause,
        )?;
    }
    Ok(true)
}

/// The branch and the feature of the owner `owner` of a rescue ref, from
/// the ledger. `None` when the ledger has no row, or when more than 1
/// branch has the folder name of a branch ref.
pub fn owner_of(conn: &Connection, owner: &RefOwner) -> Result<Option<Owner>> {
    let branch = match owner {
        RefOwner::Branch(folder) => {
            let mut names = rows::branch_names(conn)?
                .into_iter()
                .filter(|n| rules::branch_folder_name(n).as_deref() == Some(folder.as_str()));
            match (names.next(), names.next()) {
                (Some(name), None) => name,
                _ => return Ok(None),
            }
        }
        RefOwner::Worktree(id) => match store::get(conn, id)? {
            Some(row) => row.branch,
            None => return Ok(None),
        },
    };
    let feature = match rows::branch(conn, &branch)? {
        Some(row) => rows::feature(conn, &row.feature_id)?.map(|f| FeatureRefs {
            id: f.id,
            branch: f.branch,
            primary: f.primary_branch,
        }),
        None => None,
    };
    Ok(Some(Owner { branch, feature }))
}

/// The class of the ref of `facts`.
pub fn class(facts: &RefFacts) -> Class {
    rules::classify(&Content {
        in_newer_tip: facts.in_newer_tip,
        in_landed: facts.in_landed,
    })
}

/// Decide the class of each rescue ref of `lists`, in `tx`, with no git
/// call:
/// - over `rescue_unique_budget_mb`, 1 needs-you item is open; under it,
///   with the facts of each repository, the item ends. Nothing is deleted
///   for the budget.
/// - each `superseded` ref whose feature is `merged` or `abandoned` since
///   `retention_days` before `now` gets 1 [`Deletion`].
pub fn collect(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    lists: &[RepoFacts],
    now: DateTime<Utc>,
) -> Result<Collection> {
    let mut c = Collection::default();
    let features = lists
        .iter()
        .flat_map(|l| l.refs.iter().flatten())
        .filter_map(|r| r.feature.as_deref());
    let ended_at = features_ended_s(tx, features)?;
    for list in lists {
        for r in list.refs.iter().flatten() {
            let class = class(r);
            let tally = match class {
                Class::Unique => &mut c.unique,
                Class::Superseded => &mut c.superseded,
            };
            tally.count += 1;
            tally.bytes = tally.bytes.saturating_add(r.bytes.unwrap_or(0));
            let ended = r.feature.as_ref().and_then(|id| ended_at.get(id).copied());
            if rules::expired(class, ended, now.timestamp(), ctx.config.retention_days) {
                c.deletions.push(Deletion {
                    repo: list.repo.clone(),
                    name: r.name.clone(),
                    tip: r.tip.clone(),
                });
            }
        }
    }
    let budget = ctx.config.rescue_unique_budget_mb;
    c.over_budget = rules::over_budget(c.unique.bytes, budget);
    if c.over_budget {
        let text = format!(
            "The unique rescue refs hold {} MB, over {BUDGET_KEY} = {budget}. fleetd deletes \
             no unique rescue ref. Restore the work with horch worktree restore, drop a ref \
             with horch rescue drop <ref>, or raise {BUDGET_KEY}.",
            c.unique.bytes / (1024 * 1024)
        );
        let item = NewItem {
            key: NEEDS_YOU_KEY,
            kind: NEEDS_YOU_KEY,
            entity: Entity::Worktree,
            entity_id: BUDGET_KEY,
            text: &text,
        };
        ctx.needs_you.open_item(tx, &item)?;
    } else if lists.iter().all(|l| l.refs.is_some()) {
        ctx.needs_you.resolve_item(tx, NEEDS_YOU_KEY)?;
    }
    Ok(c)
}

/// When each feature of `ids` that is `merged` or `abandoned` moved to 1
/// of those states, in Unix seconds, by id. A feature that is open or
/// ready, or with no such move, is not in the map. 1 snapshot reads the
/// states and the last move to each ended state: 3 statements for any
/// count of features.
fn features_ended_s<'a>(
    conn: &Connection,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeMap<String, i64>> {
    one_snapshot(conn, || {
        let states = rows::feature_states(conn, ids)?;
        let ended = || {
            states
                .iter()
                .filter(|(_, s)| ENDED.contains(&s.as_str()))
                .map(|(id, _)| id.as_str())
        };
        let mut at: BTreeMap<String, String> = BTreeMap::new();
        for state in ENDED {
            for (id, t) in last_moves_to(conn, Entity::Feature, ended(), state)? {
                let last = at.entry(id).or_default();
                *last = std::cmp::max(last.clone(), t);
            }
        }
        Ok(at
            .into_iter()
            .filter_map(|(id, t)| Some((id, crate::clock::parse(&t)?.timestamp())))
            .collect())
    })
}

/// Delete the ref of `d` only when it still names `d.tip`: `git update-ref
/// -d <ref> <old>`. Run it after the commit of [`collect`]. An error leaves
/// the ref, and the next pass plans it again.
pub fn delete(git: &dyn IntegrationGitGateway, d: &Deletion) -> Result<Deleted> {
    let repo = std::path::Path::new(&d.repo);
    Ok(match git.delete_ref(repo, &d.name, &d.tip)? {
        RefMove::Moved => Deleted::Deleted,
        RefMove::Mismatch { actual: None } => Deleted::Gone,
        RefMove::Mismatch { actual: Some(tip) } => Deleted::Moved(tip),
    })
}

/// The plan of `horch rescue drop <ref>`: the ref, its tip, its class and
/// the bytes that only it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPlan {
    pub deletion: Deletion,
    pub class: Class,
    pub bytes: Option<u64>,
}

/// Plan the drop of the rescue ref `name` of `repo` from `refs`, the facts
/// of its rescue refs. A name outside `refs/horch/rescue/`, or a ref that
/// `refs` does not have, is an error.
pub fn plan_drop(repo: &str, name: &str, refs: &[RefFacts]) -> Result<DropPlan> {
    if rules::parse(name).is_none() {
        bail!("{name} is not a rescue ref: horch rescue drop deletes only a ref {REF_PREFIX}<owner>/<n>");
    }
    let Some(facts) = refs.iter().find(|r| r.name == name) else {
        bail!("the rescue ref {name} does not exist in {repo}");
    };
    Ok(DropPlan {
        deletion: Deletion {
            repo: repo.to_string(),
            name: name.to_string(),
            tip: facts.tip.clone(),
        },
        class: class(facts),
        bytes: facts.bytes,
    })
}

/// The cause of the event of your drop of a ref at `tip`.
fn drop_cause(tip: &str) -> String {
    format!("the operator drops the rescue ref at {tip}")
}

/// Record your drop of `plan` with 1 event about the ref, in `tx`, before
/// the effect. `false`, with no write, when that event of the ref at that
/// tip exists: an earlier drop committed it and died before its effect.
pub fn record_drop(tx: &Connection, plan: &DropPlan) -> Result<bool> {
    let d = &plan.deletion;
    let cause = drop_cause(&d.tip);
    if door::has_fact(tx, Entity::Worktree, &d.name, EventKind::Note, &cause)? {
        return Ok(false);
    }
    let data = serde_json::json!({
        "ref": d.name,
        "tip": d.tip,
        "class": plan.class.as_str(),
        "bytes": plan.bytes,
    });
    door::append_fact(
        tx,
        &Fact {
            kind: EventKind::Note,
            entity: Entity::Worktree,
            id: &d.name,
            cause: &cause,
            actor: Actor::Operator,
            data_json: &data.to_string(),
        },
    )?;
    Ok(true)
}

/// The effect after the commit of [`record_drop`]: [`delete`], then read
/// the ref back. A ref at another tip stays, and is an error. So is a ref
/// that git still reads back.
pub fn drop_ref(git: &dyn IntegrationGitGateway, d: &Deletion) -> Result<Deleted> {
    let done = delete(git, d)?;
    if let Deleted::Moved(tip) = &done {
        bail!(
            "the rescue ref {} is now at {tip}, not {}; nothing is deleted",
            d.name,
            d.tip
        );
    }
    if let Some(tip) = git.tip(std::path::Path::new(&d.repo), &d.name)? {
        bail!(
            "the rescue ref {} still reads back {tip} after its deletion",
            d.name
        );
    }
    Ok(done)
}

/// T-408: collect reads the features of all rescue refs in set reads.
#[cfg(test)]
mod collect_reads_tests {
    use rusqlite::params;

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::sqlite::{append_event, Event};
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;

    /// The feature `id` in `state`, moved to it at `at` when `at` is given.
    fn feature(c: &Connection, id: &str, state: &str, at: Option<&str>) {
        c.execute(
            "INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
             VALUES (?1, ?1, ?1, 'main', ?2, 'now', 'now')",
            params![id, state],
        )
        .unwrap();
        let Some(at) = at else {
            return;
        };
        let e = Event {
            entity: Entity::Feature,
            id,
            from: Some("open"),
            to: state,
            cause: "test",
            actor: Actor::Horch,
        };
        append_event(c, &e).unwrap();
        c.execute(
            "UPDATE events SET at = ?1 WHERE event_id = last_insert_rowid()",
            [at],
        )
        .unwrap();
    }

    fn superseded(name: &str, feature: &str) -> RefFacts {
        RefFacts {
            name: name.to_string(),
            tip: "t".to_string(),
            branch: Some(feature.to_string()),
            feature: Some(feature.to_string()),
            bytes: Some(1),
            in_newer_tip: Some(true),
            in_landed: None,
        }
    }

    /// For each i of `n`: a ref of a merged and of an abandoned feature,
    /// ended long ago, is deleted; a ref of an open feature, of a merged
    /// feature with no move event, and of a feature with no row stays.
    /// The deletions are the ones of the read of each feature, and 3 and
    /// 300 of each run the same statements.
    #[test]
    fn collect_of_3_and_300_refs_of_each_kind_runs_the_same_statements() {
        let count = |n: usize| {
            let tmp = tempfile::tempdir().unwrap();
            let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
            let tx = store.conn_mut().transaction().unwrap();
            let old = Some("2020-01-01T00:00:00Z");
            let mut refs = Vec::new();
            let mut want = Vec::new();
            for i in 0..n {
                for (kind, state, at, deleted) in [
                    ("M", "merged", old, true),
                    ("A", "abandoned", old, true),
                    ("O", "open", None, false),
                    ("N", "merged", None, false),
                ] {
                    let id = format!("{kind}{i:04}");
                    feature(&tx, &id, state, at);
                    let name = format!("refs/horch/rescue/{id}/1");
                    refs.push(superseded(&name, &id));
                    if deleted {
                        want.push(name);
                    }
                }
                refs.push(superseded(
                    &format!("refs/horch/rescue/X{i:04}/1"),
                    "no-row",
                ));
            }
            let lists = [RepoFacts {
                repo: "/p".to_string(),
                refs: Some(refs),
            }];
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let now = crate::clock::parse("2026-11-21T00:00:00Z").unwrap();
            let (c, statements) =
                count_statements(&tx, || collect(&ctx, &tx, &lists, now).unwrap());
            let got: Vec<String> = c.deletions.into_iter().map(|d| d.name).collect();
            assert_eq!(got, want);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}
