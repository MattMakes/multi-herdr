use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::result_data;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::domain::project;
use crate::fleetd::worktree_custody::adapters::rescue_refs_git::facts_of;
use crate::fleetd::worktree_custody::application::rescue_refs::class;
use crate::fleetd::worktree_custody::domain::rescue_class::Class;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Integration {
    pub holds: Vec<ProjectHold>,
    pub bundle_refs: u64,
    pub rescue: Option<RescueClasses>,
    pub features: Vec<FeaturePush>,
    pub stale_remotes: Vec<StaleRemote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectHold {
    pub project: String,
    pub cause: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ClassTally {
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RescueClasses {
    pub unique: ClassTally,
    pub superseded: ClassTally,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeaturePush {
    pub id: String,
    pub name: Option<String>,
    pub state: String,
    pub push_state: Option<String>,
    pub push_failures: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaleRemote {
    pub repo: String,
    pub branch: String,
    pub stale_since: String,
}

pub fn integration(conn: &Connection) -> Result<Integration> {
    Ok(Integration {
        holds: holds(conn)?,
        bundle_refs: conn.query_row(
            "SELECT COUNT(*) FROM bundles WHERE candidate_ref IS NOT NULL",
            [],
            |r| r.get(0),
        )?,
        rescue: rescue(conn)?,
        features: features(conn)?,
        stale_remotes: stale_remotes(conn)?,
    })
}

fn holds(conn: &Connection) -> Result<Vec<ProjectHold>> {
    Ok(needs_you::open_items(conn)?
        .into_iter()
        .filter(|i| i.kind == project::MISSING_FAST_GATE_KIND)
        .map(|i| ProjectHold {
            cause: project::missing_fast_gate_cause(&i.entity_id),
            project: i.entity_id,
        })
        .collect())
}

fn rescue(conn: &Connection) -> Result<Option<RescueClasses>> {
    let mut out: Option<RescueClasses> = None;
    for row in jobs_store::last_passed(conn, Kind::WorktreeList)? {
        let Some(refs) = result_data(&row).as_ref().and_then(facts_of) else {
            continue;
        };
        let c = out.get_or_insert_with(RescueClasses::default);
        for r in &refs {
            let tally = match class(r) {
                Class::Unique => &mut c.unique,
                Class::Superseded => &mut c.superseded,
            };
            tally.count += 1;
            tally.bytes = tally.bytes.saturating_add(r.bytes.unwrap_or(0));
        }
    }
    Ok(out)
}

fn features(conn: &Connection) -> Result<Vec<FeaturePush>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, state, push_state, push_failures FROM features
         WHERE state IN ('open', 'ready') ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(FeaturePush {
            id: r.get(0)?,
            name: r.get(1)?,
            state: r.get(2)?,
            push_state: r.get(3)?,
            push_failures: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn stale_remotes(conn: &Connection) -> Result<Vec<StaleRemote>> {
    let mut stmt = conn.prepare(
        "SELECT repo, branch, stale_since FROM remote_tips
         WHERE stale_since IS NOT NULL ORDER BY repo, branch",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(StaleRemote {
            repo: r.get(0)?,
            branch: r.get(1)?,
            stale_since: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
