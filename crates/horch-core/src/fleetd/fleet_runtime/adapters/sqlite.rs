//! The 1 door: the SQLite writes of the state of each entity.
//!
//! `application::ledger::transition()` is the only caller of
//! [`set_state`]. Each state change and its event go through it
//! (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One door").

use std::cell::RefCell;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::taskledger::model::{Actor, EventKind};
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
    }
}

/// The state of the row `id` of `entity`; `None` when no row has that id.
pub fn state_of(conn: &Connection, entity: Entity, id: &str) -> Result<Option<String>> {
    let (name, key, state) = table(entity);
    let sql = format!("SELECT {state} FROM {name} WHERE {key} = ?1");
    Ok(conn.query_row(&sql, [id], |r| r.get(0)).optional()?)
}

/// Write `to` in the state column of the row `id` of `entity`. Only
/// `transition()` calls this.
pub fn set_state(conn: &Connection, entity: Entity, id: &str, to: &str) -> Result<()> {
    let (name, key, state) = table(entity);
    let sql = format!("UPDATE {name} SET {state} = ?2 WHERE {key} = ?1");
    conn.execute(&sql, params![id, to])?;
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
        Entity::Job | Entity::NeedsYou => return Ok((None, None, None)),
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
    let (run, task, worker) = about(tx, e.entity, e.id)?;
    let from = e.from.unwrap_or("none");
    let text = format!("{}; {} {}: {from} -> {}", e.cause, e.entity, e.id, e.to);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
    use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;
    use rusqlite::{Connection, Transaction};

    struct Ledger {
        _dir: tempfile::TempDir,
        store: Store,
        run_id: String,
    }

    fn ledger() -> Ledger {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, dir.path(), &new))
            .unwrap();
        Ledger {
            _dir: dir,
            store,
            run_id: run.run_id,
        }
    }

    impl Ledger {
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
        assert_eq!(status(l.conn(), &task), "queued");
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
        assert_eq!(status(l.conn(), &a), "queued");
        assert_eq!(status(l.conn(), &b), "running");
    }

    #[test]
    fn illegal_transition_puts_1_needs_you_item_and_1_event() {
        let mut l = ledger();
        let task = l.task(1);
        let _ = l.moved(Entity::Task, &task, "done");
        let _ = l.moved(Entity::Task, &task, "done");
        let refused = (
            Some("queued".to_string()),
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
    fn attempt(l: &Ledger, task: &str, worker: &str) -> &'static str {
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
    fn assert_moved(l: &mut Ledger, entity: Entity, id: &str, from: &str, to: &str) {
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
        assert_moved(&mut l, Entity::Task, &task, "queued", "running");
    }

    #[test]
    fn a_worker_move_writes_the_worker_row_and_its_event() {
        let mut l = ledger();
        let worker = l.worker();
        assert_moved(&mut l, Entity::Worker, &worker, "spawning", "starting");
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
            l.moved(Entity::Task, &task, "queued").unwrap(),
            Moved::Unchanged
        );
        assert!(events(l.conn(), Entity::Task, &task).is_empty());
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
        assert_eq!(status(l.conn(), &task), "queued");
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
        assert_eq!(status(l.conn(), &task), "queued");
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
        assert_eq!(status(l.conn(), &b), "queued");
    }

    /// SQLite calls no commit hook for a transaction with no write (review
    /// F1).
    #[test]
    fn in_door_transaction_is_false_after_a_deferred_transaction_with_no_write() {
        let mut l = ledger();
        let task = l.task(1);
        let mut conn = Connection::open(l._dir.path().join("ledger.sqlite3")).unwrap();
        let tx = conn.transaction().unwrap();
        let unchanged = move_row(&tx, None, Entity::Task, &task, "queued", cause("t"));
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
}
