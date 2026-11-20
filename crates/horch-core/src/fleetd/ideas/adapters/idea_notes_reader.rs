//! The note files of horch idea note: `ideas/I-NNNN/notes/<n>.json`, the
//! writer of the sub-orchestrator and the reader of fleetd.
//!
//! `docs/specs/fleetd/design.md`, "Ideas" (The interview; How a worker's
//! question is resolved, step 4), "Workspace layout and file protocol"
//! (protocol rules 1, 5 and 7), A35. Requirements: FM-31, RR-12, SR-6.
//!
//! - A note is an immutable message of class worker. It never uses the
//!   command channel or the request folder, and the writer writes no ledger
//!   row.
//! - The writer finds its own attempt by the parent chain of its process
//!   ([`own_attempt`]), and refuses a caller that is not the live
//!   sub-orchestrator attempt of the idea: [`NOT_THE_SUB_ORCHESTRATOR`].
//! - `notes/` is made with mode 0700 when it is missing. Each note is
//!   written 1 time with mode 0600: a temp file, fsync, then a hard link to
//!   the next free number.
//! - The reader reads each note with the protected file reader (SR-6), with
//!   the idea folder as its root, so a link in the place of `notes/` or of a
//!   note is refused. The idea of the payload must be the idea of the path.
//!
//! Windows gap: the platform gives no parent PID and no mode on Windows, so
//! the writer is refused there and the reader rejects each note.

use std::fs::DirBuilder;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::peer_process::ProcessFacts;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::ideas::domain::idea_folder::{check_idea_id, is_idea_id, FILE_LIMIT};
use crate::fleetd::task_execution::adapters::outbox_writer::{
    own_attempt, starting, LiveAttempt, ATTEMPT_POLL, ATTEMPT_WAIT,
};
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fsx::{self, Created, FsxError};

/// The folder of the notes in an idea folder.
pub const NOTES: &str = "notes";

/// The refusal of a process that is not the sub-orchestrator of the idea.
pub const NOT_THE_SUB_ORCHESTRATOR: &str = "this process is not the sub-orchestrator attempt of";

/// The mode of `notes/` and of each note.
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

/// The most note numbers that 1 write tries after the highest one it saw,
/// when other writers take them first.
const NUMBER_TRIES: u32 = 64;

/// How the sub-orchestrator handles the answer to a worker question
/// (design, "How a worker's question is resolved", step 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerCase {
    /// The approved assumptions or the spec cover it; the note cites the
    /// line.
    Covered,
    /// The answer changes an approved assumption: it goes to the operator.
    Revise,
    /// The question shows a flaw in the plan: the task is replanned.
    Replan,
    /// Nothing approved covers it: a new assumption, marked as an own
    /// decision.
    OwnDecision,
}

/// What 1 note records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteBody {
    /// 1 question of the interview and the answer of the operator. fleetd
    /// appends it to interview.md as `Q<n>`.
    Interview { question: String, answer: String },
    /// 1 open question of the interview that the operator has not answered
    /// yet. fleetd records it as a durable event before the idea waits on
    /// you (FM-29).
    Ask { question: String },
    /// The answer to the worker question `question`.
    Answer {
        question: String,
        case: AnswerCase,
        /// `<file>:<line>`: only with [`AnswerCase::Covered`], and required
        /// there.
        cite: Option<String>,
        text: String,
    },
    /// The sub-orchestrator dismisses the judge finding `finding`.
    Dismiss { finding: String, reason: String },
}

/// 1 valid note of an idea folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The idea of the folder.
    pub idea: String,
    /// The id of the `attempts` row of the sub-orchestrator that wrote it.
    pub attempt: String,
    pub body: NoteBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Interview,
    Ask,
    Answer,
    Dismiss,
}

/// The JSON form of a note. `question` is the interview question of an
/// `interview` note and the question id of an `answer` note; `text` is the
/// answer, the question of an `ask` note, or the reason of a `dismiss` note.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteFile {
    idea: String,
    attempt: String,
    kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    case: Option<AnswerCase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finding: Option<String>,
    text: String,
}

impl From<&Note> for NoteFile {
    fn from(note: &Note) -> NoteFile {
        let mut file = NoteFile {
            idea: note.idea.clone(),
            attempt: note.attempt.clone(),
            kind: Kind::Interview,
            question: None,
            case: None,
            cite: None,
            finding: None,
            text: String::new(),
        };
        match &note.body {
            NoteBody::Interview { question, answer } => {
                file.question = Some(question.clone());
                file.text = answer.clone();
            }
            NoteBody::Ask { question } => {
                file.kind = Kind::Ask;
                file.text = question.clone();
            }
            NoteBody::Answer {
                question,
                case,
                cite,
                text,
            } => {
                file.kind = Kind::Answer;
                file.question = Some(question.clone());
                file.case = Some(*case);
                file.cite = cite.clone();
                file.text = text.clone();
            }
            NoteBody::Dismiss { finding, reason } => {
                file.kind = Kind::Dismiss;
                file.finding = Some(finding.clone());
                file.text = reason.clone();
            }
        }
        file
    }
}

impl TryFrom<NoteFile> for Note {
    type Error = String;

    fn try_from(f: NoteFile) -> Result<Note, String> {
        let body = match (f.kind, f.question, f.case, f.cite, f.finding) {
            (Kind::Interview, Some(question), None, None, None) => NoteBody::Interview {
                question,
                answer: f.text,
            },
            (Kind::Ask, None, None, None, None) => NoteBody::Ask { question: f.text },
            (Kind::Answer, Some(question), Some(case), cite, None) => NoteBody::Answer {
                question,
                case,
                cite,
                text: f.text,
            },
            (Kind::Dismiss, None, None, None, Some(finding)) => NoteBody::Dismiss {
                finding,
                reason: f.text,
            },
            (kind, ..) => {
                return Err(format!(
                    "the keys do not match the kind {}",
                    kind_name(kind)
                ))
            }
        };
        let note = Note {
            idea: f.idea,
            attempt: f.attempt,
            body,
        };
        check(&note)?;
        Ok(note)
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Interview => "interview",
        Kind::Ask => "ask",
        Kind::Answer => "answer",
        Kind::Dismiss => "dismiss",
    }
}

/// The rules of a note that the JSON form does not hold.
fn check(note: &Note) -> Result<(), String> {
    if !is_idea_id(&note.idea) {
        return Err(format!("`{}` is not an idea id I-NNNN", note.idea));
    }
    let filled = |name: &str, value: &str| {
        if value.trim().is_empty() {
            Err(format!("{name} is empty"))
        } else {
            Ok(())
        }
    };
    filled("attempt", &note.attempt)?;
    match &note.body {
        NoteBody::Interview { question, answer } => {
            filled("the question", question)?;
            filled("the answer", answer)
        }
        NoteBody::Ask { question } => {
            filled("the question", question)?;
            check_ask(question)
        }
        NoteBody::Answer {
            question,
            case,
            cite,
            text,
        } => {
            filled("the question id", question)?;
            filled("the answer", text)?;
            match (case, cite) {
                (AnswerCase::Covered, Some(cite)) => filled("the citation", cite),
                (AnswerCase::Covered, None) => Err("a covered answer needs a citation".into()),
                (_, Some(_)) => Err("only a covered answer has a citation".into()),
                (_, None) => Ok(()),
            }
        }
        NoteBody::Dismiss { finding, reason } => {
            filled("the finding", finding)?;
            filled("the reason", reason)
        }
    }
}

/// The most characters of the question of an `ask` note. fleetd shows it
/// in 1 needs-you item.
pub const ASK_LIMIT: usize = 1000;

/// The question of an `ask` note is 1 line of at most [`ASK_LIMIT`]
/// characters with no control character, so the needs-you item that shows
/// it holds only that line.
fn check_ask(question: &str) -> Result<(), String> {
    if question.chars().any(char::is_control) {
        return Err("the question has a line break or a control character".into());
    }
    if question.chars().count() > ASK_LIMIT {
        return Err(format!(
            "the question is longer than {ASK_LIMIT} characters"
        ));
    }
    Ok(())
}

/// The writer of the notes of the sub-orchestrator of 1 idea.
#[derive(Debug)]
pub struct NoteWriter {
    notes: PathBuf,
    idea: String,
    attempt: String,
}

impl NoteWriter {
    /// The writer of process `pid` for `idea`, under `ideas`, the `ideas/`
    /// folder of the fleet root. `ledger` is the ledger, opened read-only
    /// by the caller; `facts` are the process facts. The 1 live attempt in
    /// the parent chain of `pid` must be the sub-orchestrator attempt of
    /// `idea`, and the idea folder must exist. While an attempt has no
    /// recorded wrapper yet, a process with no own attempt reads the ledger
    /// again, for at most the start timeout of a launch, as a task command
    /// does.
    pub fn of_process(
        pid: u32,
        ideas: &Path,
        idea: &str,
        ledger: &Connection,
        facts: &dyn ProcessFacts,
    ) -> Result<NoteWriter> {
        if !is_idea_id(idea) {
            bail!("`{idea}` is not an idea id I-NNNN");
        }
        let until = Instant::now() + ATTEMPT_WAIT;
        let (live, own) = loop {
            let live = live_attempts(ledger)?;
            match own_attempt(pid, &live.attempts, facts) {
                Ok(own) => break (live, own),
                Err(e) if e.detail.is_none() && Instant::now() < until && starting(ledger)? => {
                    std::thread::sleep(ATTEMPT_POLL);
                }
                Err(e) => return Err(e.into()),
            }
        };
        let of_idea = live
            .ideas
            .iter()
            .find(|(attempt, _)| *attempt == own.attempt_id)
            .and_then(|(_, of)| of.as_deref());
        match of_idea {
            Some(of) if of == idea => {}
            Some(of) => bail!(
                "{NOT_THE_SUB_ORCHESTRATOR} {idea}: attempt {} is the sub-orchestrator of {of}",
                own.attempt_id
            ),
            None => bail!(
                "{NOT_THE_SUB_ORCHESTRATOR} {idea}: attempt {} is a task attempt",
                own.attempt_id
            ),
        }
        let folder = ideas.join(idea);
        let meta = std::fs::symlink_metadata(&folder)
            .with_context(|| format!("the folder of idea {idea}: {}", folder.display()))?;
        if !meta.is_dir() {
            bail!(
                "the folder of idea {idea} is not a folder: {}",
                folder.display()
            );
        }
        Ok(NoteWriter {
            notes: folder.join(NOTES),
            idea: idea.to_string(),
            attempt: own.attempt_id,
        })
    }

    /// Write `body` as the next note of the idea, 1 time. Returns its path.
    pub fn write(&self, body: NoteBody) -> Result<PathBuf> {
        let note = Note {
            idea: self.idea.clone(),
            attempt: self.attempt.clone(),
            body,
        };
        check(&note).map_err(anyhow::Error::msg)?;
        let bytes = serde_json::to_vec_pretty(&NoteFile::from(&note))?;
        make_dir(&self.notes)?;
        let first = numbers(&self.notes)?.last().map_or(1, |n| n + 1);
        for n in first..first + NUMBER_TRIES {
            let path = self.notes.join(note_name(n));
            match fsx::create_immutable(&path, &bytes, FILE_MODE) {
                Ok(Created::Written | Created::AlreadyIdentical) => return Ok(path),
                Err(FsxError::Conflict { .. }) => continue,
                Err(e) => return Err(e.into()),
            }
        }
        bail!(
            "no free note number in {} after {NUMBER_TRIES} tries",
            self.notes.display()
        )
    }
}

/// The live attempts with a recorded wrapper, and the idea of each. A task
/// attempt is in the list too, so a process inside a task attempt finds
/// that attempt and is refused.
struct Live {
    attempts: Vec<LiveAttempt>,
    /// The attempt id and its idea, for each attempt with no task.
    ideas: Vec<(String, Option<String>)>,
}

fn live_attempts(conn: &Connection) -> Result<Live> {
    let mut live = Live {
        attempts: Vec::new(),
        ideas: Vec::new(),
    };
    for w in attempts_store::live_wrappers(conn)? {
        if w.task_id.is_none() {
            live.ideas.push((w.id.clone(), w.idea_id));
        }
        live.attempts.push(LiveAttempt {
            attempt: AttemptRef {
                task_id: w.task_id.unwrap_or_default(),
                attempt_id: w.id,
            },
            wrapper_pid: w.pid,
            wrapper_start: w.pid_start,
        });
    }
    Ok(live)
}

/// Make `dir` with mode 0700 when it is missing. A link or a file in its
/// place is refused.
fn make_dir(dir: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    // The writer runs only where the OS has modes: on Windows it finds no
    // own attempt first.
    let _ = horch_platform::fs::create_dir_mode(&mut builder, DIR_MODE);
    match builder.create(dir) {
        Ok(()) => {
            // The umask may have cleared bits of the mode.
            horch_platform::fs::set_mode(dir, DIR_MODE)
                .with_context(|| format!("set the mode of {}", dir.display()))?;
            if let Some(parent) = dir.parent() {
                horch_platform::fs::sync_dir(parent)
                    .with_context(|| format!("sync {}", parent.display()))?;
            }
            Ok(())
        }
        // Another process made it first, or an earlier note.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(dir)?.is_dir() {
                Ok(())
            } else {
                bail!("{} is not a folder", dir.display())
            }
        }
        Err(e) => Err(e).with_context(|| format!("make {}", dir.display())),
    }
}

/// The name of note `n`.
fn note_name(n: u32) -> String {
    format!("{n}.json")
}

/// The number of a note file name: `<n>.json`, with no leading zero. Each
/// other name, such as a temp file, is not a note.
fn note_number(name: &str) -> Option<u32> {
    let n: u32 = name.strip_suffix(".json")?.parse().ok()?;
    (note_name(n) == name).then_some(n)
}

/// The number of each note in `dir`, in order. A missing folder has none.
fn numbers(dir: &Path) -> Result<Vec<u32>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("list {}", dir.display())),
    };
    let mut numbers = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("list {}", dir.display()))?;
        if let Some(n) = note_number(&entry.file_name().to_string_lossy()) {
            numbers.push(n);
        }
    }
    numbers.sort_unstable();
    Ok(numbers)
}

/// What the reader found in 1 note file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteRead {
    Valid {
        sha256: String,
        note: Note,
    },
    /// The file failed a check. `sha256` is known when the bytes were read.
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
}

/// 1 note file and what the reader found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteFileRead {
    pub path: PathBuf,
    pub read: NoteRead,
}

/// Read each note of `idea` under `ideas`, in the order of its number. A
/// note whose path `done` names is not read: the ledger has it. `owner` is
/// the user that owns the ledger. A missing folder gives no note.
pub fn read_notes(
    ideas: &Path,
    idea: &str,
    owner: &User,
    done: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<Vec<NoteFileRead>> {
    let folder = ideas.join(idea);
    let notes = folder.join(NOTES);
    let rule = Rule {
        root: &folder,
        owner,
        limit: FILE_LIMIT,
    };
    let mut out = Vec::new();
    for n in numbers(&notes)? {
        let path = notes.join(note_name(n));
        if done(&path)? {
            continue;
        }
        let read = read_note(&path, &rule, idea);
        out.push(NoteFileRead { path, read });
    }
    Ok(out)
}

/// Read and check 1 note of the folder of `idea`.
fn read_note(path: &Path, rule: &Rule<'_>, idea: &str) -> NoteRead {
    let bytes = match read_protected(path, rule) {
        Ok(bytes) => bytes,
        Err(r) => {
            return NoteRead::Rejected {
                sha256: None,
                reason: r.reason,
            }
        }
    };
    let sha256 = crate::digest::sha256_bytes(&bytes).hex();
    let parsed = serde_json::from_slice::<NoteFile>(&bytes)
        .map_err(|e| format!("the note is not valid JSON of a note: {e}"))
        .and_then(Note::try_from)
        .and_then(|note| {
            check_idea_id("the note", &note.idea, idea).map_err(|e| e.reason)?;
            Ok(note)
        });
    match parsed {
        Ok(note) => NoteRead::Valid { sha256, note },
        Err(reason) => NoteRead::Rejected {
            sha256: Some(sha256),
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
    use crate::taskledger::schema;

    const COMMAND: u32 = 4300;
    const WRAPPER: u32 = 4200;
    const WRAPPER_START: i64 = 77;

    struct Chain;

    impl ProcessFacts for Chain {
        fn parent(&self, pid: u32) -> Option<u32> {
            Some(if pid == COMMAND { WRAPPER } else { 1 })
        }

        fn start_time(&self, pid: u32) -> Option<u64> {
            (pid == WRAPPER).then_some(WRAPPER_START as u64)
        }

        fn in_group(&self, _pid: u32, _leader: u32, _leader_start: u64) -> Option<bool> {
            None
        }
    }

    #[test]
    fn a_note_waits_while_its_sub_orchestrator_attempt_has_no_wrapper() {
        const IDEA: &str = "I-0007";
        let temp = tempfile::tempdir().unwrap();
        let ideas = temp.path().join("ideas");
        let db = temp.path().join("ledger.sqlite3");
        let mut conn = Connection::open(&db).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        IdeaFolder::create(&ideas, IDEA, User::current().unwrap()).unwrap();
        conn.execute(
            "INSERT INTO attempts (id, idea_id, worker_id, state, started_at)
             VALUES ('I-0007/A1', ?1, 'idea-lead-1', 'launching', '2026-11-20T00:00:00Z')",
            [IDEA],
        )
        .unwrap();
        let goes_live = std::thread::spawn(move || {
            std::thread::sleep(ATTEMPT_POLL * 2);
            let conn = Connection::open(&db).unwrap();
            conn.execute(
                "UPDATE attempts SET state = 'live', pid = ?1, pid_start = ?2",
                rusqlite::params![i64::from(WRAPPER), WRAPPER_START],
            )
            .unwrap();
        });
        let writer = NoteWriter::of_process(COMMAND, &ideas, IDEA, &conn, &Chain);
        goes_live.join().unwrap();
        assert_eq!(writer.unwrap().attempt, "I-0007/A1");
    }

    #[test]
    fn a_note_name_is_a_number_and_json() {
        assert_eq!(note_number("1.json"), Some(1));
        assert_eq!(note_number("12.json"), Some(12));
        assert_eq!(note_number("01.json"), None);
        assert_eq!(note_number(".1.json.new-1-abc"), None);
        assert_eq!(note_number("1.json.tmp"), None);
    }

    #[test]
    fn keys_that_do_not_match_the_kind_are_refused() {
        let file = |kind: Kind, case: Option<AnswerCase>, cite: Option<&str>| NoteFile {
            idea: "I-0007".into(),
            attempt: "I-0007/A1".into(),
            kind,
            question: Some("T-0007-03/A1/Q1".into()),
            case,
            cite: cite.map(str::to_string),
            finding: None,
            text: "an answer".into(),
        };
        assert!(Note::try_from(file(Kind::Answer, None, None)).is_err());
        assert!(Note::try_from(file(Kind::Answer, Some(AnswerCase::Covered), None)).is_err());
        assert!(Note::try_from(file(Kind::Answer, Some(AnswerCase::Replan), Some("a:1"))).is_err());
        assert!(Note::try_from(file(Kind::Interview, Some(AnswerCase::Replan), None)).is_err());
        assert!(Note::try_from(file(Kind::Dismiss, None, None)).is_err());
        assert!(Note::try_from(file(Kind::Answer, Some(AnswerCase::Covered), Some("a:1"))).is_ok());
    }
}
