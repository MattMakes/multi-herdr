//! The needs-you list: add, read and clear an item.
//!
//! Each use case that changes a state writes the row and 1 event in the
//! transaction it is given, through `transition()`.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, Transaction};

use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::sqlite as store;
use crate::fleetd::ideas::domain::needs_you::{Item, State, Write};
use crate::taskledger::model::Actor;
use crate::taskledger::util;

/// Write the open item of `w.key`. An open item gets the new text; a
/// `resolved` or `superseded` item opens again.
pub fn open_item(tx: &Transaction<'_>, w: &Write<'_>) -> Result<()> {
    let old = store::get(tx, w.key)?;
    let Some(new) = Item::written(old.as_ref(), w, &util::now()) else {
        return Ok(());
    };
    save(tx, old.as_ref(), &new, w.kind)
}

/// End the open item of `key` as `resolved`. `false` when no open item has
/// that key.
pub fn resolve(tx: &Transaction<'_>, key: &str) -> Result<bool> {
    end(tx, key, State::Resolved, "its cause ended")
}

/// End the open item of `key` as `superseded`. `false` when no open item
/// has that key.
pub fn supersede(tx: &Transaction<'_>, key: &str) -> Result<bool> {
    end(tx, key, State::Superseded, "a newer item replaced it")
}

fn end(tx: &Transaction<'_>, key: &str, to: State, cause: &str) -> Result<bool> {
    let Some(old) = store::get(tx, key)? else {
        return Ok(false);
    };
    let Some(new) = old.ended(to, &util::now()) else {
        return Ok(false);
    };
    save(tx, Some(&old), &new, cause)?;
    Ok(true)
}

/// Write `new` as the 1 row of its key; `old` is the row before. A new row
/// gets its event from `created()`. A change of state goes through
/// `transition()` before the other columns, so a refused move changes
/// nothing.
fn save(tx: &Transaction<'_>, old: Option<&Item>, new: &Item, cause: &str) -> Result<()> {
    let cause = Cause {
        actor: Actor::Horch,
        text: cause,
    };
    let Some(old) = old else {
        store::insert(tx, new)?;
        created(tx, None, Entity::NeedsYou, &new.key, cause)?;
        return Ok(());
    };
    if old.state != new.state {
        transition(
            tx,
            None,
            Entity::NeedsYou,
            &new.key,
            new.state.as_str(),
            cause,
        )?;
    }
    store::update(tx, new)
}

/// Remove each item that ended `retention_days` or more before `now`.
/// Returns the count removed.
pub fn prune(conn: &Connection, now: DateTime<Utc>, retention_days: u64) -> Result<usize> {
    let days = i64::try_from(retention_days).unwrap_or(i64::MAX);
    let cutoff = Duration::try_days(days)
        .and_then(|d| now.checked_sub_signed(d))
        .map_or_else(|| "0".to_string(), crate::clock::stamp);
    store::delete_ended(conn, &cutoff)
}

/// The open items: the only items shown.
pub fn open_items(conn: &Connection) -> Result<Vec<Item>> {
    store::open(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::store::Store;

    fn ledger() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        (dir, store)
    }

    fn write(store: &mut Store, key: &str, text: &str) {
        let (kind, id) = key.split_once(':').unwrap();
        store
            .immediate(|tx| {
                let w = Write {
                    key,
                    kind,
                    entity: "task",
                    entity_id: id,
                    text,
                };
                open_item(tx, &w)
            })
            .unwrap();
    }

    fn row_count(store: &Store) -> i64 {
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM needs_you", [], |r| r.get(0))
            .unwrap()
    }

    fn events_of(store: &Store, key: &str) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare("SELECT text FROM events WHERE kind = 'transition' ORDER BY event_id")
            .unwrap();
        let texts = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap);
        texts.filter(|t| t.contains(key)).collect()
    }

    #[test]
    fn needs_you_item_written_twice_by_key_is_1_item_and_resolves_with_its_row() {
        let (_dir, mut store) = ledger();
        write(&mut store, "illegal_transition:T-3", "first");
        write(&mut store, "illegal_transition:T-3", "second");
        assert_eq!(row_count(&store), 1);
        let item = store::get(store.conn(), "illegal_transition:T-3")
            .unwrap()
            .unwrap();
        assert_eq!((item.state, item.text.as_str()), (State::Open, "second"));
        assert_eq!(events_of(&store, "illegal_transition:T-3").len(), 1);

        // A resolve that does not commit leaves the row and the event out.
        let rolled_back: Result<()> = store.immediate(|tx| {
            assert!(resolve(tx, "illegal_transition:T-3")?);
            anyhow::bail!("roll back")
        });
        assert!(rolled_back.is_err());
        let item = store::get(store.conn(), "illegal_transition:T-3")
            .unwrap()
            .unwrap();
        assert_eq!(item.state, State::Open);
        assert_eq!(events_of(&store, "illegal_transition:T-3").len(), 1);

        assert!(store
            .immediate(|tx| resolve(tx, "illegal_transition:T-3"))
            .unwrap());
        let item = store::get(store.conn(), "illegal_transition:T-3")
            .unwrap()
            .unwrap();
        assert_eq!(item.state, State::Resolved);
        assert!(item.ended_at.is_some());
        let events = events_of(&store, "illegal_transition:T-3");
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(events[1].ends_with("open -> resolved"), "{events:?}");
        assert!(!store
            .immediate(|tx| resolve(tx, "illegal_transition:T-3"))
            .unwrap());
        assert!(!store.immediate(|tx| resolve(tx, "no_such:key")).unwrap());
    }

    #[test]
    fn only_open_items_are_shown() {
        let (_dir, mut store) = ledger();
        write(&mut store, "illegal_transition:T-1", "a");
        write(&mut store, "illegal_transition:T-2", "b");
        write(&mut store, "job_abandoned:J-1", "c");
        store
            .immediate(|tx| resolve(tx, "illegal_transition:T-1"))
            .unwrap();
        store
            .immediate(|tx| supersede(tx, "job_abandoned:J-1"))
            .unwrap();
        let keys: Vec<String> = open_items(store.conn())
            .unwrap()
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert_eq!(keys, ["illegal_transition:T-2"]);
    }

    #[test]
    fn item_not_open_is_removed_30_days_after_it_ended() {
        let (_dir, mut store) = ledger();
        write(&mut store, "illegal_transition:T-1", "ended");
        write(&mut store, "illegal_transition:T-2", "still open");
        store
            .immediate(|tx| resolve(tx, "illegal_transition:T-1"))
            .unwrap();
        let ended = store::get(store.conn(), "illegal_transition:T-1")
            .unwrap()
            .unwrap()
            .ended_at
            .unwrap();
        let ended = crate::clock::parse(&ended).unwrap();

        let day_29 = ended + Duration::days(29);
        assert_eq!(prune(store.conn(), day_29, 30).unwrap(), 0);
        assert_eq!(row_count(&store), 2);

        let day_30 = ended + Duration::days(30);
        assert_eq!(prune(store.conn(), day_30, 30).unwrap(), 1);
        assert!(store::get(store.conn(), "illegal_transition:T-1")
            .unwrap()
            .is_none());

        let much_later = ended + Duration::days(400);
        assert_eq!(prune(store.conn(), much_later, 30).unwrap(), 0);
        assert_eq!(row_count(&store), 1, "an open item is never removed");
    }

    #[test]
    fn resolved_item_opens_again_on_a_new_write_of_its_key() {
        let (_dir, mut store) = ledger();
        write(&mut store, "job_abandoned:J-1", "first");
        store
            .immediate(|tx| resolve(tx, "job_abandoned:J-1"))
            .unwrap();
        write(&mut store, "job_abandoned:J-1", "again");
        assert_eq!(row_count(&store), 1);
        let item = store::get(store.conn(), "job_abandoned:J-1")
            .unwrap()
            .unwrap();
        assert_eq!((item.state, item.text.as_str()), (State::Open, "again"));
        assert_eq!(item.ended_at, None);
        let events = events_of(&store, "job_abandoned:J-1");
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(events[2].ends_with("resolved -> open"), "{events:?}");
    }
}
