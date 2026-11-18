//! The 1 door: the SQLite writes of the state of each entity.
//!
//! `application::ledger::transition()` is the only caller of
//! [`set_state`]. Each state change and its event go through it
//! (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One door").
//! [`drop_reserved`] is the only delete of the door.

use std::cell::RefCell;

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::domain::ledger::{refused_cause, Entity, Generation};
use crate::taskledger::model::{Actor, EventKind, TaskStatus};
use crate::taskledger::util;

thread_local! {
    /// The connections of this thread, by address, that have an open
    /// transaction with a door write.
    static OPEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Whether this thread has an open transaction in which the door wrote a
/// state or an event. No effect outside the ledger starts while it is true.
pub fn in_door_transaction() -> bool {
    OPEN.with(|o| !o.borrow().is_empty())
}

/// Mark the transaction of `tx` open for [`in_door_transaction`] until it
/// commits or rolls back. The hooks of its connection clear the mark.
/// Call it only after a write: SQLite calls the commit hook only for a
/// transaction that wrote (review F1).
fn mark(tx: &Connection) {
    let key = tx as *const Connection as usize;
    let new = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let new = !o.contains(&key);
        if new {
            o.push(key);
        }
        new
    });
    if new {
        tx.commit_hook(Some(move || {
            leave(key);
            false
        }));
        tx.rollback_hook(Some(move || leave(key)));
    }
}

fn leave(key: usize) {
    OPEN.with(|o| o.borrow_mut().retain(|k| *k != key));
}

/// The generation of the `fleetd` row; `None` before the first start of
/// fleetd.
pub fn generation(conn: &Connection) -> Result<Option<Generation>> {
    let g = conn
        .query_row("SELECT generation FROM fleetd WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(g.map(Generation))
}

/// The table of `entity`: its name, its key column and its state column.
fn table(entity: Entity) -> (&'static str, &'static str, &'static str) {
    match entity {
        Entity::Task => ("tasks", "task_id", "status"),
        Entity::Worker => ("workers", "worker_id", "status"),
        Entity::Attempt => ("attempts", "id", "state"),
        Entity::Job => ("jobs", "id", "state"),
        Entity::NeedsYou => ("needs_you", "key", "state"),
        Entity::Worktree => ("worktrees", "worktree_id", "state"),
        Entity::TaskBranch => ("branches", "name", "landing_state"),
        Entity::Bundle => ("bundles", "id", "state"),
        Entity::Feature => ("features", "id", "state"),
        Entity::Idea => ("ideas", "id", "state"),
    }
}

/// The state of the row `id` of `entity`; `None` when no row has that id,
/// or when its state is null: a feature branch row has no landing state,
/// so it is not a task branch.
pub fn state_of(conn: &Connection, entity: Entity, id: &str) -> Result<Option<String>> {
    let (name, key, state) = table(entity);
    let sql = format!("SELECT {state} FROM {name} WHERE {key} = ?1");
    let found: Option<Option<String>> = conn.query_row(&sql, [id], |r| r.get(0)).optional()?;
    Ok(found.flatten())
}

/// Write `to` in the state column of the row `id` of `entity`. Only
/// `transition()` calls this.
pub fn set_state(conn: &Connection, entity: Entity, id: &str, to: &str) -> Result<()> {
    let (name, key, state) = table(entity);
    let sql = format!("UPDATE {name} SET {state} = ?2 WHERE {key} = ?1");
    conn.execute(&sql, params![id, to])?;
    Ok(())
}

/// The state that the row `id` of `entity` left on its last move to the
/// state `into`: the `from_state` of the newest event of an allowed move
/// to `into`. `None` when no such event exists. A refused move writes an
/// event with the cause [`refused_cause`], so it is not a move.
pub fn left_state(
    conn: &Connection,
    entity: Entity,
    id: &str,
    into: &str,
) -> Result<Option<String>> {
    let refused = format!("{}%", refused_cause(""));
    let found: Option<Option<String>> = conn
        .query_row(
            "SELECT from_state FROM events
             WHERE entity = ?1 AND entity_id = ?2 AND kind = ?3 AND to_state = ?4
                AND cause NOT LIKE ?5
             ORDER BY event_id DESC LIMIT 1",
            params![
                entity.as_str(),
                id,
                EventKind::Transition.as_str(),
                into,
                refused
            ],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.flatten())
}

/// Delete the lease of task `task_id`, which moves to a state that holds
/// no lease. Only `transition()` calls this.
pub fn delete_lease(conn: &Connection, task_id: &str) -> Result<()> {
    conn.execute("DELETE FROM leases WHERE task_id = ?1", [task_id])?;
    Ok(())
}

/// The run, task and worker of the row `id` of `entity`, for the columns
/// of today that `horch ledger show` reads.
fn about(
    conn: &Connection,
    entity: Entity,
    id: &str,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let sql = match entity {
        Entity::Task => "SELECT run_id, task_id, current_worker_id FROM tasks WHERE task_id = ?1",
        Entity::Worker => "SELECT run_id, task_id, worker_id FROM workers WHERE worker_id = ?1",
        Entity::Attempt => {
            "SELECT w.run_id, a.task_id, a.worker_id FROM attempts a
             JOIN workers w ON w.worker_id = a.worker_id WHERE a.id = ?1"
        }
        // A scratch row has no task: its run is the run of its mode row.
        Entity::Worktree => {
            "SELECT COALESCE(t.run_id, m.run_id), w.task_id, w.worker_id FROM worktrees w
             LEFT JOIN tasks t ON t.task_id = w.task_id
             LEFT JOIN worktree_modes m ON m.worktree_id = w.worktree_id
             WHERE w.worktree_id = ?1"
        }
        Entity::TaskBranch => {
            "SELECT t.run_id, b.task_id, t.current_worker_id FROM branches b
             LEFT JOIN tasks t ON t.task_id = b.task_id WHERE b.name = ?1"
        }
        Entity::Job | Entity::NeedsYou | Entity::Bundle | Entity::Feature | Entity::Idea => {
            return Ok((None, None, None))
        }
    };
    let row = conn
        .query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    Ok(row.unwrap_or((None, None, None)))
}

/// 1 event of a state of 1 row.
#[derive(Debug, Clone, Copy)]
pub struct Event<'a> {
    pub entity: Entity,
    pub id: &'a str,
    /// `None` when the row is new.
    pub from: Option<&'a str>,
    pub to: &'a str,
    pub cause: &'a str,
    pub actor: Actor,
}

/// Append the `transition` event `e`, with the columns of today filled as
/// `taskledger::util::event` fills them. Each state change and each new row
/// of an entity has 1 event, so this marks the transaction for
/// [`in_door_transaction`].
pub fn append_event(tx: &Connection, e: &Event<'_>) -> Result<()> {
    insert_event(
        tx,
        &Record {
            entity: e.entity,
            id: e.id,
            from: e.from,
            to: Some(e.to),
            cause: e.cause,
            actor: e.actor,
        },
    )
}

/// The fields of 1 event row. `to` is `None` when the row is gone
/// ([`drop_reserved`]).
struct Record<'a> {
    entity: Entity,
    id: &'a str,
    from: Option<&'a str>,
    to: Option<&'a str>,
    cause: &'a str,
    actor: Actor,
}

fn insert_event(tx: &Connection, e: &Record<'_>) -> Result<()> {
    let (run, task, worker) = about(tx, e.entity, e.id)?;
    // The text keeps the task words of today (MAP.md, Q3); `from_state` and
    // `to_state` hold the stored words.
    let shown = |word| match e.entity {
        Entity::Task => TaskStatus::shown_word(word),
        _ => word,
    };
    let from = e.from.map_or("none", shown);
    let shown_to = e.to.map_or("none", shown);
    let text = format!("{}; {} {}: {from} -> {shown_to}", e.cause, e.entity, e.id);
    tx.execute(
        "INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text,
            entity, entity_id, from_state, to_state, cause)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            util::now(),
            run,
            task,
            worker,
            e.actor.as_str(),
            EventKind::Transition.as_str(),
            text,
            e.entity.as_str(),
            e.id,
            e.from,
            e.to,
            e.cause
        ],
    )?;
    mark(tx);
    Ok(())
}

/// An error when `tx` has no open transaction, or when `guard` is not the
/// generation of the `fleetd` row. A writer that inserts a row before it
/// calls `created()` checks this first, so a refused call writes nothing.
pub fn check_writable(tx: &Connection, guard: Option<Generation>) -> Result<()> {
    if tx.is_autocommit() {
        bail!("a write of the ledger state needs an open transaction of the ledger");
    }
    if let Some(held) = guard {
        let found = generation(tx)?;
        if found != Some(held) {
            bail!(
                "the fleetd generation is {:?}, not {}: another fleetd holds the ledger",
                found.map(|g| g.0),
                held.0
            );
        }
    }
    Ok(())
}

/// Drop the `reserved` worktree row `id`, with its mode row, in the
/// caller's transaction `tx`: the exit of a `reserved` row whose
/// `worktree_add` job failed (MAP.md of fleetd-p3, Q18). 1 event from
/// `reserved` to no state records `cause`. A row in another state, a
/// missing row, a connection with no open transaction or a `fleetd` row of
/// another generation than `guard` gives an error, and nothing is written.
pub fn drop_reserved(
    tx: &Connection,
    guard: Option<Generation>,
    id: &str,
    cause: &str,
    actor: Actor,
) -> Result<()> {
    check_writable(tx, guard)?;
    let reserved = "reserved";
    match state_of(tx, Entity::Worktree, id)? {
        None => bail!("no worktree {id} in the ledger"),
        Some(state) if state != reserved => {
            bail!("worktree {id} is {state}: only a {reserved} row is dropped")
        }
        Some(_) => {}
    }
    // The event reads the run and the task of the row, so it goes first.
    let event = Record {
        entity: Entity::Worktree,
        id,
        from: Some(reserved),
        to: None,
        cause,
        actor,
    };
    insert_event(tx, &event)?;
    tx.execute("DELETE FROM worktree_modes WHERE worktree_id = ?1", [id])?;
    tx.execute("DELETE FROM worktrees WHERE worktree_id = ?1", [id])?;
    Ok(())
}

/// 1 event of 1 row that is not a state change.
#[derive(Debug, Clone, Copy)]
pub struct Fact<'a> {
    pub kind: EventKind,
    pub entity: Entity,
    pub id: &'a str,
    pub cause: &'a str,
    pub actor: Actor,
    /// The `data_json` column: 1 JSON value.
    pub data_json: &'a str,
}

/// Append the event `f` of kind `f.kind`, with the run, task and worker of
/// its row filled as [`append_event`] fills them. `from_state` and
/// `to_state` stay null: the row does not change.
pub fn append_fact(tx: &Connection, f: &Fact<'_>) -> Result<()> {
    let (run, task, worker) = about(tx, f.entity, f.id)?;
    let text = format!("{}: {}; {} {}", f.kind, f.cause, f.entity, f.id);
    tx.execute(
        "INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text,
            entity, entity_id, cause, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            util::now(),
            run,
            task,
            worker,
            f.actor.as_str(),
            f.kind.as_str(),
            text,
            f.entity.as_str(),
            f.id,
            f.cause,
            f.data_json
        ],
    )?;
    mark(tx);
    Ok(())
}

/// Whether an event of kind `kind` with the cause `cause` is about the row
/// `id` of `entity`. No index of `events` covers the entity, so the read
/// scans the table: a caller asks only about rows that need the answer.
pub fn has_fact(
    conn: &Connection,
    entity: Entity,
    id: &str,
    kind: EventKind,
    cause: &str,
) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events
            WHERE entity = ?1 AND entity_id = ?2 AND kind = ?3 AND cause = ?4)",
        params![entity.as_str(), id, kind.as_str(), cause],
        |r| r.get(0),
    )?)
}

/// The `data_json` of the first event of kind `kind` with the cause `cause`
/// about the row `id` of `entity`, or `None` when no event is. The read
/// scans the table, as [`has_fact`] does.
pub fn fact_data(
    conn: &Connection,
    entity: Entity,
    id: &str,
    kind: EventKind,
    cause: &str,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT data_json FROM events
            WHERE entity = ?1 AND entity_id = ?2 AND kind = ?3 AND cause = ?4
            ORDER BY event_id LIMIT 1",
            params![entity.as_str(), id, kind.as_str(), cause],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// The stamp of the last `transition` event that moved the row `id` of
/// `entity` to 1 of `states`, or `None` when no event did.
pub fn entered_at(
    conn: &Connection,
    entity: Entity,
    id: &str,
    states: &[&str],
) -> Result<Option<String>> {
    let mut last: Option<String> = None;
    for state in states {
        let at: Option<String> = conn.query_row(
            "SELECT max(at) FROM events
             WHERE entity = ?1 AND entity_id = ?2 AND kind = ?3 AND to_state = ?4",
            params![entity.as_str(), id, EventKind::Transition.as_str(), state],
            |r| r.get(0),
        )?;
        last = last.max(at);
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::application::ledger::{
        record as record_fact, record_refused, transition, Cause, DoorError, Moved,
    };
    use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;
    use rusqlite::{Connection, Transaction};

    struct TestLedger {
        _dir: tempfile::TempDir,
        store: Store,
        run_id: String,
    }

    fn ledger() -> TestLedger {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, dir.path(), &new))
            .unwrap();
        TestLedger {
            _dir: dir,
            store,
            run_id: run.run_id,
        }
    }

    impl TestLedger {
        fn task(&mut self, n: u32) -> String {
            let run = runs::get(self.store.conn(), &self.run_id).unwrap();
            let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
            self.store
                .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        }

        fn worker(&mut self) -> String {
            let new = NewWorker {
                run_id: self.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            };
            self.store
                .immediate(|tx| workers::insert(tx, &new))
                .unwrap()
        }

        fn conn(&self) -> &Connection {
            self.store.conn()
        }

        /// `to` for `entity` `id` in its own transaction.
        fn moved(&mut self, entity: Entity, id: &str, to: &str) -> Result<Moved, DoorError> {
            self.store
                .immediate(|tx| Ok(move_row(tx, None, entity, id, to, cause("test"))))
                .unwrap()
        }
    }

    /// `transition()` with the needs-you list of ideas as its refusals.
    fn move_row(
        tx: &Transaction<'_>,
        guard: Option<Generation>,
        entity: Entity,
        id: &str,
        to: &str,
        cause: Cause<'_>,
    ) -> Result<Moved, DoorError> {
        transition(tx, guard, entity, id, to, cause, &NeedsYouList)
    }

    fn cause(text: &str) -> Cause<'_> {
        Cause {
            actor: Actor::Horch,
            text,
        }
    }

    fn status(conn: &Connection, task: &str) -> String {
        conn.query_row("SELECT status FROM tasks WHERE task_id = ?1", [task], |r| {
            r.get(0)
        })
        .unwrap()
    }

    /// The transition events of 1 row: from, to, cause.
    fn events(
        conn: &Connection,
        entity: Entity,
        id: &str,
    ) -> Vec<(Option<String>, String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT from_state, to_state, cause FROM events
                 WHERE kind = 'transition' AND entity = ?1 AND entity_id = ?2
                 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([entity.as_str(), id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn open_items(conn: &Connection) -> Vec<(String, String, String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT key, kind, entity, entity_id FROM needs_you
                 WHERE state = 'open' ORDER BY key",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn illegal_transition_is_refused_and_writes_nothing() {
        let mut l = ledger();
        let task = l.task(1);
        let out = l.moved(Entity::Task, &task, "done");
        assert!(matches!(out, Err(DoorError::Refused { .. })), "{out:?}");
        assert_eq!(status(l.conn(), &task), "ready");
    }

    #[test]
    fn illegal_transition_does_not_stop_the_pass() {
        let mut l = ledger();
        let (a, b) = (l.task(1), l.task(2));
        l.store
            .immediate(|tx| {
                let refused = move_row(tx, None, Entity::Task, &a, "done", cause("pass"));
                assert!(matches!(refused, Err(DoorError::Refused { .. })));
                let legal = move_row(tx, None, Entity::Task, &b, "running", cause("pass"));
                assert_eq!(legal.unwrap(), Moved::Changed);
                Ok(())
            })
            .unwrap();
        assert_eq!(status(l.conn(), &a), "ready");
        assert_eq!(status(l.conn(), &b), "running");
    }

    #[test]
    fn illegal_transition_puts_1_needs_you_item_and_1_event() {
        let mut l = ledger();
        let task = l.task(1);
        let _ = l.moved(Entity::Task, &task, "done");
        let _ = l.moved(Entity::Task, &task, "done");
        let refused = (
            Some("ready".to_string()),
            "done".to_string(),
            "illegal_transition: test".to_string(),
        );
        assert_eq!(
            events(l.conn(), Entity::Task, &task),
            [refused.clone(), refused],
            "1 event for each refusal"
        );
        assert_eq!(
            open_items(l.conn()),
            [(
                format!("illegal_transition:task:{task}"),
                "illegal_transition".to_string(),
                "task".to_string(),
                task.clone()
            )],
            "1 item for the row, however often it is refused"
        );
    }

    /// The attempt `A1` of `task` and `worker`, in `launching`.
    fn attempt(l: &TestLedger, task: &str, worker: &str) -> &'static str {
        l.conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state)
                 VALUES ('A1', ?1, ?2, 'launching')",
                [task, worker],
            )
            .unwrap();
        "A1"
    }

    /// Move `id` of `entity` to `to`, then assert its row state and its 1
    /// event, from `from` with the cause `test`.
    fn assert_moved(l: &mut TestLedger, entity: Entity, id: &str, from: &str, to: &str) {
        assert_eq!(l.moved(entity, id, to).unwrap(), Moved::Changed);
        assert_eq!(state_of(l.conn(), entity, id).unwrap().as_deref(), Some(to));
        assert_eq!(
            events(l.conn(), entity, id),
            [(Some(from.to_string()), to.to_string(), "test".to_string())]
        );
    }

    #[test]
    fn each_transition_writes_1_event_with_its_cause_and_entity() {
        let mut l = ledger();
        let task = l.task(1);
        assert_moved(&mut l, Entity::Task, &task, "ready", "running");
    }

    #[test]
    fn a_worker_move_writes_the_worker_row_and_its_event() {
        let mut l = ledger();
        let worker = l.worker();
        assert_moved(&mut l, Entity::Worker, &worker, "spawning", "starting");
    }

    #[test]
    fn record_writes_1_event_of_its_kind_with_its_data_and_no_state_change() {
        let mut l = ledger();
        let task = l.task(1);
        let data = serde_json::json!({ "fleet_clock": 42 });
        let fact = |tx: &Connection| {
            record_fact(
                tx,
                None,
                EventKind::Doorbell,
                Entity::Task,
                &task,
                cause("rung 1"),
                &data,
            )
        };
        assert!(matches!(fact(l.conn()), Err(DoorError::NoTransaction)));
        l.store.immediate(|tx| Ok(fact(tx)?)).unwrap();
        let row: (String, String, String, String, Option<String>, String) = l
            .conn()
            .query_row(
                "SELECT kind, entity, entity_id, cause, to_state, data_json FROM events
                 WHERE kind = 'doorbell'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "doorbell".into(),
                "task".into(),
                task.clone(),
                "rung 1".into(),
                None,
                r#"{"fleet_clock":42}"#.into()
            )
        );
        assert_eq!(status(l.conn(), &task), "ready");
        let missing = l.store.immediate(|tx| {
            Ok(record_fact(
                tx,
                None,
                EventKind::Doorbell,
                Entity::Task,
                "nope",
                cause("x"),
                &data,
            ))
        });
        assert!(matches!(missing.unwrap(), Err(DoorError::NoRow { .. })));
    }

    #[test]
    fn has_fact_finds_only_an_event_of_its_row_kind_and_cause() {
        let mut l = ledger();
        let task = l.task(1);
        let other = l.task(2);
        let data = serde_json::json!({});
        l.store
            .immediate(|tx| {
                record_fact(
                    tx,
                    None,
                    EventKind::Note,
                    Entity::Task,
                    &task,
                    cause("seen"),
                    &data,
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        let has = |id: &str, kind: EventKind, text: &str| {
            has_fact(l.conn(), Entity::Task, id, kind, text).unwrap()
        };
        assert!(has(&task, EventKind::Note, "seen"));
        assert!(!has(&task, EventKind::Note, "other cause"));
        assert!(!has(&task, EventKind::Doorbell, "seen"));
        assert!(!has(&other, EventKind::Note, "seen"));
        assert!(!has_fact(l.conn(), Entity::Worker, &task, EventKind::Note, "seen").unwrap());
    }

    /// A ledger built at version `from` and migrated to the newest version
    /// accepts the event kinds that came after the step that built
    /// `events`: `doorbell` of the stall ladder, and `hold_started` and
    /// `hold_ended` of Phase 1. Each step runs with the code of this
    /// binary, so this does not cover a ledger whose `events` step ran
    /// under an older binary: its `CHECK` keeps the older list.
    #[test]
    fn migrated_ledger_accepts_the_new_event_kinds() {
        use crate::taskledger::schema;
        for from in [7, schema::PHASE_2_VERSION] {
            let dir = tempfile::tempdir().unwrap();
            let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
            schema::migrate_to(&mut conn, from).unwrap();
            schema::migrate(&mut conn).unwrap();
            for kind in [
                EventKind::Doorbell,
                EventKind::HoldStarted,
                EventKind::HoldEnded,
            ] {
                conn.execute(
                    "INSERT INTO events (at, actor, kind, text) VALUES ('now', 'horch', ?1, 'x')",
                    [kind.as_str()],
                )
                .unwrap_or_else(|e| panic!("from version {from}: {kind}: {e}"));
            }
        }
    }

    #[test]
    fn an_attempt_move_writes_the_attempt_row_and_its_event() {
        let mut l = ledger();
        let (task, worker) = (l.task(1), l.worker());
        let a1 = attempt(&l, &task, &worker);
        assert_moved(&mut l, Entity::Attempt, a1, "launching", "live");
    }

    #[test]
    fn a_job_move_finds_the_job_by_its_integer_id() {
        let mut l = ledger();
        l.conn()
            .execute(
                "INSERT INTO jobs (id, kind, subject, input_key, state, time_limit_s, created_at)
                 VALUES (7, 'task_check', 'T-1', 'k', 'queued', 60, 'now')",
                [],
            )
            .unwrap();
        assert_moved(&mut l, Entity::Job, "7", "queued", "running");
    }

    #[test]
    fn an_event_fills_the_columns_of_today_from_its_row() {
        let mut l = ledger();
        let (task, worker) = (l.task(1), l.worker());
        let a1 = attempt(&l, &task, &worker);
        l.moved(Entity::Attempt, a1, "live").unwrap();
        let row: (String, String, String, String, String) = l
            .conn()
            .query_row(
                "SELECT run_id, task_id, worker_id, actor, text FROM events
                 WHERE entity = 'attempt' AND entity_id = 'A1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        let (run, task_col, worker_col, actor, text) = row;
        assert_eq!(
            (run, task_col, worker_col),
            (l.run_id.clone(), task, worker)
        );
        assert_eq!(actor, "horch");
        assert_eq!(text, "test; attempt A1: launching -> live");
    }

    #[test]
    fn a_move_to_the_same_state_writes_no_event() {
        let mut l = ledger();
        let task = l.task(1);
        assert_eq!(
            l.moved(Entity::Task, &task, "ready").unwrap(),
            Moved::Unchanged
        );
        assert!(events(l.conn(), Entity::Task, &task).is_empty());
    }

    /// The refused move ready -> verifying of `task`, from a transaction
    /// that rolls back.
    fn rolled_back_refusal(l: &TestLedger, task: &str) -> DoorError {
        let tx =
            Transaction::new_unchecked(l.conn(), rusqlite::TransactionBehavior::Immediate).unwrap();
        let err = move_row(&tx, None, Entity::Task, task, "verifying", cause("test")).unwrap_err();
        drop(tx);
        err
    }

    /// `record_refused` in its own transaction.
    fn record(l: &TestLedger, err: &DoorError) -> bool {
        let tx =
            Transaction::new_unchecked(l.conn(), rusqlite::TransactionBehavior::Immediate).unwrap();
        let wrote = record_refused(&tx, err, &NeedsYouList).unwrap();
        tx.commit().unwrap();
        wrote
    }

    #[test]
    fn a_refusal_is_recorded_after_its_rollback() {
        let mut l = ledger();
        let task = l.task(1);
        let err = rolled_back_refusal(&l, &task);
        assert!(open_items(l.conn()).is_empty(), "the rollback removed it");
        assert!(record(&l, &err));
        assert_eq!(open_items(l.conn()).len(), 1);
        assert_eq!(events(l.conn(), Entity::Task, &task).len(), 1);
    }

    /// Review F4: the row moved after the rollback, so the refusal names a
    /// state that the row no longer has. Nothing is written.
    #[test]
    fn a_refusal_of_a_row_that_moved_on_is_not_recorded() {
        let mut l = ledger();
        let task = l.task(1);
        let err = rolled_back_refusal(&l, &task);
        l.moved(Entity::Task, &task, "running").unwrap();
        assert!(!record(&l, &err));
        assert!(open_items(l.conn()).is_empty());
        let events = events(l.conn(), Entity::Task, &task);
        assert!(events.iter().all(|e| e.1 != "verifying"), "{events:?}");
    }

    #[test]
    fn transition_outside_a_transaction_writes_nothing() {
        let mut l = ledger();
        let task = l.task(1);
        let out = transition(
            l.conn(),
            None,
            Entity::Task,
            &task,
            "running",
            cause("test"),
            &NeedsYouList,
        );
        assert!(matches!(out, Err(DoorError::NoTransaction)), "{out:?}");
        assert_eq!(status(l.conn(), &task), "ready");
        assert!(events(l.conn(), Entity::Task, &task)
            .iter()
            .all(|e| e.1 != "running"));
    }

    #[test]
    fn transition_with_a_lost_generation_writes_nothing() {
        let mut l = ledger();
        let task = l.task(1);
        l.conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 2, 1, 1, 'now', 0, 'now')",
                [],
            )
            .unwrap();
        l.store
            .immediate(|tx| {
                let lost = move_row(
                    tx,
                    Some(Generation(1)),
                    Entity::Task,
                    &task,
                    "done",
                    cause("old fleetd"),
                );
                assert!(
                    matches!(lost, Err(DoorError::GenerationLost { .. })),
                    "{lost:?}"
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(status(l.conn(), &task), "ready");
        assert!(events(l.conn(), Entity::Task, &task).is_empty());
        assert!(open_items(l.conn()).is_empty());

        l.store
            .immediate(|tx| {
                let own = Some(Generation(2));
                let moved = move_row(tx, own, Entity::Task, &task, "running", cause("fleetd"));
                assert_eq!(moved.unwrap(), Moved::Changed);
                Ok(())
            })
            .unwrap();
        assert_eq!(status(l.conn(), &task), "running");
    }

    #[test]
    fn record_with_a_lost_generation_writes_nothing() {
        let mut l = ledger();
        let task = l.task(1);
        l.conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 2, 1, 1, 'now', 0, 'now')",
                [],
            )
            .unwrap();
        let data = serde_json::json!({});
        let count = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind = 'doorbell'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        for (held, written) in [(1, 0), (2, 1)] {
            l.store
                .immediate(|tx| {
                    let out = record_fact(
                        tx,
                        Some(Generation(held)),
                        EventKind::Doorbell,
                        Entity::Task,
                        &task,
                        cause("fleetd"),
                        &data,
                    );
                    assert_eq!(out.is_ok(), written == 1, "{out:?}");
                    if written == 0 {
                        assert!(matches!(out, Err(DoorError::GenerationLost { .. })));
                    }
                    Ok(())
                })
                .unwrap();
            assert_eq!(count(l.conn()), written, "generation {held}");
        }
    }

    #[test]
    fn in_door_transaction_is_true_only_inside_the_transaction() {
        let mut l = ledger();
        let (a, b) = (l.task(1), l.task(2));
        assert!(!in_door_transaction());
        l.store
            .immediate(|tx| {
                assert!(!in_door_transaction(), "no transition() yet");
                move_row(tx, None, Entity::Task, &a, "running", cause("t")).unwrap();
                assert!(in_door_transaction());
                Ok(())
            })
            .unwrap();
        assert!(!in_door_transaction(), "after the commit");

        let rolled_back: anyhow::Result<()> = l.store.immediate(|tx| {
            move_row(tx, None, Entity::Task, &b, "running", cause("t")).unwrap();
            assert!(in_door_transaction());
            anyhow::bail!("roll back")
        });
        assert!(rolled_back.is_err());
        assert!(!in_door_transaction(), "after the rollback");
        assert_eq!(status(l.conn(), &b), "ready");
    }

    /// SQLite calls no commit hook for a transaction with no write (review
    /// F1).
    #[test]
    fn in_door_transaction_is_false_after_a_deferred_transaction_with_no_write() {
        let mut l = ledger();
        let task = l.task(1);
        let mut conn = Connection::open(l._dir.path().join("ledger.sqlite3")).unwrap();
        let tx = conn.transaction().unwrap();
        let unchanged = move_row(&tx, None, Entity::Task, &task, "ready", cause("t"));
        assert_eq!(unchanged.unwrap(), Moved::Unchanged);
        let missing = move_row(&tx, None, Entity::Task, "T-none", "running", cause("t"));
        assert!(
            matches!(missing, Err(DoorError::NoRow { .. })),
            "{missing:?}"
        );
        tx.commit().unwrap();
        assert!(!in_door_transaction());
    }

    /// A transaction whose only door write makes a needs-you item is a door
    /// transaction (review F2).
    #[test]
    fn in_door_transaction_is_true_while_a_new_needs_you_item_is_not_committed() {
        use crate::fleetd::ideas::application::needs_you::open_item;
        use crate::fleetd::ideas::domain::needs_you::Write;
        let mut l = ledger();
        let w = Write {
            key: "job_abandoned:J-1",
            kind: "job_abandoned",
            entity: "job",
            entity_id: "J-1",
            text: "3 runs ended abandoned",
        };
        l.store
            .immediate(|tx| {
                open_item(tx, &w)?;
                assert!(in_door_transaction());
                Ok(())
            })
            .unwrap();
        assert!(!in_door_transaction(), "after the commit");
        assert_eq!(
            events(l.conn(), Entity::NeedsYou, w.key),
            [(None, "open".to_string(), "job_abandoned".to_string())]
        );
    }

    /// The worktree row `id` of `task` in `state`, inserted as the test
    /// wants it: a state change of a test row needs no event.
    fn worktree(l: &TestLedger, id: &str, task: Option<&str>, state: &str) {
        l.conn()
            .execute(
                "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base,
                    status, created_at, state)
                 VALUES (?1, ?2, '/r', '/r/.worktrees/' || ?1, ?1, 'main', 'open', 'then', ?3)",
                params![id, task, state],
            )
            .unwrap();
    }

    fn worktree_events(l: &TestLedger, id: &str) -> Vec<(Option<String>, Option<String>, String)> {
        let mut stmt = l
            .conn()
            .prepare(
                "SELECT from_state, to_state, cause FROM events
                 WHERE kind = 'transition' AND entity = 'worktree' AND entity_id = ?1
                 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The design's exits of a worktree (MAP.md of fleetd-p3, Q10).
    const WORKTREE_EXITS: [(&str, &str); 12] = [
        ("reserved", "created"),
        ("created", "active"),
        ("active", "created"),
        ("active", "landing"),
        ("active", "orphaned"),
        ("landing", "landed"),
        ("landing", "rescued"),
        ("landed", "removed"),
        ("orphaned", "rescued"),
        ("orphaned", "removed"),
        ("rescued", "active"),
        ("rescued", "removed"),
    ];

    #[test]
    fn worktree_moves_along_each_exit_of_the_design() {
        let mut l = ledger();
        let task = l.task(1);
        for (n, (from, to)) in WORKTREE_EXITS.into_iter().enumerate() {
            let id = format!("wt{n}");
            worktree(&l, &id, Some(&task), from);
            assert_moved(&mut l, Entity::Worktree, &id, from, to);
        }
        let (run, task_col, text): (String, String, String) = l
            .conn()
            .query_row(
                "SELECT run_id, task_id, text FROM events WHERE entity_id = 'wt0'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((run, task_col), (l.run_id.clone(), task));
        assert_eq!(text, "test; worktree wt0: reserved -> created");
    }

    #[test]
    fn a_scratch_worktree_event_gets_its_run_from_its_mode_row() {
        let mut l = ledger();
        worktree(&l, "wt1", None, "created");
        l.conn()
            .execute(
                "INSERT INTO worktree_modes (worktree_id, run_id, name, scratch)
                 VALUES ('wt1', ?1, 'wt1', 1)",
                [&l.run_id],
            )
            .unwrap();
        l.moved(Entity::Worktree, "wt1", "active").unwrap();
        let run: String = l
            .conn()
            .query_row(
                "SELECT run_id FROM events WHERE entity_id = 'wt1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(run, l.run_id);
    }

    #[test]
    fn worktree_move_outside_the_table_is_refused_and_recorded() {
        let mut l = ledger();
        worktree(&l, "wt1", None, "reserved");
        let out = l.moved(Entity::Worktree, "wt1", "active");
        assert!(matches!(out, Err(DoorError::Refused { .. })), "{out:?}");
        assert_eq!(
            state_of(l.conn(), Entity::Worktree, "wt1")
                .unwrap()
                .as_deref(),
            Some("reserved")
        );
        assert_eq!(
            worktree_events(&l, "wt1"),
            [(
                Some("reserved".to_string()),
                Some("active".to_string()),
                "illegal_transition: test".to_string()
            )]
        );
        assert_eq!(
            open_items(l.conn()),
            [(
                "illegal_transition:worktree:wt1".to_string(),
                "illegal_transition".to_string(),
                "worktree".to_string(),
                "wt1".to_string()
            )]
        );
    }

    #[test]
    fn removed_worktree_is_terminal() {
        let mut l = ledger();
        worktree(&l, "wt1", None, "removed");
        for to in [
            "reserved", "created", "active", "landing", "landed", "orphaned", "rescued",
        ] {
            let out = l.moved(Entity::Worktree, "wt1", to);
            assert!(
                matches!(out, Err(DoorError::Refused { .. })),
                "{to}: {out:?}"
            );
        }
        assert_eq!(
            state_of(l.conn(), Entity::Worktree, "wt1")
                .unwrap()
                .as_deref(),
            Some("removed")
        );
    }

    fn drop_in_tx(l: &mut TestLedger, id: &str) -> anyhow::Result<()> {
        l.store
            .immediate(|tx| {
                Ok(drop_reserved(
                    tx,
                    None,
                    id,
                    "worktree_add_failed",
                    Actor::Horch,
                ))
            })
            .unwrap()
    }

    #[test]
    fn drop_reserved_deletes_only_a_reserved_row_and_writes_its_event() {
        let mut l = ledger();
        let task = l.task(1);
        worktree(&l, "wt1", Some(&task), "reserved");
        l.conn()
            .execute(
                "INSERT INTO worktree_modes (worktree_id, run_id, name) VALUES ('wt1', ?1, 'a')",
                [&l.run_id],
            )
            .unwrap();
        drop_in_tx(&mut l, "wt1").unwrap();
        assert_eq!(state_of(l.conn(), Entity::Worktree, "wt1").unwrap(), None);
        let modes: i64 = l
            .conn()
            .query_row("SELECT count(*) FROM worktree_modes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(modes, 0, "the mode row goes with its row");
        assert_eq!(
            worktree_events(&l, "wt1"),
            [(
                Some("reserved".to_string()),
                None,
                "worktree_add_failed".to_string()
            )]
        );
        let (run, task_col, text): (String, String, String) = l
            .conn()
            .query_row(
                "SELECT run_id, task_id, text FROM events WHERE entity_id = 'wt1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((run, task_col), (l.run_id.clone(), task));
        assert_eq!(text, "worktree_add_failed; worktree wt1: reserved -> none");

        for (n, state) in [
            "created", "active", "landing", "landed", "removed", "orphaned", "rescued",
        ]
        .into_iter()
        .enumerate()
        {
            let id = format!("other{n}");
            worktree(&l, &id, None, state);
            let err = drop_in_tx(&mut l, &id).unwrap_err();
            assert!(err.to_string().contains(state), "{state}: {err:#}");
            assert_eq!(
                state_of(l.conn(), Entity::Worktree, &id)
                    .unwrap()
                    .as_deref(),
                Some(state)
            );
            assert_eq!(worktree_events(&l, &id), []);
        }
        assert!(drop_in_tx(&mut l, "nope").is_err(), "no row");
    }

    #[test]
    fn drop_reserved_outside_a_transaction_writes_nothing() {
        let l = ledger();
        worktree(&l, "wt1", None, "reserved");
        let out = drop_reserved(l.conn(), None, "wt1", "worktree_add_failed", Actor::Horch);
        assert!(out.is_err());
        assert_eq!(
            state_of(l.conn(), Entity::Worktree, "wt1")
                .unwrap()
                .as_deref(),
            Some("reserved")
        );
    }

    #[test]
    fn drop_reserved_with_a_lost_generation_writes_nothing() {
        let mut l = ledger();
        worktree(&l, "wt1", None, "reserved");
        let out = l
            .store
            .immediate(|tx| {
                Ok(drop_reserved(
                    tx,
                    Some(Generation(7)),
                    "wt1",
                    "worktree_add_failed",
                    Actor::Horch,
                ))
            })
            .unwrap();
        assert!(out.is_err());
        assert_eq!(
            state_of(l.conn(), Entity::Worktree, "wt1")
                .unwrap()
                .as_deref(),
            Some("reserved")
        );
        assert_eq!(worktree_events(&l, "wt1"), []);
    }
}
