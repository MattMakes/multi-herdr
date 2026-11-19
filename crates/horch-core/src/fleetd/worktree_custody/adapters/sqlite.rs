//! The rows of the `worktrees` table: the store of worktree custody.
//!
//! A new row and its event go through `created()` of the 1 door. A state
//! change is not a function of this store: the use cases call
//! `transition()` (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One
//! door"). `status` keeps the words of today (`docs/tickets/fleetd-p3/MAP.md`,
//! Q3).

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::taskledger::model::WorktreeStatus;
use crate::taskledger::util;

/// `worktrees.state`: the 8 states of the worktree table of the design
/// ("Worktrees: leased, reconciled, never lost").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorktreeState {
    Reserved,
    Created,
    Active,
    Landing,
    Landed,
    Removed,
    Orphaned,
    Rescued,
}

impl WorktreeState {
    /// Every state, in the order of the design table.
    pub const ALL: [WorktreeState; 8] = [
        WorktreeState::Reserved,
        WorktreeState::Created,
        WorktreeState::Active,
        WorktreeState::Landing,
        WorktreeState::Landed,
        WorktreeState::Removed,
        WorktreeState::Orphaned,
        WorktreeState::Rescued,
    ];

    /// The stored word.
    pub fn as_str(self) -> &'static str {
        match self {
            WorktreeState::Reserved => "reserved",
            WorktreeState::Created => "created",
            WorktreeState::Active => "active",
            WorktreeState::Landing => "landing",
            WorktreeState::Landed => "landed",
            WorktreeState::Removed => "removed",
            WorktreeState::Orphaned => "orphaned",
            WorktreeState::Rescued => "rescued",
        }
    }

    /// The state that a stored word names.
    pub fn parse(text: &str) -> Result<WorktreeState> {
        match WorktreeState::ALL.into_iter().find(|s| s.as_str() == text) {
            Some(state) => Ok(state),
            None => bail!("'{text}' is not a worktree state"),
        }
    }
}

impl std::fmt::Display for WorktreeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `worktree_modes` row of a worktree: its run, its name and its
/// scratch flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeMode {
    pub run_id: String,
    pub name: String,
    pub scratch: bool,
}

/// 1 row of `worktrees`, with its mode row, as custody reads it.
/// `base_commit` is the design's `base_sha` and `bytes_on_disk` its
/// `disk_bytes` (MAP.md of fleetd-p3, Q2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRow {
    pub worktree_id: String,
    pub task_id: Option<String>,
    pub worker_id: Option<String>,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub base_commit: Option<String>,
    pub status: WorktreeStatus,
    pub state: WorktreeState,
    pub owner_attempt: Option<String>,
    pub head_sha: Option<String>,
    pub dirty: bool,
    pub bytes_on_disk: Option<i64>,
    pub rescue_ref: Option<String>,
    pub seen_at: Option<String>,
    pub mode: Option<WorktreeMode>,
}

/// A new row: [`insert_reserved`] writes it in `reserved`, and
/// [`insert_orphan`] in `orphaned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorktree {
    pub worktree_id: String,
    pub task_id: Option<String>,
    pub worker_id: Option<String>,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub base_commit: Option<String>,
    pub status: WorktreeStatus,
    pub mode: Option<WorktreeMode>,
}

/// The columns of [`WorktreeRow`], in the order of [`row`].
const COLUMNS: &str = "w.worktree_id, w.task_id, w.worker_id, w.repo_root, w.path, w.branch,
    w.base, w.base_commit, w.status, w.state, w.owner_attempt, w.head_sha, w.dirty,
    w.bytes_on_disk, w.rescue_ref, w.seen_at, m.run_id, m.name, m.scratch";

fn row(r: &Row<'_>) -> rusqlite::Result<(WorktreeRow, String, String)> {
    let run_id: Option<String> = r.get(16)?;
    let mode = match run_id {
        Some(run_id) => Some(WorktreeMode {
            run_id,
            name: r.get(17)?,
            scratch: r.get(18)?,
        }),
        None => None,
    };
    let wt = WorktreeRow {
        worktree_id: r.get(0)?,
        task_id: r.get(1)?,
        worker_id: r.get(2)?,
        repo_root: r.get(3)?,
        path: r.get(4)?,
        branch: r.get(5)?,
        base: r.get(6)?,
        base_commit: r.get(7)?,
        // Parsed after the read: a word of the row is not a SQLite error.
        status: WorktreeStatus::Open,
        state: WorktreeState::Reserved,
        owner_attempt: r.get(10)?,
        head_sha: r.get(11)?,
        dirty: r.get(12)?,
        bytes_on_disk: r.get(13)?,
        rescue_ref: r.get(14)?,
        seen_at: r.get(15)?,
        mode,
    };
    Ok((wt, r.get(8)?, r.get(9)?))
}

fn parsed((mut wt, status, state): (WorktreeRow, String, String)) -> Result<WorktreeRow> {
    wt.status = WorktreeStatus::parse(&status)?;
    wt.state = WorktreeState::parse(&state)?;
    Ok(wt)
}

fn one(conn: &Connection, sql: &str, key: &str) -> Result<Option<WorktreeRow>> {
    conn.query_row(sql, [key], row)
        .optional()?
        .map(parsed)
        .transpose()
}

/// The row `id`; `None` when no row has that id.
pub fn get(conn: &Connection, id: &str) -> Result<Option<WorktreeRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM worktrees w
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE w.worktree_id = ?1"
    );
    one(conn, &sql, id)
}

/// The tip that the newest row of the branch `branch` recorded, also a
/// `removed` row: its landed commit, else its head. `None` when no row
/// records one.
pub fn recorded_tip(conn: &Connection, branch: &str) -> Result<Option<String>> {
    let tip: Option<Option<String>> = conn
        .query_row(
            "SELECT COALESCE(landed_commit, head_sha) FROM worktrees WHERE branch = ?1
             ORDER BY rowid DESC LIMIT 1",
            [branch],
            |r| r.get(0),
        )
        .optional()?;
    Ok(tip.flatten())
}

/// The row of `task_id` that is not `removed`; the newest when there are
/// more.
pub fn of_task(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM worktrees w
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE w.task_id = ?1 AND w.state <> 'removed'
         ORDER BY w.rowid DESC LIMIT 1"
    );
    one(conn, &sql, task_id)
}

/// The row of the folder `path` that is not `removed`; `None` when no such
/// row has that path. A `removed` row is history, read by its id.
pub fn by_path(conn: &Connection, path: &str) -> Result<Option<WorktreeRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM worktrees w
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE w.path = ?1 AND w.state <> 'removed'"
    );
    one(conn, &sql, path)
}

/// Each row in 1 of `states`, oldest first.
pub fn in_states(conn: &Connection, states: &[WorktreeState]) -> Result<Vec<WorktreeRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM worktrees w
         LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
         WHERE instr(?1, ',' || w.state || ',') > 0
         ORDER BY w.rowid"
    );
    // The words as 1 list, `,reserved,orphaned,`: no state word has a comma.
    let words: String = states.iter().map(|s| format!(",{s}")).collect::<String>() + ",";
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([words], row)?;
    rows.map(|r| parsed(r?)).collect()
}

/// Insert `new` in `state`, with its mode row, then record it through
/// `created()` of the door.
fn insert(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewWorktree,
    state: WorktreeState,
    cause: Cause<'_>,
) -> Result<()> {
    // `created()` checks this too, but after the insert: a refused call
    // would leave a row with no event (review F1).
    check_writable(tx, guard)?;
    tx.execute(
        "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
            base_commit, status, state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            new.worktree_id,
            new.task_id,
            new.worker_id,
            new.repo_root,
            new.path,
            new.branch,
            new.base,
            new.base_commit,
            new.status.as_str(),
            state.as_str(),
            util::now(),
        ],
    )?;
    if let Some(mode) = &new.mode {
        tx.execute(
            "INSERT INTO worktree_modes (worktree_id, run_id, name, scratch)
             VALUES (?1, ?2, ?3, ?4)",
            params![new.worktree_id, mode.run_id, mode.name, mode.scratch],
        )?;
    }
    created(tx, guard, Entity::Worktree, &new.worktree_id, cause)?;
    Ok(())
}

/// Insert `new` in `reserved`: a tree that fleetd makes. 1 event records
/// it, with `cause`.
pub fn insert_reserved(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewWorktree,
    cause: Cause<'_>,
) -> Result<()> {
    insert(tx, guard, new, WorktreeState::Reserved, cause)
}

/// Insert `new` in `orphaned`: a folder that the reconciler adopts. 1
/// event records it, with `cause`.
pub fn insert_orphan(
    tx: &Connection,
    guard: Option<Generation>,
    new: &NewWorktree,
    cause: Cause<'_>,
) -> Result<()> {
    insert(tx, guard, new, WorktreeState::Orphaned, cause)
}

/// An error when `changed` is not 1 row.
fn updated(changed: usize, id: &str) -> Result<()> {
    if changed != 1 {
        bail!("no worktree {id} in the ledger");
    }
    Ok(())
}

/// The facts that git and the disk give of the row `id`. No state
/// changes, so no event.
pub fn set_facts(
    tx: &Connection,
    id: &str,
    head_sha: &str,
    dirty: bool,
    bytes: Option<i64>,
    seen_at: &str,
) -> Result<()> {
    let changed = tx.execute(
        "UPDATE worktrees SET head_sha = ?2, dirty = ?3, bytes_on_disk = ?4, seen_at = ?5
         WHERE worktree_id = ?1",
        params![id, head_sha, dirty, bytes, seen_at],
    )?;
    updated(changed, id)
}

/// The attempt that holds the row `id`; `None` when no attempt holds it.
pub fn set_owner(tx: &Connection, id: &str, attempt: Option<&str>) -> Result<()> {
    let changed = tx.execute(
        "UPDATE worktrees SET owner_attempt = ?2 WHERE worktree_id = ?1",
        params![id, attempt],
    )?;
    updated(changed, id)
}

/// The verified rescue ref of the row `id`.
pub fn set_rescue_ref(tx: &Connection, id: &str, rescue_ref: &str) -> Result<()> {
    let changed = tx.execute(
        "UPDATE worktrees SET rescue_ref = ?2 WHERE worktree_id = ?1",
        params![id, rescue_ref],
    )?;
    updated(changed, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::application::ledger::transition;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    struct TestLedger {
        _dir: tempfile::TempDir,
        store: Store,
        run_id: String,
        task_id: String,
        worker_id: String,
    }

    fn ledger() -> TestLedger {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir.path(), &new)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        let task = NewTask::work("t1", "t1.md", "dev");
        let task_id = store
            .immediate(|tx| tasks::add(tx, &run, &task, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let worker = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let worker_id = store.immediate(|tx| workers::insert(tx, &worker)).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state) VALUES ('A1', ?1, ?2, 'live')",
                [&task_id, &worker_id],
            )
            .unwrap();
        TestLedger {
            _dir: dir,
            store,
            run_id: run.run_id,
            task_id,
            worker_id,
        }
    }

    fn cause(text: &str) -> Cause<'_> {
        Cause {
            actor: Actor::Horch,
            text,
        }
    }

    fn new_row(l: &TestLedger, id: &str) -> NewWorktree {
        NewWorktree {
            worktree_id: id.into(),
            task_id: Some(l.task_id.clone()),
            worker_id: Some(l.worker_id.clone()),
            repo_root: "/r".into(),
            path: format!("/r/.worktrees/{id}"),
            branch: format!("fleet/{id}"),
            base: "main".into(),
            base_commit: Some("b1".into()),
            status: WorktreeStatus::Open,
            mode: Some(WorktreeMode {
                run_id: l.run_id.clone(),
                name: id.into(),
                scratch: false,
            }),
        }
    }

    fn move_to(l: &mut TestLedger, id: &str, to: WorktreeState) {
        l.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Worktree,
                    id,
                    to.as_str(),
                    cause("t"),
                    &NeedsYouList,
                )
                .map_err(anyhow::Error::from)
            })
            .unwrap();
    }

    #[test]
    fn store_reads_back_each_column_it_writes() {
        let mut l = ledger();
        let new = new_row(&l, "wt1");
        l.store
            .immediate(|tx| insert_reserved(tx, None, &new, cause("claim")))
            .unwrap();
        l.store
            .immediate(|tx| {
                set_facts(tx, "wt1", "h1", true, Some(42), "seen")?;
                set_owner(tx, "wt1", Some("A1"))?;
                set_rescue_ref(tx, "wt1", "refs/horch/rescue/wt1/1")
            })
            .unwrap();
        let expected = WorktreeRow {
            worktree_id: "wt1".into(),
            task_id: new.task_id.clone(),
            worker_id: new.worker_id.clone(),
            repo_root: "/r".into(),
            path: "/r/.worktrees/wt1".into(),
            branch: "fleet/wt1".into(),
            base: "main".into(),
            base_commit: Some("b1".into()),
            status: WorktreeStatus::Open,
            state: WorktreeState::Reserved,
            owner_attempt: Some("A1".into()),
            head_sha: Some("h1".into()),
            dirty: true,
            bytes_on_disk: Some(42),
            rescue_ref: Some("refs/horch/rescue/wt1/1".into()),
            seen_at: Some("seen".into()),
            mode: new.mode.clone(),
        };
        let conn = l.store.conn();
        assert_eq!(get(conn, "wt1").unwrap(), Some(expected.clone()));
        assert_eq!(
            by_path(conn, "/r/.worktrees/wt1").unwrap(),
            Some(expected.clone())
        );
        assert_eq!(of_task(conn, &l.task_id).unwrap(), Some(expected.clone()));
        assert_eq!(get(conn, "nope").unwrap(), None);
        assert_eq!(by_path(conn, "/nope").unwrap(), None);

        let mut orphan = new_row(&l, "wt2");
        orphan.task_id = None;
        orphan.worker_id = None;
        orphan.base_commit = None;
        orphan.mode = None;
        l.store
            .immediate(|tx| insert_orphan(tx, None, &orphan, cause("adopt")))
            .unwrap();
        l.store.immediate(|tx| set_owner(tx, "wt1", None)).unwrap();
        let conn = l.store.conn();
        let wt2 = get(conn, "wt2").unwrap().unwrap();
        assert_eq!(
            (wt2.state, wt2.task_id, wt2.mode, wt2.dirty, wt2.head_sha),
            (WorktreeState::Orphaned, None, None, false, None)
        );
        assert_eq!(get(conn, "wt1").unwrap().unwrap().owner_attempt, None);

        let ids = |states: &[WorktreeState]| -> Vec<String> {
            in_states(conn, states)
                .unwrap()
                .into_iter()
                .map(|w| w.worktree_id)
                .collect()
        };
        assert_eq!(ids(&[WorktreeState::Reserved]), ["wt1"]);
        assert_eq!(
            ids(&[WorktreeState::Orphaned, WorktreeState::Reserved]),
            ["wt1", "wt2"]
        );
        assert_eq!(ids(&[WorktreeState::Active]), Vec::<String>::new());

        let created_events: Vec<(String, Option<String>, String, String)> = conn
            .prepare(
                "SELECT entity_id, from_state, to_state, cause FROM events
                 WHERE kind = 'transition' AND entity = 'worktree' ORDER BY event_id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            created_events,
            [
                ("wt1".into(), None, "reserved".into(), "claim".into()),
                ("wt2".into(), None, "orphaned".into(), "adopt".into()),
            ],
            "1 event for each new row, and none for the facts"
        );
    }

    #[test]
    fn by_path_gives_the_live_row_and_not_a_removed_row_of_its_path() {
        let mut l = ledger();
        let old = new_row(&l, "wt1");
        l.store
            .immediate(|tx| insert_orphan(tx, None, &old, cause("adopt")))
            .unwrap();
        move_to(&mut l, "wt1", WorktreeState::Rescued);
        move_to(&mut l, "wt1", WorktreeState::Removed);
        assert_eq!(by_path(l.store.conn(), &old.path).unwrap(), None);
        // A folder at the path of the removed row is a new row.
        let new = NewWorktree {
            worktree_id: "wt2".into(),
            ..old.clone()
        };
        l.store
            .immediate(|tx| insert_orphan(tx, None, &new, cause("adopt")))
            .unwrap();
        let got = by_path(l.store.conn(), &old.path).unwrap().unwrap();
        assert_eq!(got.worktree_id, "wt2");
        assert_eq!(
            get(l.store.conn(), "wt1").unwrap().unwrap().state,
            WorktreeState::Removed
        );
    }

    #[test]
    fn of_task_skips_a_removed_row() {
        let mut l = ledger();
        let old = new_row(&l, "wt1");
        l.store
            .immediate(|tx| insert_orphan(tx, None, &old, cause("adopt")))
            .unwrap();
        move_to(&mut l, "wt1", WorktreeState::Rescued);
        move_to(&mut l, "wt1", WorktreeState::Removed);
        assert_eq!(of_task(l.store.conn(), &l.task_id).unwrap(), None);
        let new = new_row(&l, "wt2");
        l.store
            .immediate(|tx| insert_reserved(tx, None, &new, cause("claim")))
            .unwrap();
        let found = of_task(l.store.conn(), &l.task_id).unwrap().unwrap();
        assert_eq!(found.worktree_id, "wt2");
    }

    fn count(l: &TestLedger, sql: &str) -> i64 {
        l.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_refused_insert_writes_no_row_and_no_event() {
        let mut l = ledger();
        let new = new_row(&l, "wt1");
        assert!(insert_reserved(l.store.conn(), None, &new, cause("claim")).is_err());
        assert!(insert_orphan(l.store.conn(), None, &new, cause("adopt")).is_err());
        let lost = l
            .store
            .immediate(|tx| {
                Ok(insert_reserved(
                    tx,
                    Some(Generation(7)),
                    &new,
                    cause("claim"),
                ))
            })
            .unwrap();
        assert!(lost.is_err());
        assert_eq!(count(&l, "SELECT count(*) FROM worktrees"), 0);
        assert_eq!(count(&l, "SELECT count(*) FROM worktree_modes"), 0);
        assert_eq!(
            count(&l, "SELECT count(*) FROM events WHERE entity = 'worktree'"),
            0
        );
    }

    #[test]
    fn a_fact_of_a_missing_row_is_an_error() {
        let mut l = ledger();
        let out = l
            .store
            .immediate(|tx| set_facts(tx, "nope", "h", false, None, "s"));
        assert!(out.is_err());
        let out = l.store.immediate(|tx| set_owner(tx, "nope", None));
        assert!(out.is_err());
        let out = l.store.immediate(|tx| set_rescue_ref(tx, "nope", "r"));
        assert!(out.is_err());
    }

    #[test]
    fn each_state_word_parses_back_to_its_state() {
        for state in WorktreeState::ALL {
            assert_eq!(WorktreeState::parse(state.as_str()).unwrap(), state);
        }
        assert!(WorktreeState::parse("cleaned").is_err());
    }
}
