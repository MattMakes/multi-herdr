//! Small helpers every task ledger module uses.

use anyhow::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::model::{Actor, EventKind};
use crate::fleetd::fleet_runtime::application::ledger::{self, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;

/// A fresh row id.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// "Now" as the ledger writes it: the one clock (`HORCH_NOW` pins it).
pub fn now() -> String {
    crate::clock::now_stamp()
}

/// Seconds from `from` to `to`, both ledger stamps; `None` when one does not
/// parse.
pub fn seconds_between(from: &str, to: &str) -> Option<i64> {
    let a = crate::clock::parse(from)?;
    let b = crate::clock::parse(to)?;
    Some((b - a).num_seconds())
}

/// The ids an event is about.
#[derive(Debug, Clone, Default)]
pub struct About<'a> {
    pub run: Option<&'a str>,
    pub task: Option<&'a str>,
    pub worker: Option<&'a str>,
}

/// Append 1 event.
pub fn event(
    conn: &Connection,
    about: &About<'_>,
    actor: Actor,
    kind: EventKind,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            now(),
            about.run,
            about.task,
            about.worker,
            actor.as_str(),
            kind.as_str(),
            text
        ],
    )?;
    Ok(())
}

/// Run `f` in 1 transaction of `conn`: the open transaction of the
/// caller, else a new `BEGIN IMMEDIATE` that commits when `f` succeeds.
pub fn atomic<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f(conn);
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

/// Move the row `id` of `entity` to `to` through `transition()`, the 1
/// door, in the open transaction of `conn`. `cause` is `"<word>: <text>"`
/// with the event word and the text of today when the change had an event,
/// so that [`event_view`] shows the event as before. A refused move is an
/// error.
pub fn transition(
    conn: &Connection,
    entity: Entity,
    id: &str,
    to: &str,
    actor: Actor,
    cause: &str,
) -> Result<()> {
    let cause = Cause { actor, text: cause };
    ledger::transition(conn, None, entity, id, to, cause, &NeedsYouList)?;
    Ok(())
}

/// The kind and the text that a view shows for an event. A `transition`
/// event whose cause is `"<word>: <text>"`, with an event word of today
/// (`claimed`, `requeued`), shows as that word and that text, as before.
/// Each other event shows its own kind and text.
pub fn event_view(kind: String, text: String, cause: Option<&str>) -> (String, String) {
    let parts = cause.and_then(|c| c.split_once(": "));
    match parts {
        Some((word, said))
            if kind == EventKind::Transition.as_str()
                && word != kind
                && EventKind::parse(word).is_ok() =>
        {
            (word.to_string(), said.to_string())
        }
        _ => (kind, text),
    }
}

/// The SQL condition that an event shows the kind of the parameter `p`
/// (for example `?2`), as [`event_view`] gives it.
pub fn kind_is(p: &str) -> String {
    format!(
        "(kind = {p} OR (kind = 'transition'
            AND substr(cause, 1, length({p}) + 2) = {p} || ': '))"
    )
}

/// This machine's name, for `workers.host`. A PID names a process only on
/// the host that recorded it.
pub fn host_name() -> String {
    crate::runtime::machine::host_name()
}

/// Whether 2 repo-relative paths overlap: equal, or 1 is a directory that
/// holds the other. `src/a` and `src/a/b.rs` overlap; `src/a` and `src/ab`
/// do not.
pub fn paths_overlap(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

/// A JSON array of strings, for a TEXT column.
pub fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

/// The strings of a JSON-array TEXT column; empty on anything else.
pub fn parse_json_list(text: Option<&str>) -> Vec<String> {
    text.and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_by_path_component() {
        assert!(paths_overlap("src/a", "src/a"));
        assert!(paths_overlap("src/a", "src/a/b.rs"));
        assert!(paths_overlap("src/a/b.rs", "src/a/"));
        assert!(!paths_overlap("src/a", "src/ab"));
        assert!(!paths_overlap("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn a_transition_event_shows_the_word_and_the_text_of_its_cause() {
        let view = |kind: &str, cause: Option<&str>| {
            let (k, t) = event_view(kind.into(), "raw".into(), cause);
            (k, t)
        };
        let pair = |k: &str, t: &str| (k.to_string(), t.to_string());
        assert_eq!(
            view("transition", Some("claimed: w1 claimed T-1.")),
            pair("claimed", "w1 claimed T-1.")
        );
        assert_eq!(
            view("transition", Some("end: done")),
            pair("transition", "raw")
        );
        assert_eq!(
            view("transition", Some("requeued")),
            pair("transition", "raw")
        );
        assert_eq!(view("transition", None), pair("transition", "raw"));
        assert_eq!(
            view("progress", Some("claimed: x")),
            pair("progress", "raw")
        );
    }

    #[test]
    fn kind_is_matches_the_kind_or_the_word_of_the_cause() {
        let conn = Connection::open_in_memory().unwrap();
        let shows = |kind: &str, cause: Option<&str>| -> bool {
            conn.query_row(
                &format!(
                    "SELECT {} FROM (SELECT ?1 AS kind, ?2 AS cause)",
                    kind_is("?3")
                ),
                rusqlite::params![kind, cause, "no_task"],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(shows("no_task", None));
        assert!(shows("transition", Some("no_task: no task for dev")));
        assert!(!shows("transition", Some("no_task")));
        assert!(!shows("transition", Some("no_tasks: x")));
        assert!(!shows("transition", Some("noXtask: x")));
        assert!(!shows("claimed", None));
    }

    #[test]
    fn seconds_between_stamps() {
        assert_eq!(
            seconds_between("2026-10-27T10:00:00Z", "2026-10-27T10:10:00Z"),
            Some(600)
        );
        assert_eq!(seconds_between("x", "2026-10-27T10:10:00Z"), None);
    }
}
