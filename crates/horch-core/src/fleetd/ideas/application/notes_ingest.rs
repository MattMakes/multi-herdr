//! The ingest of the notes of horch idea note: each note file of an idea
//! folder is ingested 1 time.
//!
//! `docs/specs/fleetd/design.md`, "Ideas" (The interview; How a worker's
//! question is resolved, step 4), "Workspace layout and file protocol"
//! (protocol rules 2 and 7), A35. Requirements: FM-31, RR-12, SR-6.
//!
//! - Part 1 of a pass calls [`read`] for the ideas that it scans. A path
//!   with an `ingested` row is not read again.
//! - Part 2 calls [`ingest`] in its transaction. Each note gets 1
//!   `ingested` row, by its path and its sha256.
//! - A note is accepted only when its attempt is the sub-orchestrator
//!   attempt of its idea in the ledger.
//! - An `interview` note appends `Q<n>` and its answer to interview.md
//!   before its row is written. The append is skipped when interview.md
//!   holds the entry already, so a pass whose transaction does not commit
//!   appends nothing again. It records the durable event
//!   [`QUESTION_ANSWERED`], which closes the open interview question.
//! - An `ask` note records the durable event [`QUESTION_ASKED`] with the
//!   question and the count of amendments. Only this event lets a
//!   clarifying idea wait on you (FM-29): an idle pane alone does not.
//! - An `answer` note and a `dismiss` note are in the [`Report`] of the 1
//!   pass that accepts them: the caller gives each answer to the question
//!   flow (ticket 32) and each dismiss to the judge pair (ticket 41), in the
//!   same transaction.
//! - A rejected note gets its row with the reason, and changes nothing
//!   else. fleetd stays healthy.

use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause, DoorError};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::idea_notes_reader::{
    read_notes, Note, NoteBody, NoteFileRead, NoteRead,
};
use crate::fleetd::ideas::domain::idea_folder::{interview_entry, FILE_LIMIT, INTERVIEW_MD};
use crate::fleetd::task_execution::adapters::ingested_store as store;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::util;

/// The cause of the note event of an open interview question. Its data
/// holds `attempt`, `question` and `amendments`, the count of amendments
/// when the question was asked.
pub const QUESTION_ASKED: &str = "interview question asked";

/// The cause of the note event of an answered interview question. Its data
/// holds `attempt` and `question`.
pub const QUESTION_ANSWERED: &str = "interview question answered";

/// The notes that part 1 read from the folder of 1 idea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeaNotes {
    pub idea: String,
    pub notes: Vec<NoteFileRead>,
}

/// What 1 ingest did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The interview notes that are in interview.md now.
    pub interview: Vec<Note>,
    /// The open interview questions that this ingest recorded.
    pub asked: Vec<Note>,
    /// The answer notes, for the question flow.
    pub answers: Vec<Note>,
    /// The dismiss notes, for the judge pair.
    pub dismissals: Vec<Note>,
    /// The notes that this ingest recorded as rejected.
    pub rejected: Vec<PathBuf>,
}

/// Part 1: read the new notes of each of `ideas` under `ideas_dir`, the
/// `ideas/` folder of the fleet root. `owner` is the user that owns the
/// ledger. A note with an `ingested` row is not read. An idea with no new
/// note is left out.
pub fn read(
    conn: &Connection,
    ideas_dir: &Path,
    ideas: &[String],
    owner: &User,
) -> Result<Vec<IdeaNotes>> {
    let mut out = Vec::new();
    for idea in ideas {
        let mut done = |path: &Path| store::has(conn, &key(path));
        let notes = read_notes(ideas_dir, idea, owner, &mut done)?;
        if !notes.is_empty() {
            out.push(IdeaNotes {
                idea: idea.clone(),
                notes,
            });
        }
    }
    Ok(out)
}

/// Part 2: record each note of `reads` 1 time and act on each valid note,
/// in the open transaction `tx`. The fleetd of generation `guard` writes.
pub fn ingest(
    tx: &Connection,
    guard: Option<Generation>,
    ideas_dir: &Path,
    owner: &User,
    reads: &[IdeaNotes],
) -> Result<Report> {
    check_pass(tx, guard)?;
    let at = util::now();
    let mut report = Report::default();
    for file in reads.iter().flat_map(|r| &r.notes) {
        let path = key(&file.path);
        if store::has(tx, &path)? {
            continue;
        }
        let accepted = match &file.read {
            NoteRead::Rejected { sha256, reason } => Err((sha256.as_deref(), reason.clone())),
            NoteRead::Valid { sha256, note } => match accept(tx, guard, ideas_dir, owner, note)? {
                Ok(()) => Ok((sha256.as_str(), note)),
                Err(reason) => Err((Some(sha256.as_str()), reason)),
            },
        };
        match accepted {
            Ok((sha256, note)) => {
                store::record_message(tx, &path, Some(sha256), store::ACCEPTED, None, &at)?;
                let to = match note.body {
                    NoteBody::Interview { .. } => &mut report.interview,
                    NoteBody::Ask { .. } => &mut report.asked,
                    NoteBody::Answer { .. } => &mut report.answers,
                    NoteBody::Dismiss { .. } => &mut report.dismissals,
                };
                to.push(note.clone());
            }
            Err((sha256, reason)) => {
                store::record_message(tx, &path, sha256, store::REJECTED, Some(&reason), &at)?;
                report.rejected.push(file.path.clone());
            }
        }
    }
    Ok(report)
}

/// Check the attempt of `note`, append an interview note to interview.md,
/// and record the question event of an `ask` or an `interview` note. The
/// inner error is the reason of the rejection.
fn accept(
    tx: &Connection,
    guard: Option<Generation>,
    ideas_dir: &Path,
    owner: &User,
    note: &Note,
) -> Result<Result<(), String>> {
    if let Err(reason) = check_attempt(tx, note)? {
        return Ok(Err(reason));
    }
    let (cause, data) = match &note.body {
        NoteBody::Ask { question } => {
            // An unreadable amendments/ rejects this note only: the pass goes on.
            let count = match amendments(ideas_dir, owner, &note.idea) {
                Ok(count) => count,
                Err(reason) => return Ok(Err(reason)),
            };
            let data =
                json!({ "attempt": note.attempt, "question": question, "amendments": count });
            (QUESTION_ASKED, data)
        }
        NoteBody::Interview { question, answer } => {
            if let Err(reason) = append_once(ideas_dir, owner, note, question, answer) {
                return Ok(Err(reason));
            }
            let data = json!({ "attempt": note.attempt, "question": question });
            (QUESTION_ANSWERED, data)
        }
        NoteBody::Answer { .. } | NoteBody::Dismiss { .. } => return Ok(Ok(())),
    };
    let by = Cause {
        actor: Actor::Horch,
        text: cause,
    };
    record(
        tx,
        guard,
        EventKind::Note,
        Entity::Idea,
        &note.idea,
        by,
        &data,
    )?;
    Ok(Ok(()))
}

/// The count of amendments of `idea`. The error is the reason of the
/// rejection.
fn amendments(ideas_dir: &Path, owner: &User, idea: &str) -> Result<usize, String> {
    let folder = IdeaFolder::new(ideas_dir, idea, owner.clone()).map_err(|e| e.to_string())?;
    match folder.read_amendments() {
        Ok(texts) => Ok(texts.len()),
        Err(FolderError::Missing { .. }) => Ok(0),
        Err(e) => Err(format!("the amendments of idea {idea} are unreadable: {e}")),
    }
}

/// Append `question` and `answer` to interview.md of the idea of `note`,
/// unless it holds that entry already. The error is the reason of the
/// rejection.
fn append_once(
    ideas_dir: &Path,
    owner: &User,
    note: &Note,
    question: &str,
    answer: &str,
) -> Result<(), String> {
    let folder =
        IdeaFolder::new(ideas_dir, &note.idea, owner.clone()).map_err(|e| e.to_string())?;
    if !holds_entry(&folder, owner, question, answer)? {
        folder
            .append_interview(question, answer)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The attempt of `note` is the sub-orchestrator attempt of its idea. The
/// inner error is the reason of the rejection.
fn check_attempt(tx: &Connection, note: &Note) -> Result<Result<(), String>> {
    Ok(match attempts_store::task_and_idea(tx, &note.attempt)? {
        Some((None, Some(idea))) if idea == note.idea => Ok(()),
        _ => Err(format!(
            "attempt {} is not the sub-orchestrator attempt of idea {}",
            note.attempt, note.idea
        )),
    })
}

/// Whether interview.md of `folder` holds the entry of `question` and
/// `answer` already. No interview.md holds no entry.
fn holds_entry(
    folder: &IdeaFolder,
    owner: &User,
    question: &str,
    answer: &str,
) -> Result<bool, String> {
    let path = folder.dir().join(INTERVIEW_MD);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(false);
    }
    let rule = Rule {
        root: folder.dir(),
        owner,
        limit: FILE_LIMIT,
    };
    let bytes = read_protected(&path, &rule)
        .map_err(|r| format!("{INTERVIEW_MD} is rejected: {}", r.reason))?;
    let text = String::from_utf8_lossy(&bytes);
    // The entry without its `## Q<n>` line: the text that this note adds.
    let entry = interview_entry(0, question, answer);
    let body = entry.strip_prefix("\n## Q0").unwrap_or(&entry);
    Ok(text
        .split("\n## Q")
        .skip(1)
        .any(|e| e.trim_start_matches(|c: char| c.is_ascii_digit()) == body))
}

/// The `ingested` key of `path`.
fn key(path: &Path) -> String {
    path.display().to_string()
}

/// Refuse a connection with no open transaction, and a pass of another
/// generation, before the first write.
fn check_pass(tx: &Connection, guard: Option<Generation>) -> Result<()> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction.into());
    }
    if let Some(held) = guard {
        let found = door::generation(tx)?;
        if found != Some(held) {
            return Err(DoorError::GenerationLost { held, found }.into());
        }
    }
    Ok(())
}
