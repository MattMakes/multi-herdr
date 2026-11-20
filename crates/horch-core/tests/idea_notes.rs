//! horch idea note (ticket 25 of fleetd-p6): the sub-orchestrator records
//! each question and answer as 1 file of its idea folder, and fleetd
//! ingests each file 1 time.
//!
//! `docs/specs/fleetd/design.md`, "Ideas" (The interview; How a worker's
//! question is resolved, step 4), "Workspace layout and file protocol"
//! (protocol rules 1, 5 and 7), A35; FM-31, RR-12, SR-6.
//!
//! Each test uses a temp ledger and a temp fleet root. The process chain is
//! a fake: no test reads a real process.
//!
//! Unix only: the protected file reader refuses each file on Windows, where
//! the platform gives no mode.
#![cfg(unix)]

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::peer_process::ProcessFacts;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::idea_notes_reader::{
    AnswerCase, Note, NoteBody, NoteWriter, NOTES, NOT_THE_SUB_ORCHESTRATOR,
};
use horch_core::fleetd::ideas::application::notes_ingest::{self, IdeaNotes, Report};
use horch_core::fleetd::task_execution::adapters::outbox_writer::NO_LIVE_ATTEMPT;
use horch_core::taskledger::schema;
use horch_platform::channel::User;
use rusqlite::{params, Connection};

const IDEA: &str = "I-0007";
const OTHER_IDEA: &str = "I-0008";
const AT: &str = "2026-11-18T12:00:00Z";

/// The PID of the worker command in each test, and of its parent chain:
/// command -> agent -> wrapper -> 1.
const COMMAND: u32 = 300;
const AGENT: u32 = 200;
const WRAPPER: u32 = 100;
const WRAPPER_START: u64 = 5;

/// A fake process table: the parent and the start time of each PID.
struct Chain {
    parent: HashMap<u32, u32>,
    start: HashMap<u32, u64>,
}

fn chain() -> Chain {
    Chain {
        parent: HashMap::from([(COMMAND, AGENT), (AGENT, WRAPPER), (WRAPPER, 1), (1, 0)]),
        start: HashMap::from([(COMMAND, 9), (AGENT, 7), (WRAPPER, WRAPPER_START), (1, 1)]),
    }
}

impl ProcessFacts for Chain {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.parent.get(&pid).copied()
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        self.start.get(&pid).copied()
    }

    fn in_group(&self, _pid: u32, _leader: u32, _leader_start: u64) -> Option<bool> {
        None
    }
}

/// A temp ledger with the schema of today, the row of [`IDEA`] in
/// `clarifying`, and a fleet root with the folder of [`IDEA`].
struct World {
    _temp: tempfile::TempDir,
    ideas: PathBuf,
    conn: Connection,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let ideas = temp.path().join("fleet").join("ideas");
    let mut conn = Connection::open(temp.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    // The ingest of an `ask` or an `interview` note records an idea event.
    conn.execute(
        "INSERT INTO ideas (id, feature_id, state, priority, created_at, updated_at)
         VALUES (?1, 'F-0001', 'clarifying', 0, ?2, ?2)",
        params![IDEA, AT],
    )
    .unwrap();
    IdeaFolder::create(&ideas, IDEA, owner()).unwrap();
    World {
        _temp: temp,
        ideas,
        conn,
    }
}

fn owner() -> User {
    User::current().unwrap()
}

impl World {
    /// Record the live attempt `id` of `task` or of `idea`, whose wrapper
    /// is [`WRAPPER`].
    fn live(&self, id: &str, task: Option<&str>, idea: Option<&str>) {
        self.conn
            .execute(
                "INSERT INTO attempts (id, task_id, idea_id, worker_id, pid, pid_start, state, started_at)
                 VALUES (?1, ?2, ?3, 'idea-lead-1', ?4, ?5, 'live', ?6)",
                params![
                    id,
                    task,
                    idea,
                    i64::from(WRAPPER),
                    i64::try_from(WRAPPER_START).unwrap(),
                    AT
                ],
            )
            .unwrap();
    }

    /// Record the live sub-orchestrator attempt of [`IDEA`].
    fn sub_orchestrator(&self) -> String {
        let id = format!("{IDEA}/A1");
        self.live(&id, None, Some(IDEA));
        id
    }

    /// The writer of the worker command [`COMMAND`] for `idea`.
    fn writer(&self, idea: &str) -> anyhow::Result<NoteWriter> {
        NoteWriter::of_process(COMMAND, &self.ideas, idea, &self.conn, &chain())
    }

    fn notes(&self, idea: &str) -> PathBuf {
        self.ideas.join(idea).join(NOTES)
    }

    /// Part 1 and part 2 of 1 pass, in 1 committed transaction.
    fn pass(&mut self) -> Report {
        let reads = self.read();
        let tx = self.conn.transaction().unwrap();
        let report = notes_ingest::ingest(&tx, None, &self.ideas, &owner(), &reads).unwrap();
        tx.commit().unwrap();
        report
    }

    fn read(&self) -> Vec<IdeaNotes> {
        notes_ingest::read(&self.conn, &self.ideas, &[IDEA.to_string()], &owner()).unwrap()
    }

    /// The cause and the data of each note event of [`IDEA`], oldest first.
    fn idea_notes(&self) -> Vec<(String, serde_json::Value)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT cause, data_json FROM events
                 WHERE entity = 'idea' AND entity_id = ?1 AND kind = 'note' ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([IDEA], |r| {
            let data: String = r.get(1)?;
            Ok((r.get(0)?, serde_json::from_str(&data).unwrap()))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// The `ingested` row of `path`: its outcome and its reason.
    fn ingested(&self, path: &Path) -> (String, Option<String>) {
        self.conn
            .query_row(
                "SELECT outcome, reason FROM ingested WHERE path = ?1",
                [path.display().to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn interview(&self) -> String {
        fs::read_to_string(self.ideas.join(IDEA).join("interview.md")).unwrap()
    }
}

fn interview_body() -> NoteBody {
    NoteBody::Interview {
        question: "Do retries apply to uploads?".into(),
        answer: "No. Only sync requests.".into(),
    }
}

fn answer_body() -> NoteBody {
    NoteBody::Answer {
        question: "T-0007-03/A1/Q1".into(),
        case: AnswerCase::Covered,
        cite: Some("assumptions.md:2".into()),
        text: "Use 5 tries. Assumption A1 covers it.".into(),
    }
}

/// Write `json` as note `n` of `idea` with mode 0600, as a raw writer can.
fn write_raw(w: &World, idea: &str, n: u32, json: &serde_json::Value) -> PathBuf {
    let dir = w.notes(idea);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join(format!("{n}.json"));
    fs::write(&path, serde_json::to_vec(json).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    path
}

#[test]
fn sub_orchestrator_answer_arrives_as_an_idea_note_file() {
    let mut w = world();
    let attempt = w.sub_orchestrator();

    let path = w.writer(IDEA).unwrap().write(answer_body()).unwrap();

    // 1 file in notes/ of its idea, mode 0600 in a folder of mode 0700.
    assert_eq!(path, w.notes(IDEA).join("1.json"));
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&w.notes(IDEA)), 0o700);
    let file: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(file["idea"], IDEA);
    assert_eq!(file["attempt"], attempt.as_str());
    assert_eq!(file["kind"], "answer");
    assert_eq!(file["question"], "T-0007-03/A1/Q1");
    assert_eq!(file["case"], "covered");
    assert_eq!(file["cite"], "assumptions.md:2");
    // The command writes no ledger row and no request (A35).
    assert_eq!(w.count("ingested"), 0);
    assert_eq!(w.count("requests"), 0);

    // fleetd passes the answer on 1 time.
    let first = w.pass();
    let expected = Note {
        idea: IDEA.into(),
        attempt,
        body: answer_body(),
    };
    assert_eq!(first.answers, vec![expected]);
    assert!(first.interview.is_empty() && first.dismissals.is_empty());
    assert!(first.rejected.is_empty());
    assert_eq!(w.ingested(&path).0, "accepted");
    let second = w.pass();
    assert!(second.answers.is_empty(), "{second:?}");
    // An answer note does not touch interview.md.
    assert!(!w.ideas.join(IDEA).join("interview.md").exists());
}

#[test]
fn interview_note_appends_to_interview_md_once() {
    let mut w = world();
    w.sub_orchestrator();
    let writer = w.writer(IDEA).unwrap();
    let first = writer.write(interview_body()).unwrap();
    let second = writer
        .write(NoteBody::Interview {
            question: "Is a config option needed?".into(),
            answer: "No.".into(),
        })
        .unwrap();
    assert_eq!(second, w.notes(IDEA).join("2.json"));

    // A pass whose transaction does not commit: the next pass must not
    // append the same answers again.
    let reads = w.read();
    {
        let tx = w.conn.transaction().unwrap();
        notes_ingest::ingest(&tx, None, &w.ideas, &owner(), &reads).unwrap();
        tx.rollback().unwrap();
    }
    let report = w.pass();
    assert_eq!(report.interview.len(), 2, "{report:?}");
    w.pass();
    w.pass();

    let interview = w.interview();
    assert_eq!(interview.matches("## Q").count(), 2, "{interview}");
    assert_eq!(interview.matches("Do retries apply to uploads?").count(), 1);
    assert!(interview
        .contains("## Q1\nDo retries apply to uploads?\n\nAnswer: No. Only sync requests.\n"));
    assert!(interview.contains("## Q2\nIs a config option needed?\n\nAnswer: No.\n"));
    assert_eq!(w.ingested(&first).0, "accepted");
    assert_eq!(w.ingested(&second).0, "accepted");
}

#[test]
fn note_of_another_idea_is_rejected() {
    let mut w = world();
    let attempt = w.sub_orchestrator();
    let path = write_raw(
        &w,
        IDEA,
        1,
        &serde_json::json!({
            "idea": OTHER_IDEA,
            "attempt": attempt,
            "kind": "interview",
            "question": "Do retries apply to uploads?",
            "text": "No.",
        }),
    );
    // A note of the right idea, from an attempt of another idea.
    w.live("I-0008/A1", None, Some(OTHER_IDEA));
    let foreign = write_raw(
        &w,
        IDEA,
        2,
        &serde_json::json!({
            "idea": IDEA,
            "attempt": "I-0008/A1",
            "kind": "interview",
            "question": "Do retries apply to uploads?",
            "text": "No.",
        }),
    );

    let report = w.pass();
    assert_eq!(report.rejected, vec![path.clone(), foreign.clone()]);
    assert!(report.interview.is_empty());
    let (outcome, reason) = w.ingested(&path);
    assert_eq!(outcome, "rejected");
    assert!(reason.unwrap().contains(OTHER_IDEA));
    let (outcome, reason) = w.ingested(&foreign);
    assert_eq!(outcome, "rejected");
    assert!(reason.unwrap().contains("I-0008/A1"));
    assert!(!w.ideas.join(IDEA).join("interview.md").exists());
    // A rejected path is not read again.
    assert!(w.read().iter().all(|r| r.notes.is_empty()));
}

#[test]
fn note_that_is_a_symlink_is_rejected() {
    let mut w = world();
    let attempt = w.sub_orchestrator();
    // A valid note outside notes/, and a link to it in notes/.
    let target = write_raw(
        &w,
        IDEA,
        9,
        &serde_json::json!({
            "idea": IDEA,
            "attempt": attempt,
            "kind": "interview",
            "question": "Do retries apply to uploads?",
            "text": "No.",
        }),
    );
    let outside = w.ideas.parent().unwrap().join("note.json");
    fs::rename(&target, &outside).unwrap();
    let link = w.notes(IDEA).join("1.json");
    symlink(&outside, &link).unwrap();

    let report = w.pass();
    assert_eq!(report.rejected, vec![link.clone()]);
    let (outcome, reason) = w.ingested(&link);
    assert_eq!(outcome, "rejected");
    assert!(reason.unwrap().contains("link"));
    assert!(!w.ideas.join(IDEA).join("interview.md").exists());
}

#[test]
fn idea_note_outside_a_sub_orchestrator_attempt_is_refused() {
    let w = world();

    // No live attempt in the parent chain.
    let none = w.writer(IDEA).unwrap_err().to_string();
    assert!(none.contains(NO_LIVE_ATTEMPT), "{none}");

    // The attempt in the chain is a task attempt, not a sub-orchestrator.
    w.live("T-0007-03/A1", Some("T-0007-03"), None);
    let task = w.writer(IDEA).unwrap_err().to_string();
    assert!(task.contains(NOT_THE_SUB_ORCHESTRATOR), "{task}");
    w.conn.execute("DELETE FROM attempts", []).unwrap();

    // The sub-orchestrator of another idea.
    w.live("I-0008/A1", None, Some(OTHER_IDEA));
    let other = w.writer(IDEA).unwrap_err().to_string();
    assert!(other.contains(NOT_THE_SUB_ORCHESTRATOR), "{other}");
    assert!(other.contains(OTHER_IDEA), "{other}");

    // No note file is written.
    assert!(!w.notes(IDEA).exists());
}

#[test]
fn ask_note_records_a_durable_open_question_and_interview_note_closes_it() {
    let mut w = world();
    let attempt = w.sub_orchestrator();
    let writer = w.writer(IDEA).unwrap();
    let ask = NoteBody::Ask {
        question: "Do retries apply to uploads?".into(),
    };
    let path = writer.write(ask.clone()).unwrap();
    let file: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(file["kind"], "ask");
    assert_eq!(file["text"], "Do retries apply to uploads?");

    let report = w.pass();
    assert_eq!(
        report.asked,
        vec![Note {
            idea: IDEA.into(),
            attempt: attempt.clone(),
            body: ask,
        }]
    );
    assert!(report.interview.is_empty());
    // An open question does not touch interview.md.
    assert!(!w.ideas.join(IDEA).join("interview.md").exists());
    let events = w.idea_notes();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, notes_ingest::QUESTION_ASKED);
    assert_eq!(events[0].1["question"], "Do retries apply to uploads?");
    assert_eq!(events[0].1["attempt"], attempt.as_str());
    assert_eq!(events[0].1["amendments"], 0);

    writer.write(interview_body()).unwrap();
    w.pass();
    let events = w.idea_notes();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[1].0, notes_ingest::QUESTION_ANSWERED);
    assert_eq!(events[1].1["question"], "Do retries apply to uploads?");
}
