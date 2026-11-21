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

pub const QUESTION_ASKED: &str = "interview question asked";

pub const QUESTION_ANSWERED: &str = "interview question answered";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeaNotes {
    pub idea: String,
    pub notes: Vec<NoteFileRead>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub interview: Vec<Note>,
    pub asked: Vec<Note>,
    pub answers: Vec<Note>,
    pub dismissals: Vec<Note>,
    pub rejected: Vec<PathBuf>,
}

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

fn amendments(ideas_dir: &Path, owner: &User, idea: &str) -> Result<usize, String> {
    let folder = IdeaFolder::new(ideas_dir, idea, owner.clone()).map_err(|e| e.to_string())?;
    match folder.read_amendments() {
        Ok(texts) => Ok(texts.len()),
        Err(FolderError::Missing { .. }) => Ok(0),
        Err(e) => Err(format!("the amendments of idea {idea} are unreadable: {e}")),
    }
}

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

fn check_attempt(tx: &Connection, note: &Note) -> Result<Result<(), String>> {
    Ok(match attempts_store::task_and_idea(tx, &note.attempt)? {
        Some((None, Some(idea))) if idea == note.idea => Ok(()),
        _ => Err(format!(
            "attempt {} is not the sub-orchestrator attempt of idea {}",
            note.attempt, note.idea
        )),
    })
}

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
    let entry = interview_entry(0, question, answer);
    let body = entry.strip_prefix("\n## Q0").unwrap_or(&entry);
    Ok(text
        .split("\n## Q")
        .skip(1)
        .any(|e| e.trim_start_matches(|c: char| c.is_ascii_digit()) == body))
}

fn key(path: &Path) -> String {
    path.display().to_string()
}

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
