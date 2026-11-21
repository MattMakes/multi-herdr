use anyhow::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::model::{Actor, EventKind};
use crate::fleetd::fleet_runtime::application::ledger::{self, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> String {
    crate::clock::now_stamp()
}

pub fn seconds_between(from: &str, to: &str) -> Option<i64> {
    let a = crate::clock::parse(from)?;
    let b = crate::clock::parse(to)?;
    Some((b - a).num_seconds())
}

#[derive(Debug, Clone, Default)]
pub struct About<'a> {
    pub run: Option<&'a str>,
    pub task: Option<&'a str>,
    pub worker: Option<&'a str>,
}

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

pub fn atomic<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f(conn);
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    match f(&tx) {
        Ok(out) => {
            tx.commit()?;
            Ok(out)
        }
        Err(e) => {
            drop(tx);
            record_refusal_after_rollback(conn, &e);
            Err(e)
        }
    }
}

pub fn record_refusal_after_rollback(conn: &Connection, err: &anyhow::Error) {
    let Some(refused) = err.downcast_ref::<DoorError>() else {
        return;
    };
    let write = || -> Result<()> {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        if ledger::record_refused(&tx, refused, &NeedsYouList)? {
            tx.commit()?;
        } else if matches!(refused, DoorError::Refused { .. }) {
            anyhow::bail!("the row changed after the rollback");
        }
        Ok(())
    };
    if let Err(e) = write() {
        eprintln!("horch: the ledger did not record the refusal ({refused}): {e:#}");
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Why<'a> {
    Event(EventKind, &'a str),
    Rule(Rule, &'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    End,
    Handoff,
    LaunchFailed,
    SupervisorStarted,
    AgentStarted,
    Finish,
    DependenciesMet,
}

impl Rule {
    fn as_str(self) -> &'static str {
        match self {
            Rule::End => "end",
            Rule::Handoff => "handoff",
            Rule::LaunchFailed => "launch_failed",
            Rule::SupervisorStarted => "supervisor_started",
            Rule::AgentStarted => "agent_started",
            Rule::Finish => "finish",
            Rule::DependenciesMet => "dependencies_met",
        }
    }
}

const SEP: &str = ": ";

impl Why<'_> {
    pub fn cause(self) -> String {
        let (name, text) = match self {
            Why::Event(kind, text) => (kind.as_str(), text),
            Why::Rule(rule, text) => (rule.as_str(), text),
        };
        format!("{name}{SEP}{text}")
    }
}

pub fn transition(
    conn: &Connection,
    entity: Entity,
    id: &str,
    to: &str,
    actor: Actor,
    why: Why<'_>,
) -> Result<()> {
    let text = why.cause();
    let cause = Cause { actor, text: &text };
    ledger::transition(conn, None, entity, id, to, cause, &NeedsYouList)?;
    Ok(())
}

fn cause_name_sql() -> String {
    format!("substr(cause, 1, instr(cause, '{SEP}') - 1)")
}

fn shows_event_sql() -> String {
    let words: Vec<String> = EventKind::ALL
        .iter()
        .filter(|k| **k != EventKind::Transition)
        .map(|k| format!("'{}'", k.as_str()))
        .collect();
    format!(
        "kind = '{}' AND {} IN ({})",
        EventKind::Transition.as_str(),
        cause_name_sql(),
        words.join(", ")
    )
}

pub fn shown_kind_sql() -> String {
    format!(
        "CASE WHEN {} THEN {} ELSE kind END",
        shows_event_sql(),
        cause_name_sql()
    )
}

pub fn shown_text_sql() -> String {
    format!(
        "CASE WHEN {} THEN substr(cause, instr(cause, '{SEP}') + {}) ELSE text END",
        shows_event_sql(),
        SEP.len()
    )
}

pub fn kind_is(p: &str) -> String {
    format!("({} = {p})", shown_kind_sql())
}

pub fn host_name() -> String {
    crate::runtime::machine::host_name()
}

pub fn paths_overlap(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

pub fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

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

    fn shown(kind: &str, cause: Option<&str>) -> (String, String) {
        let conn = Connection::open_in_memory().unwrap();
        conn.query_row(
            &format!(
                "SELECT {}, {} FROM (SELECT ?1 AS kind, 'raw' AS text, ?2 AS cause)",
                shown_kind_sql(),
                shown_text_sql()
            ),
            rusqlite::params![kind, cause],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    fn pair(k: &str, t: &str) -> (String, String) {
        (k.to_string(), t.to_string())
    }

    #[test]
    fn an_event_cause_shows_as_its_kind_and_its_text() {
        let claimed = Why::Event(EventKind::Claimed, "w1 claimed T-1.").cause();
        assert_eq!(claimed, "claimed: w1 claimed T-1.");
        assert_eq!(
            shown("transition", Some(&claimed)),
            pair("claimed", "w1 claimed T-1.")
        );
        let no_task = Why::Event(EventKind::NoTask, "a: b").cause();
        assert_eq!(shown("transition", Some(&no_task)), pair("no_task", "a: b"));
    }

    #[test]
    fn a_rule_cause_and_other_events_show_as_they_are() {
        let end = Why::Rule(Rule::End, "done").cause();
        assert_eq!(end, "end: done");
        assert_eq!(shown("transition", Some(&end)), pair("transition", "raw"));
        assert_eq!(
            shown("transition", Some("requeued")),
            pair("transition", "raw")
        );
        assert_eq!(shown("transition", None), pair("transition", "raw"));
        assert_eq!(
            shown("progress", Some("claimed: x")),
            pair("progress", "raw")
        );
    }

    #[test]
    fn no_rule_is_named_as_an_event_kind() {
        use Rule::*;
        for rule in [
            End,
            Handoff,
            LaunchFailed,
            SupervisorStarted,
            AgentStarted,
            Finish,
            DependenciesMet,
        ] {
            assert!(EventKind::parse(rule.as_str()).is_err(), "{rule:?}");
        }
    }

    #[test]
    fn kind_is_matches_the_kind_that_a_view_shows() {
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
