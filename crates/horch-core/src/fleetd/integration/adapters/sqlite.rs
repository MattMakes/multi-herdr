use anyhow::{bail, Result};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, record, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::taskledger::model::EventKind;
use crate::taskledger::util;

const FEATURE_FIRST: &str = "open";
const BRANCH_FIRST: &str = "building";
const BUNDLE_FIRST: &str = "building";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRow {
    pub id: String,
    pub name: Option<String>,
    pub branch: Option<String>,
    pub primary_branch: String,
    pub stacked_on: Option<String>,
    pub state: String,
    pub pr: Option<String>,
    pub push_state: Option<String>,
    pub push_failures: i64,
    pub push_not_before: Option<i64>,
    pub pr_state: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRow {
    pub name: String,
    pub feature_id: String,
    pub task_id: Option<String>,
    pub landing_state: Option<String>,
    pub owner: String,
    pub due_at: i64,
    pub queue_seq: Option<i64>,
    pub repair_attempts: i64,
    pub rescue_ref: Option<String>,
    pub seen_tip: Option<String>,
    pub seen_remote_tip: Option<String>,
    pub orphan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRow {
    pub id: i64,
    pub feature_id: String,
    pub base_tip: String,
    pub candidate_ref: Option<String>,
    pub candidate_tip: Option<String>,
    pub parent_id: Option<i64>,
    pub state: String,
    pub job_id: Option<i64>,
    pub due_at: i64,
    pub result: Option<String>,
    pub at: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRunRow {
    pub id: i64,
    pub feature_id: String,
    pub gate: String,
    pub tip: String,
    pub base_tip: String,
    pub job_id: i64,
    pub result: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTipRow {
    pub repo: String,
    pub branch: String,
    pub tip: String,
    pub fetched_at: String,
    pub stale_since: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewFeature<'a> {
    pub id: &'a str,
    pub name: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub primary_branch: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct NewBranch<'a> {
    pub name: &'a str,
    pub feature_id: &'a str,
    pub task_id: Option<&'a str>,
    pub owner: &'a str,
    pub due_at: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewBundle<'a> {
    pub feature_id: &'a str,
    pub base_tip: &'a str,
    pub parent_id: Option<i64>,
    pub members: &'a [&'a str],
    pub due_at: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewGateRun<'a> {
    pub feature_id: &'a str,
    pub gate: &'a str,
    pub tip: &'a str,
    pub base_tip: &'a str,
    pub job_id: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum BranchFact<'a> {
    SeenTip(&'a str),
    SeenRemoteTip(&'a str),
    QueueSeq(Option<i64>),
    RepairAttempts(i64),
    DueAt(i64),
    RescueRef(&'a str),
    Owner(&'a str),
    Orphan,
}

impl BranchFact<'_> {
    fn cell(self) -> (&'static str, Value) {
        match self {
            BranchFact::SeenTip(t) => ("seen_tip", text(Some(t))),
            BranchFact::SeenRemoteTip(t) => ("seen_remote_tip", text(Some(t))),
            BranchFact::QueueSeq(n) => ("queue_seq", int(n)),
            BranchFact::RepairAttempts(n) => ("repair_attempts", int(Some(n))),
            BranchFact::DueAt(n) => ("due_at", int(Some(n))),
            BranchFact::RescueRef(r) => ("rescue_ref", text(Some(r))),
            BranchFact::Owner(o) => ("owner", text(Some(o))),
            BranchFact::Orphan => ("orphan", int(Some(1))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BundleFact<'a> {
    CandidateRef(Option<&'a str>),
    CandidateTip(&'a str),
    JobId(i64),
    DueAt(i64),
}

impl BundleFact<'_> {
    fn cell(self) -> (&'static str, Value) {
        match self {
            BundleFact::CandidateRef(r) => ("candidate_ref", text(r)),
            BundleFact::CandidateTip(t) => ("candidate_tip", text(Some(t))),
            BundleFact::JobId(n) => ("job_id", int(Some(n))),
            BundleFact::DueAt(n) => ("due_at", int(Some(n))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FeatureFact<'a> {
    PushState(&'a str),
    PushFailures(i64),
    PushNotBefore(Option<i64>),
    Pr(&'a str),
    PrState(&'a str),
    Name(&'a str),
    Branch(&'a str),
}

impl FeatureFact<'_> {
    fn cell(self) -> (&'static str, Value) {
        match self {
            FeatureFact::PushState(s) => ("push_state", text(Some(s))),
            FeatureFact::PushFailures(n) => ("push_failures", int(Some(n))),
            FeatureFact::PushNotBefore(n) => ("push_not_before", int(n)),
            FeatureFact::Pr(s) => ("pr", text(Some(s))),
            FeatureFact::PrState(s) => ("pr_state", text(Some(s))),
            FeatureFact::Name(s) => ("name", text(Some(s))),
            FeatureFact::Branch(s) => ("branch", text(Some(s))),
        }
    }
}

fn text(t: Option<&str>) -> Value {
    t.map_or(Value::Null, |t| Value::Text(t.to_string()))
}

fn int(n: Option<i64>) -> Value {
    n.map_or(Value::Null, Value::Integer)
}

fn fact_data(branch: Option<&str>, column: &str, value: &Value) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    if let Some(name) = branch {
        data.insert("branch".into(), json!(name));
    }
    let value = match value {
        Value::Integer(n) => json!(n),
        Value::Text(t) => json!(t),
        _ => serde_json::Value::Null,
    };
    data.insert(column.into(), value);
    serde_json::Value::Object(data)
}

pub fn insert_feature(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewFeature<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let now = util::now();
    tx.execute(
        "INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            new.id,
            new.name,
            new.branch,
            new.primary_branch,
            FEATURE_FIRST,
            now
        ],
    )?;
    created(tx, guard, Entity::Feature, new.id, cause)?;
    Ok(())
}

pub fn insert_branch(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewBranch<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let state = new.task_id.map(|_| BRANCH_FIRST);
    tx.execute(
        "INSERT INTO branches (name, feature_id, task_id, landing_state, owner, due_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new.name,
            new.feature_id,
            new.task_id,
            state,
            new.owner,
            new.due_at
        ],
    )?;
    if state.is_some() {
        created(tx, guard, Entity::TaskBranch, new.name, cause)?;
    } else {
        let data = json!({ "branch": new.name });
        record(
            tx,
            guard,
            EventKind::Note,
            Entity::Feature,
            new.feature_id,
            cause,
            &data,
        )?;
    }
    Ok(())
}

pub fn insert_bundle(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewBundle<'_>,
    cause: Cause<'_>,
) -> Result<i64> {
    if new.members.is_empty() {
        bail!("a bundle needs 1 or more task branches");
    }
    check_writable(tx, guard)?;
    tx.execute_batch("SAVEPOINT insert_bundle")?;
    match bundle_rows(tx, guard, new, cause) {
        Ok(id) => {
            tx.execute_batch("RELEASE insert_bundle")?;
            Ok(id)
        }
        Err(e) => {
            tx.execute_batch("ROLLBACK TO insert_bundle; RELEASE insert_bundle")?;
            Err(e)
        }
    }
}

fn bundle_rows(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewBundle<'_>,
    cause: Cause<'_>,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO bundles (feature_id, base_tip, parent_id, state, due_at, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new.feature_id,
            new.base_tip,
            new.parent_id,
            BUNDLE_FIRST,
            new.due_at,
            util::now()
        ],
    )?;
    let id = tx.last_insert_rowid();
    for (position, branch) in new.members.iter().enumerate() {
        tx.execute(
            "INSERT INTO bundle_members (bundle_id, branch, position) VALUES (?1, ?2, ?3)",
            params![id, branch, position as i64],
        )?;
    }
    created(tx, guard, Entity::Bundle, &id.to_string(), cause)?;
    Ok(id)
}

pub fn insert_gate_run(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewGateRun<'_>,
    cause: Cause<'_>,
) -> Result<i64> {
    check_writable(tx, guard)?;
    tx.execute(
        "INSERT INTO gate_runs (feature_id, gate, tip, base_tip, job_id, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new.feature_id,
            new.gate,
            new.tip,
            new.base_tip,
            new.job_id,
            util::now()
        ],
    )?;
    let id = tx.last_insert_rowid();
    let data = json!({ "gate_run": id, "gate": new.gate, "tip": new.tip });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        new.feature_id,
        cause,
        &data,
    )?;
    Ok(id)
}

pub fn put_remote_tip(
    tx: &Connection,
    guard: Option<Generation>,
    repo: &str,
    branch: &str,
    tip: &str,
) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "INSERT OR REPLACE INTO remote_tips (repo, branch, tip, fetched_at, stale_since)
         VALUES (?1, ?2, ?3, ?4, NULL)",
        params![repo, branch, tip, util::now()],
    )?;
    Ok(())
}

pub fn set_remote_stale(
    tx: &Connection,
    guard: Option<Generation>,
    repo: &str,
    stale: bool,
) -> Result<()> {
    check_writable(tx, guard)?;
    if stale {
        tx.execute(
            "UPDATE remote_tips SET stale_since = ?2 WHERE repo = ?1 AND stale_since IS NULL",
            params![repo, util::now()],
        )?;
    } else {
        tx.execute(
            "UPDATE remote_tips SET stale_since = NULL WHERE repo = ?1",
            params![repo],
        )?;
    }
    Ok(())
}

pub fn set_branch_fact(
    tx: &Connection,
    guard: Option<Generation>,
    name: &str,
    fact: BranchFact<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let Some(row) = branch(tx, name)? else {
        bail!("no branch {name} in the ledger");
    };
    let (column, value) = fact.cell();
    tx.execute(
        &format!("UPDATE branches SET {column} = ?2 WHERE name = ?1"),
        params![name, value],
    )?;
    let data = fact_data(Some(name), column, &value);
    let (entity, id) = match row.landing_state {
        Some(_) => (Entity::TaskBranch, name),
        None => (Entity::Feature, row.feature_id.as_str()),
    };
    record(tx, guard, EventKind::Note, entity, id, cause, &data)?;
    Ok(())
}

pub fn set_bundle_fact(
    tx: &Connection,
    guard: Option<Generation>,
    id: i64,
    fact: BundleFact<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let (column, value) = fact.cell();
    let changed = tx.execute(
        &format!("UPDATE bundles SET {column} = ?2 WHERE id = ?1"),
        params![id, value],
    )?;
    if changed != 1 {
        bail!("no bundle {id} in the ledger");
    }
    let data = fact_data(None, column, &value);
    let key = id.to_string();
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Bundle,
        &key,
        cause,
        &data,
    )?;
    Ok(())
}

pub fn set_feature_fact(
    tx: &Connection,
    guard: Option<Generation>,
    id: &str,
    fact: FeatureFact<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let (column, value) = fact.cell();
    let changed = tx.execute(
        &format!("UPDATE features SET {column} = ?2, updated_at = ?3 WHERE id = ?1"),
        params![id, value, util::now()],
    )?;
    if changed != 1 {
        bail!("no feature {id} in the ledger");
    }
    let data = fact_data(None, column, &value);
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        id,
        cause,
        &data,
    )?;
    Ok(())
}

pub fn feature(conn: &Connection, id: &str) -> Result<Option<FeatureRow>> {
    let row = |r: &Row<'_>| {
        Ok(FeatureRow {
            id: r.get(0)?,
            name: r.get(1)?,
            branch: r.get(2)?,
            primary_branch: r.get(3)?,
            stacked_on: r.get(4)?,
            state: r.get(5)?,
            pr: r.get(6)?,
            push_state: r.get(7)?,
            push_failures: r.get(8)?,
            push_not_before: r.get(9)?,
            pr_state: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
        })
    };
    Ok(conn
        .query_row(
            "SELECT id, name, branch, primary_branch, stacked_on, state, pr, push_state,
                push_failures, push_not_before, pr_state, created_at, updated_at
             FROM features WHERE id = ?1",
            [id],
            row,
        )
        .optional()?)
}

pub fn feature_named(conn: &Connection, name: &str) -> Result<Option<FeatureRow>> {
    let id: Option<String> = conn
        .query_row("SELECT id FROM features WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .optional()?;
    match id {
        Some(id) => feature(conn, &id),
        None => Ok(None),
    }
}

pub fn name_taken(conn: &Connection, name: &str, branch: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM features WHERE name = ?1 OR branch = ?2)
             OR EXISTS (SELECT 1 FROM branches WHERE name = ?2)",
        [name, branch],
        |r| r.get(0),
    )?)
}

pub fn feature_states<'a>(
    conn: &Connection,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let ids: Vec<&str> = ids.into_iter().collect();
    let sql = format!(
        "SELECT id, state FROM features WHERE id IN ({})",
        crate::taskledger::sql_words::marks(1, ids.len())
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn feature_ids(conn: &Connection, live: bool) -> Result<Vec<String>> {
    let sql = if live {
        "SELECT id FROM features WHERE state IN ('open', 'ready') ORDER BY id"
    } else {
        "SELECT id FROM features ORDER BY id"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn bundles_to_follow(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM bundles
         WHERE state IN ('building', 'gating', 'passed') OR candidate_ref IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn dropped_without_candidate(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT id, job_id FROM bundles
         WHERE state = 'dropped' AND candidate_tip IS NULL AND job_id IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn live_feature_branches(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.branch, b.seen_tip FROM features f
         JOIN branches b ON b.name = f.branch AND b.feature_id = f.id AND b.task_id IS NULL
         WHERE f.state IN ('open', 'ready') AND b.seen_tip IS NOT NULL
         ORDER BY f.id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn tasks_landed(conn: &Connection) -> Result<std::collections::BTreeMap<String, bool>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, max(landing_state = 'landed') FROM branches
         WHERE task_id IS NOT NULL GROUP BY task_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn branch(conn: &Connection, name: &str) -> Result<Option<BranchRow>> {
    let row = |r: &Row<'_>| {
        Ok(BranchRow {
            name: r.get(0)?,
            feature_id: r.get(1)?,
            task_id: r.get(2)?,
            landing_state: r.get(3)?,
            owner: r.get(4)?,
            due_at: r.get(5)?,
            queue_seq: r.get(6)?,
            repair_attempts: r.get(7)?,
            rescue_ref: r.get(8)?,
            seen_tip: r.get(9)?,
            seen_remote_tip: r.get(10)?,
            orphan: r.get(11)?,
        })
    };
    Ok(conn
        .query_row(
            "SELECT name, feature_id, task_id, landing_state, owner, due_at, queue_seq,
                repair_attempts, rescue_ref, seen_tip, seen_remote_tip, orphan
             FROM branches WHERE name = ?1",
            [name],
            row,
        )
        .optional()?)
}

pub fn landing_states_of_task(conn: &Connection, task_id: &str) -> Result<Vec<Option<String>>> {
    let mut stmt =
        conn.prepare("SELECT landing_state FROM branches WHERE task_id = ?1 ORDER BY name")?;
    let states = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(states.collect::<rusqlite::Result<_>>()?)
}

pub fn task_branch_landing_states<'a>(
    conn: &Connection,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, Option<String>>> {
    let names: Vec<&str> = names.into_iter().collect();
    let sql = format!(
        "SELECT name, landing_state FROM branches
         WHERE task_id IS NOT NULL AND name IN ({})",
        crate::taskledger::sql_words::marks(1, names.len())
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(names), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn branch_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM branches ORDER BY name")?;
    let names = stmt.query_map([], |r| r.get(0))?;
    Ok(names.collect::<rusqlite::Result<_>>()?)
}

pub fn task_branches_in(conn: &Connection, state: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT name, task_id FROM branches
         WHERE landing_state = ?1 AND task_id IS NOT NULL ORDER BY name",
    )?;
    let rows = stmt.query_map([state], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn bundle_jobs_of_branch(
    conn: &Connection,
    branch: &str,
    state: &str,
) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.job_id FROM bundles b JOIN bundle_members m ON m.bundle_id = b.id
         WHERE m.branch = ?1 AND b.state = ?2 AND b.job_id IS NOT NULL
         ORDER BY b.id DESC",
    )?;
    let rows = stmt.query_map([branch, state], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn bundle(conn: &Connection, id: i64) -> Result<Option<BundleRow>> {
    let row = |r: &Row<'_>| {
        Ok(BundleRow {
            id: r.get(0)?,
            feature_id: r.get(1)?,
            base_tip: r.get(2)?,
            candidate_ref: r.get(3)?,
            candidate_tip: r.get(4)?,
            parent_id: r.get(5)?,
            state: r.get(6)?,
            job_id: r.get(7)?,
            due_at: r.get(8)?,
            result: r.get(9)?,
            at: r.get(10)?,
            members: Vec::new(),
        })
    };
    let Some(mut found) = conn
        .query_row(
            "SELECT id, feature_id, base_tip, candidate_ref, candidate_tip, parent_id, state,
                job_id, due_at, result, at
             FROM bundles WHERE id = ?1",
            [id],
            row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stmt =
        conn.prepare("SELECT branch FROM bundle_members WHERE bundle_id = ?1 ORDER BY position")?;
    found.members = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(found))
}

fn gate_run_row(r: &Row<'_>) -> rusqlite::Result<GateRunRow> {
    Ok(GateRunRow {
        id: r.get(0)?,
        feature_id: r.get(1)?,
        gate: r.get(2)?,
        tip: r.get(3)?,
        base_tip: r.get(4)?,
        job_id: r.get(5)?,
        result: r.get(6)?,
        at: r.get(7)?,
    })
}

pub fn gate_run(conn: &Connection, id: i64) -> Result<Option<GateRunRow>> {
    Ok(conn
        .query_row(
            "SELECT id, feature_id, gate, tip, base_tip, job_id, result, at
             FROM gate_runs WHERE id = ?1",
            [id],
            gate_run_row,
        )
        .optional()?)
}

pub fn gate_run_of(
    conn: &Connection,
    feature_id: &str,
    gate: &str,
    tip: &str,
    base_tip: &str,
) -> Result<Option<GateRunRow>> {
    Ok(conn
        .query_row(
            "SELECT id, feature_id, gate, tip, base_tip, job_id, result, at
             FROM gate_runs WHERE feature_id = ?1 AND gate = ?2 AND tip = ?3 AND base_tip = ?4
             ORDER BY id DESC LIMIT 1",
            params![feature_id, gate, tip, base_tip],
            gate_run_row,
        )
        .optional()?)
}

pub fn set_gate_result(
    tx: &Connection,
    guard: Option<Generation>,
    id: i64,
    job_id: i64,
    result: &str,
    cause: Cause<'_>,
) -> Result<()> {
    check_writable(tx, guard)?;
    let feature: Option<String> = tx
        .query_row(
            "UPDATE gate_runs SET result = ?2, job_id = ?3 WHERE id = ?1 RETURNING feature_id",
            params![id, result, job_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(feature) = feature else {
        bail!("no gate run {id} in the ledger");
    };
    let data = json!({ "gate_run": id, "job": job_id, "result": result });
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Feature,
        &feature,
        cause,
        &data,
    )?;
    Ok(())
}

pub fn remote_tip(conn: &Connection, repo: &str, branch: &str) -> Result<Option<RemoteTipRow>> {
    let row = |r: &Row<'_>| {
        Ok(RemoteTipRow {
            repo: r.get(0)?,
            branch: r.get(1)?,
            tip: r.get(2)?,
            fetched_at: r.get(3)?,
            stale_since: r.get(4)?,
        })
    };
    Ok(conn
        .query_row(
            "SELECT repo, branch, tip, fetched_at, stale_since
             FROM remote_tips WHERE repo = ?1 AND branch = ?2",
            [repo, branch],
            row,
        )
        .optional()?)
}
