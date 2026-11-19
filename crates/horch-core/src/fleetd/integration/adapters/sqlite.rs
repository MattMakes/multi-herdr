//! The rows of the integration tables: `features`, `branches`, `bundles`,
//! `bundle_members`, `gate_runs` and `remote_tips`.
//!
//! No other code writes these tables. A new feature, task branch or bundle
//! and its event go through `created()` of the 1 door. A state change is
//! not a function of this store: the use cases call `transition()`
//! (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One door"). Each write
//! of a fact that is not a state records 1 event with its entity (OR-2).
//! A feature branch row has no landing state, so it is not a task branch:
//! the event of its row is an event of its feature.

use anyhow::{bail, Result};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, record, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::taskledger::model::EventKind;
use crate::taskledger::util;

/// The first state of a feature and of a task branch, and of a bundle.
const FEATURE_FIRST: &str = "open";
const BRANCH_FIRST: &str = "building";
const BUNDLE_FIRST: &str = "building";

/// 1 row of `features`.
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
    /// Fleet clock seconds.
    pub push_not_before: Option<i64>,
    pub pr_state: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 1 row of `branches`: a task branch, or the branch of a feature
/// (`task_id` and `landing_state` are null).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRow {
    pub name: String,
    pub feature_id: String,
    pub task_id: Option<String>,
    pub landing_state: Option<String>,
    pub owner: String,
    /// Fleet clock seconds.
    pub due_at: i64,
    pub queue_seq: Option<i64>,
    pub repair_attempts: i64,
    pub rescue_ref: Option<String>,
    pub seen_tip: Option<String>,
    pub seen_remote_tip: Option<String>,
    pub orphan: bool,
}

/// 1 row of `bundles`, with its members in queue order.
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
    /// Fleet clock seconds.
    pub due_at: i64,
    pub result: Option<String>,
    pub at: String,
    pub members: Vec<String>,
}

/// 1 row of `gate_runs`.
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

/// 1 row of `remote_tips`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTipRow {
    pub repo: String,
    pub branch: String,
    pub tip: String,
    pub fetched_at: String,
    pub stale_since: Option<String>,
}

/// A new feature. It starts `open`.
#[derive(Debug, Clone, Copy)]
pub struct NewFeature<'a> {
    pub id: &'a str,
    pub name: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub primary_branch: &'a str,
}

/// A new branch row. A row with a task is a task branch, and it starts
/// `building`. A row with no task is the branch of its feature.
#[derive(Debug, Clone, Copy)]
pub struct NewBranch<'a> {
    pub name: &'a str,
    pub feature_id: &'a str,
    pub task_id: Option<&'a str>,
    pub owner: &'a str,
    /// Fleet clock seconds.
    pub due_at: i64,
}

/// A new bundle of 1 or more task branches, in queue order. It starts
/// `building`.
#[derive(Debug, Clone, Copy)]
pub struct NewBundle<'a> {
    pub feature_id: &'a str,
    pub base_tip: &'a str,
    pub parent_id: Option<i64>,
    pub members: &'a [&'a str],
    /// Fleet clock seconds.
    pub due_at: i64,
}

/// A new run of a gate: `fast` or `full`.
#[derive(Debug, Clone, Copy)]
pub struct NewGateRun<'a> {
    pub feature_id: &'a str,
    pub gate: &'a str,
    pub tip: &'a str,
    pub base_tip: &'a str,
    pub job_id: i64,
}

/// A fact of a branch row that is not its state.
#[derive(Debug, Clone, Copy)]
pub enum BranchFact<'a> {
    SeenTip(&'a str),
    SeenRemoteTip(&'a str),
    QueueSeq(Option<i64>),
    RepairAttempts(i64),
    /// Fleet clock seconds.
    DueAt(i64),
    /// The last rescue ref of the branch (ticket 24).
    RescueRef(&'a str),
    /// The owner of its landing state (ticket 25).
    Owner(&'a str),
    /// Found in git with no row, and adopted (I11, ticket 44).
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

/// A fact of a bundle that is not its state.
#[derive(Debug, Clone, Copy)]
pub enum BundleFact<'a> {
    /// `None` when the deletion of the ref is verified.
    CandidateRef(Option<&'a str>),
    CandidateTip(&'a str),
    /// The build job, then the fast gate job.
    JobId(i64),
    /// Fleet clock seconds.
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

/// A fact of a feature that is not its state.
#[derive(Debug, Clone, Copy)]
pub enum FeatureFact<'a> {
    /// `ok` or `failed`.
    PushState(&'a str),
    PushFailures(i64),
    /// Fleet clock seconds.
    PushNotBefore(Option<i64>),
    /// The pull request or the compare URL (ticket 39).
    Pr(&'a str),
    /// `none`, `open` or `closed`.
    PrState(&'a str),
    /// The kebab-case name, checked by fleetd (ticket 15 of fleetd-p6).
    Name(&'a str),
    /// The feature branch, written before git has it (ticket 15 of fleetd-p6).
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

/// The `data_json` of the event of a fact: its column and value, and the
/// branch of a branch row.
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

// ─── inserts ────────────────────────────────────────────────────────────

/// Insert the `open` feature `new`, with its event, in `tx`.
pub fn insert_feature(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewFeature<'_>,
    cause: Cause<'_>,
) -> Result<()> {
    // `created()` checks this too, but after the insert.
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

/// Insert the branch row `new`, with its event, in `tx`: a task branch in
/// `building`, or the branch of a feature with no landing state.
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

/// Insert the `building` bundle `new` and its members, with its event, in
/// `tx`. The bundle row, its members and its event are written together or
/// not at all, so the caller's transaction goes on after an error: a member
/// with no row, or a member of another bundle that is not ended (I12).
/// Gives the id of the bundle.
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

/// Insert the gate run `new`, with 1 event of its feature, in `tx`. Gives
/// its id.
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

/// Write the fetched tip `tip` of `branch` in `repo`, in `tx`: a passed
/// fetch, so the row is not stale. A remote tip is not a row of an entity,
/// and each fetch period writes it, so it has no event.
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

/// Mark the remote tips of `repo` stale, in `tx`: a failed fetch sets
/// `stale_since` of each row where it is not set yet, so it keeps the time
/// of the first failure. `stale` false clears it: a passed fetch.
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

// ─── facts ──────────────────────────────────────────────────────────────

/// Write `fact` of the branch row `name`, with 1 event, in `tx`. The event
/// of a task branch is an event of the task branch; the event of a feature
/// branch is an event of its feature.
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

/// Write `fact` of the bundle `id`, with 1 event, in `tx`.
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

/// Write `fact` of the feature `id`, with 1 event, in `tx`.
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

// ─── reads ──────────────────────────────────────────────────────────────

/// The feature `id`.
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

/// The feature with the name `name`.
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

/// Whether a feature has the name `name` or the branch `branch`, or a
/// branch row has the name `branch`.
pub fn name_taken(conn: &Connection, name: &str, branch: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM features WHERE name = ?1 OR branch = ?2)
             OR EXISTS (SELECT 1 FROM branches WHERE name = ?2)",
        [name, branch],
        |r| r.get(0),
    )?)
}

/// The ids of the features, by id; with `live`, only the `open` and
/// `ready` features.
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

/// The ids of the bundles that a pass follows: each bundle that is not
/// ended, and each ended bundle whose candidate ref is not verified as
/// deleted, by id.
pub fn bundles_to_follow(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM bundles
         WHERE state IN ('building', 'gating', 'passed') OR candidate_ref IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Each `dropped` bundle with no recorded candidate and with a job, as
/// (bundle id, job id), by bundle id. Its build job can have written the
/// candidate ref after the drop.
pub fn dropped_without_candidate(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT id, job_id FROM bundles
         WHERE state = 'dropped' AND candidate_tip IS NULL AND job_id IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The branch of each `open` or `ready` feature that has its branch row,
/// with the `seen_tip` of that row, as (feature id, branch, seen tip), by
/// feature id.
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

/// Each task with a task branch row, and whether 1 of its task branches
/// is `landed` (I2).
pub fn tasks_landed(conn: &Connection) -> Result<std::collections::BTreeMap<String, bool>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, max(landing_state = 'landed') FROM branches
         WHERE task_id IS NOT NULL GROUP BY task_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The branch row `name`.
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

/// The landing state of each branch row of the task `task_id`.
pub fn landing_states_of_task(conn: &Connection, task_id: &str) -> Result<Vec<Option<String>>> {
    let mut stmt =
        conn.prepare("SELECT landing_state FROM branches WHERE task_id = ?1 ORDER BY name")?;
    let states = stmt.query_map([task_id], |r| r.get(0))?;
    Ok(states.collect::<rusqlite::Result<_>>()?)
}

/// The name of each branch row, sorted.
pub fn branch_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM branches ORDER BY name")?;
    let names = stmt.query_map([], |r| r.get(0))?;
    Ok(names.collect::<rusqlite::Result<_>>()?)
}

/// The name and the task of each branch row of a task in `state`, sorted
/// by name (the repair attempts of ticket 33).
pub fn task_branches_in(conn: &Connection, state: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT name, task_id FROM branches
         WHERE landing_state = ?1 AND task_id IS NOT NULL ORDER BY name",
    )?;
    let rows = stmt.query_map([state], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The id and the job of each bundle in `state` that holds the branch
/// `branch`, newest first (the conflict of a resolver attempt, ticket 33).
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

/// The bundle `id`, with its members in queue order.
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

/// The gate run `id`.
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

/// The newest gate run of `gate` of the feature `feature_id` on `tip` over
/// `base_tip`.
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

/// Write the result `result` (`pass` or `fail`) of the gate run `id` and
/// its last job run `job_id`, with 1 event of its feature, in `tx`.
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

/// The fetched tip of `branch` in `repo`.
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
