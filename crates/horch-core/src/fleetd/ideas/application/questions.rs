//! The use cases of a question: ask and answer.
//!
//! Each use case runs in the transaction of its caller, else in 1 new
//! transaction.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::questions_store as store;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::application::question_flow::{feature_of_task, needs_you_key, owner_of};
use crate::fleetd::ideas::domain::questions::{next_n, row_id, Question, State};
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::{tasks, util};

/// 1 question of the worker of an attempt, from its question file. Its
/// feature is the feature of the idea of its task, and its owner is the
/// sub-orchestrator of that feature ([`owner_of`]; fleetd-p6 MAP.md, Q11).
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    /// The id of the mail of a QUESTION line, so that `horch tell <role>
    /// --answers <id>` finds the row. `None` for `horch ask`.
    pub mail_id: Option<&'a str>,
    pub text: &'a str,
    pub asked_at: &'a str,
}

/// The `questions` row of the question of `a`: the row of its mail that
/// exists, else a new `open` row of the feature of its task, owned by the
/// sub-orchestrator of that feature, with the time limit `question_limit_s`
/// after `clock`, and 1 `question` event that holds the mail id and the
/// text. The question flow counts the limit only while the owner runs
/// ([`crate::fleetd::ideas::application::question_flow`]).
pub fn ask(
    conn: &Connection,
    a: &Ask<'_>,
    clock: FleetSeconds,
    question_limit_s: u64,
) -> Result<Question> {
    util::atomic(conn, |tx| {
        let asked = facts(tx, a.task_id, EventKind::Question)?
            .into_iter()
            .find(|f| a.mail_id.is_some() && f.mail_id.as_deref() == a.mail_id);
        if let Some(q) = asked.and_then(|f| store::get(tx, &f.question_id).transpose()) {
            return q;
        }
        let feature = feature_of_task(tx, a.task_id)?.with_context(|| {
            format!(
                "task {} has no idea, so its question has no owner",
                a.task_id
            )
        })?;
        let ids = store::ids_of_attempt(tx, a.attempt_id)?;
        let n = next_n(a.attempt_id, ids.iter().map(String::as_str));
        let q = Question {
            id: row_id(a.attempt_id, n),
            task_id: a.task_id.to_string(),
            owner: owner_of(&feature),
            feature_id: feature,
            state: State::Open,
            due_at: clock.0.saturating_add_unsigned(question_limit_s),
            asked_at: a.asked_at.to_string(),
            answered_at: None,
        };
        store::insert(tx, &q)?;
        let text = format!("question {} asked", q.id);
        let data = json!({ "question_id": q.id, "mail_id": a.mail_id, "text": a.text });
        record_event(tx, EventKind::Question, a.task_id, &text, &data)?;
        Ok(q)
    })
}

/// The answer `answer` to the mail question `mail_id`: its row that waits
/// for an answer goes to `answered`. The id of the row, or `None` when no
/// such row asked it.
pub fn answer_mail(conn: &Connection, mail_id: &str, answer: &str) -> Result<Option<String>> {
    util::atomic(conn, |tx| {
        let open = open_with_mail(tx)?;
        let Some((q, _)) = open
            .into_iter()
            .find(|(_, m)| m.as_deref() == Some(mail_id))
        else {
            return Ok(None);
        };
        answered(tx, &q, Some(answer))?;
        Ok(Some(q.id))
    })
}

/// The row `q` that waits for an answer (`open` or `escalated`) goes to
/// `answered`, with the answer text when it is known. Its needs-you item
/// is resolved, and its task is freed when no other question of the task
/// waits for an answer.
pub fn answered(conn: &Connection, q: &Question, answer: Option<&str>) -> Result<()> {
    util::atomic(conn, |tx| {
        if !store::set_answered(tx, &q.id, &util::now())? {
            return Ok(());
        }
        let text = format!("question {} answered", q.id);
        let data = json!({ "question_id": q.id, "text": answer });
        record_event(tx, EventKind::Answered, &q.task_id, &text, &data)?;
        needs_you::resolve(tx, &needs_you_key(&q.id))?;
        if store::unanswered_of_task(tx, &q.task_id)? == 0 {
            tasks::question_answered(tx, &q.task_id, &q.id)?;
        }
        Ok(())
    })
}

/// The `data_json` of a `question` or `answered` event of a row: the
/// `questions` table holds no text, so before the worker outbox of Phase 4
/// these events hold the mail id, the question and the answer.
#[derive(Debug, serde::Deserialize)]
struct Fact {
    question_id: String,
    mail_id: Option<String>,
    text: Option<String>,
}

/// The text of the question of the row `q`, when its `question` event holds
/// it.
pub fn asked_text(conn: &Connection, q: &Question) -> Result<Option<String>> {
    Ok(facts(conn, &q.task_id, EventKind::Question)?
        .into_iter()
        .find(|f| f.question_id == q.id && f.text.is_some())
        .and_then(|f| f.text))
}

/// The facts of kind `kind` about task `task_id`, oldest first.
fn facts(conn: &Connection, task_id: &str, kind: EventKind) -> Result<Vec<Fact>> {
    tasks::task_facts(conn, task_id, kind)?
        .into_iter()
        .map(|v| Ok(serde_json::from_value(v)?))
        .collect()
}

/// Each row that waits for an answer (`open` or `escalated`) with the mail
/// id of its question, oldest first.
pub fn open_with_mail(conn: &Connection) -> Result<Vec<(Question, Option<String>)>> {
    let mut out = Vec::new();
    for q in store::unanswered(conn)? {
        let mail = facts(conn, &q.task_id, EventKind::Question)?
            .into_iter()
            .find(|f| f.question_id == q.id)
            .and_then(|f| f.mail_id);
        out.push((q, mail));
    }
    Ok(out)
}

/// 1 answered question with its texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub id: String,
    pub question: Option<String>,
    /// `None` when the question closed with no recorded answer text.
    pub answer: Option<String>,
}

/// [`answered_of_task`] of each task of `task_ids`, by task: 3 statements
/// for any count of tasks. A task with no answered row is not in the map.
pub fn answered_of_tasks<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str> + Clone,
) -> Result<std::collections::BTreeMap<String, Vec<Answered>>> {
    let facts_by_task = |kind| -> Result<std::collections::BTreeMap<String, Vec<Fact>>> {
        let mut out: std::collections::BTreeMap<String, Vec<Fact>> = Default::default();
        for (task_id, v) in tasks::task_facts_of(conn, task_ids.clone(), kind)? {
            out.entry(task_id)
                .or_default()
                .push(serde_json::from_value(v)?);
        }
        Ok(out)
    };
    let asked = facts_by_task(EventKind::Question)?;
    let answers = facts_by_task(EventKind::Answered)?;
    let mut out: std::collections::BTreeMap<String, Vec<Answered>> = Default::default();
    for q in store::of_tasks(conn, task_ids)? {
        if q.state != State::Answered {
            continue;
        }
        let text = |facts: &std::collections::BTreeMap<String, Vec<Fact>>| {
            text_of(facts.get(&q.task_id).map_or(&[][..], Vec::as_slice), &q.id)
        };
        let answered = Answered {
            question: text(&asked),
            answer: text(&answers),
            id: q.id.clone(),
        };
        out.entry(q.task_id).or_default().push(answered);
    }
    Ok(out)
}

fn text_of(facts: &[Fact], id: &str) -> Option<String> {
    facts
        .iter()
        .rev()
        .find(|f| f.question_id == id && f.text.is_some())
        .and_then(|f| f.text.clone())
}

/// The answered rows of task `task_id`, oldest first, with their texts.
pub fn answered_of_task(conn: &Connection, task_id: &str) -> Result<Vec<Answered>> {
    let asked = facts(conn, task_id, EventKind::Question)?;
    let answers = facts(conn, task_id, EventKind::Answered)?;
    Ok(store::of_task(conn, task_id)?
        .into_iter()
        .filter(|q| q.state == State::Answered)
        .map(|q| Answered {
            question: text_of(&asked, &q.id),
            answer: text_of(&answers, &q.id),
            id: q.id,
        })
        .collect())
}

fn record_event(
    tx: &Connection,
    kind: EventKind,
    task_id: &str,
    text: &str,
    data: &serde_json::Value,
) -> Result<()> {
    let cause = Cause {
        actor: Actor::Horch,
        text,
    };
    record(tx, None, kind, Entity::Task, task_id, cause, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::store::Store;
    use crate::taskledger::tasks::NewTask;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        task: String,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let task = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    std::path::Path::new("/r"),
                    &NewRun {
                        slug: "q".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                let new = NewTask::work("t", "t.md", "opus");
                let task = tasks::add(tx, &run, &new, Actor::Orchestrator)?;
                Ok(task.task_id)
            })
            .unwrap();
        Fixture {
            _dir: dir,
            store,
            task,
        }
    }

    fn ask_of<'a>(f: &'a Fixture, mail_id: &'a str) -> Ask<'a> {
        Ask {
            task_id: &f.task,
            attempt_id: "A1",
            mail_id: Some(mail_id),
            text: "QUESTION: which?",
            asked_at: "2026-11-17T00:00:00Z",
        }
    }

    #[test]
    fn open_question_mail_writes_1_row_with_owner_and_due_at() {
        let f = fixture();
        let q = ask(f.store.conn(), &ask_of(&f, "m1"), FleetSeconds(100), 900).unwrap();
        assert_eq!(q.id, "A1/Q1");
        assert_eq!(q.feature_id, "F-0001", "the feature of the run of the task");
        assert_eq!(q.owner, "idea-lead:F-0001");
        assert_eq!(q.state, State::Open);
        assert_eq!(q.due_at, 1_000);
        assert_eq!(store::get(f.store.conn(), "A1/Q1").unwrap(), Some(q));
    }

    #[test]
    fn second_sync_keeps_1_row_for_1_question() {
        let f = fixture();
        let first = ask(f.store.conn(), &ask_of(&f, "m1"), FleetSeconds(100), 900).unwrap();
        let again = ask(f.store.conn(), &ask_of(&f, "m1"), FleetSeconds(500), 900).unwrap();
        assert_eq!(again, first, "the second ask keeps the row and its due_at");
        let other = ask(f.store.conn(), &ask_of(&f, "m2"), FleetSeconds(500), 900).unwrap();
        assert_eq!(other.id, "A1/Q2");
        let rows: i64 = f
            .store
            .conn()
            .query_row("SELECT count(*) FROM questions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[test]
    fn answer_moves_the_row_to_answered() {
        let f = fixture();
        ask(f.store.conn(), &ask_of(&f, "m1"), FleetSeconds(100), 900).unwrap();
        let id = answer_mail(f.store.conn(), "m1", "use the first").unwrap();
        assert_eq!(id.as_deref(), Some("A1/Q1"));
        let q = store::get(f.store.conn(), "A1/Q1").unwrap().unwrap();
        assert_eq!(q.state, State::Answered);
        assert!(q.answered_at.is_some());
        let texts = answered_of_task(f.store.conn(), &f.task).unwrap();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].question.as_deref(), Some("QUESTION: which?"));
        assert_eq!(texts[0].answer.as_deref(), Some("use the first"));
        assert_eq!(
            answer_mail(f.store.conn(), "m1", "again").unwrap(),
            None,
            "an answered row takes no second answer"
        );
    }
}
