//! `AnswerQuestion`: the answer of a worker question in its 4 cases, and the
//! decision log of the feature.
//!
//! `docs/specs/fleetd/design.md`, "Ideas" (How a worker's question is
//! resolved, steps 4 to 6), D13, RR-12; `docs/tickets/fleetd-p6/32-answers.md`;
//! MAP.md of fleetd-p6, Q15.
//!
//! The sub-orchestrator of the feature answers with an `answer` note
//! (ticket 25). [`answer_question`] runs in the transaction of the ingest
//! of the note:
//!
//! - `covered`: the note cites `[I-NNNN/]<file>:<line>`, a line of
//!   assumptions.md or spec.md of an idea of the feature; with no idea, of
//!   the idea of the task. fleetd checks that the line exists. Then the
//!   question is answered and the task goes from `blocked_on_user` to
//!   `ready` ([`questions::answered`]).
//! - `revise`: the idea of the task goes to `awaiting_approval`, and the
//!   question goes to `escalated`: it went to you, so the question flow
//!   does not ring or escalate it again. Only its task waits; the other
//!   tasks of the idea go on. When you approve the idea,
//!   [`settle_revisions`] answers the question with the note text.
//! - `replan`: the question stays open and its task waits for the replan
//!   file of the sub-orchestrator ([`pending_replans`],
//!   [`super::replan::replan_task`]). The time limit of the question bounds
//!   the wait: past it the question flow escalates it to you.
//! - `own_decision`: fleetd appends `<n>. [assumed, own decision] <text>
//!   (<question id>)` to assumptions.md of the idea of the task, 1 time
//!   only (D13). It needs no new approval: when you approved assumptions.md
//!   as it was before the decision, the approval follows the new hash, so
//!   I7 still holds. Then the question is answered.
//!
//! An accepted answer records 1 `question` event of its task with the case
//! and the files it intends (RR-8): the entry of
//! `features/F-NNNN/decisions.md`, and for an own decision the new
//! assumptions.md and approval.json. fleetd writes no file in the
//! transaction. It writes them after the commit ([`Recorded::write`]).
//! After a crash between the commit and the writes,
//! [`pending_answer_files`] gives the writes again from the ledger (FM-11). A refused answer leaves the question as it
//! was, records 1 `note` event of the task with the reason, and rings the
//! live process of the feature again with the reason.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::idea_notes_reader::{AnswerCase, Note, NoteBody};
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::adapters::questions_store as store;
use crate::fleetd::ideas::application::approval::{
    record_approval, trusted_approval, ApprovalFile,
};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::ideas::application::question_flow::Ring;
use crate::fleetd::ideas::application::questions::{self, asked_text};
use crate::fleetd::ideas::domain::answer::{
    entry_heading, entry_text, line_exists, one_line, own_decision, parse_cite, with_entry, Case,
    Entry, DECISIONS_LIMIT, DECISIONS_MD, FEATURES,
};
use crate::fleetd::ideas::domain::approval::{
    approved_target, Approval, AWAITING_APPROVAL, BY_OPERATOR, PAST_APPROVAL, REVISED_TO,
};
use crate::fleetd::ideas::domain::idea_folder::{ASSUMPTIONS_MD, FILE_LIMIT};
use crate::fleetd::ideas::domain::questions::{Question, State};
use crate::fleetd::task_execution::adapters::sqlite as attempts;
use crate::fleetd::task_execution::application::planned_tasks::committed_task;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fsx;
use crate::taskledger::model::{Actor, EventKind, TaskStatus};
use crate::taskledger::{tasks, util};

/// The folder of the ideas in the fleet root.
const IDEAS: &str = "ideas";

/// The mode of the decision log.
const DECISIONS_MODE: u32 = 0o600;

/// What [`answer_question`] needs besides the ledger.
pub struct Answering<'a> {
    /// The generation of the fleetd that writes.
    pub guard: Option<Generation>,
    /// The fleet clock of the pass, for a doorbell.
    pub clock: FleetSeconds,
    /// The fleet root: it holds `ideas/` and `features/`.
    pub root: &'a Path,
    /// The user that owns the ledger, and so each file that fleetd reads.
    pub owner: &'a User,
}

/// What [`answer_question`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    Recorded(Recorded),
    Refused(Refused),
}

/// An accepted answer. Its files are written after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub question: String,
    pub case: Case,
    pub decision: DecisionLog,
    /// The approval.json of the idea that follows an own decision: (idea,
    /// file).
    pub approval: Option<(String, ApprovalFile)>,
    /// The assumptions.md of the idea with the own decision.
    pub assumptions: Option<AssumptionsWrite>,
}

impl Recorded {
    /// Write the files of the answer under the fleet root `root`:
    /// assumptions.md, approval.json, then the entry of the decision log.
    /// Each write is safe to repeat.
    pub fn write(&self, root: &Path, owner: &User) -> Result<()> {
        if let Some(a) = &self.assumptions {
            a.write(root, owner)?;
        }
        if let Some((idea, file)) = &self.approval {
            file.write(&IdeaFolder::new(&root.join(IDEAS), idea, owner.clone())?)?;
        }
        self.decision.write(root, owner)
    }

    fn effects(&self, idea: &str) -> Value {
        json!({
            "idea": idea,
            "feature": self.decision.feature,
            "heading": self.decision.heading,
            "entry": self.decision.text,
            "assumptions": self.assumptions.as_ref().map(|a| json!({
                "base_sha256": a.base_sha256,
                "text": a.text,
            })),
            "approval": self.approval.as_ref().map(|(_, f)| json!({
                "text": f.text,
                "sha256": f.sha256,
            })),
        })
    }

    fn of_event(data: &Value) -> Option<Recorded> {
        let effects = &data["effects"];
        let s = |v: &Value| v.as_str().map(str::to_string);
        let idea = s(&effects["idea"])?;
        let assumptions = match &effects["assumptions"] {
            Value::Null => None,
            a => Some(AssumptionsWrite {
                idea: idea.clone(),
                base_sha256: s(&a["base_sha256"]),
                text: s(&a["text"])?,
            }),
        };
        let approval = match &effects["approval"] {
            Value::Null => None,
            a => Some((
                idea.clone(),
                ApprovalFile {
                    text: s(&a["text"])?,
                    sha256: s(&a["sha256"])?,
                },
            )),
        };
        Some(Recorded {
            question: s(&data["question_id"])?,
            case: data["case"].as_str().and_then(Case::parse)?,
            decision: DecisionLog {
                feature: s(&effects["feature"])?,
                heading: s(&effects["heading"])?,
                text: s(&effects["entry"])?,
            },
            approval,
            assumptions,
        })
    }
}

/// The assumptions.md that an own decision intends, to write after the
/// commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssumptionsWrite {
    pub idea: String,
    /// The sha256 of the assumptions.md that the decision appends to, or
    /// `None` when the idea had no assumptions.md.
    pub base_sha256: Option<String>,
    /// The new assumptions.md.
    pub text: String,
}

impl AssumptionsWrite {
    /// Replace assumptions.md with the new text when it is still the file
    /// that the decision appends to. Otherwise the file holds the decision
    /// already, or it changed since, and this write does nothing.
    pub fn write(&self, root: &Path, owner: &User) -> Result<()> {
        let folder = IdeaFolder::new(&root.join(IDEAS), &self.idea, owner.clone())?;
        if assumptions_sha(&folder)? == self.base_sha256 {
            folder.write_assumptions(&self.text)?;
        }
        Ok(())
    }
}

/// A refused answer: the question is as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub question: String,
    /// What the sub-orchestrator reads.
    pub reason: String,
    /// The doorbell to type after the commit, when the feature has a live
    /// process in a pane and the question waits for an answer.
    pub ring: Option<Ring>,
}

/// 1 entry of `features/F-NNNN/decisions.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionLog {
    pub feature: String,
    pub heading: String,
    pub text: String,
}

impl DecisionLog {
    /// The decision log of the feature, under the fleet root `root`.
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(FEATURES).join(&self.feature).join(DECISIONS_MD)
    }

    /// Append the entry to the decision log, atomically, unless the log
    /// holds it already. The log is read with the checks of SR-6.
    pub fn write(&self, root: &Path, owner: &User) -> Result<()> {
        let features = root.join(FEATURES);
        fsx::ensure_private_dir(&features)?;
        fsx::ensure_private_dir(&features.join(&self.feature))?;
        let Some(new) = self.with_entry(root, owner)? else {
            return Ok(());
        };
        Ok(fsx::write_atomic(
            &self.path(root),
            new.as_bytes(),
            DECISIONS_MODE,
        )?)
    }

    fn with_entry(&self, root: &Path, owner: &User) -> Result<Option<String>> {
        let path = self.path(root);
        let file = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            _ => {
                let rule = Rule {
                    root: &root.join(FEATURES).join(&self.feature),
                    owner,
                    limit: DECISIONS_LIMIT,
                };
                let bytes = read_protected(&path, &rule)
                    .map_err(|r| anyhow::anyhow!("{} is refused: {}", path.display(), r.reason))?;
                Some(String::from_utf8(bytes).context("the decision log is not UTF-8 text")?)
            }
        };
        Ok(with_entry(
            file.as_deref(),
            &self.feature,
            &self.heading,
            &self.text,
        ))
    }
}

/// A check of an answer: the error is the reason of the refusal.
type Checked<T> = std::result::Result<T, String>;

/// `AnswerQuestion`: the answer note `note` of the sub-orchestrator, in the
/// open transaction `tx`. A note that is not an answer is an error.
pub fn answer_question(tx: &Connection, ctx: &Answering<'_>, note: &Note) -> Result<Answered> {
    let NoteBody::Answer {
        question,
        case,
        cite,
        text,
    } = &note.body
    else {
        bail!("the note of idea {} is not an answer", note.idea);
    };
    let case = case_of(*case);
    let Some(q) = store::get(tx, question)? else {
        return Ok(quiet(
            question,
            format!("no question {question} in the ledger"),
        ));
    };
    if q.state == State::Answered {
        return Ok(quiet(
            &q.id,
            format!("question {} is answered already", q.id),
        ));
    }
    let writer = idea_row(tx, &note.idea)?;
    if writer.feature_id != q.feature_id {
        let reason = format!(
            "question {} is of feature {}, not of feature {} of idea {}",
            q.id, q.feature_id, writer.feature_id, writer.id
        );
        return refuse(tx, ctx, &q, reason);
    }
    let idea = task_idea(tx, &q)?;
    let checked = match case {
        Case::Covered => {
            covered(tx, ctx, &q, &idea, cite.as_deref())?.map(|()| OwnDecisionFiles::default())
        }
        Case::Revise => revise(tx, ctx, &q, &idea, text)?.map(|()| OwnDecisionFiles::default()),
        Case::Replan => replan(tx, &q)?.map(|()| OwnDecisionFiles::default()),
        Case::OwnDecision => own(tx, ctx, &q, &idea, text)?,
    };
    let own = match checked {
        Ok(own) => own,
        Err(reason) => return refuse(tx, ctx, &q, reason),
    };
    let asked = asked_text(tx, &q)?;
    let entry = Entry {
        question_id: &q.id,
        case,
        task_id: &q.task_id,
        idea_id: &idea.id,
        question: asked.as_deref(),
        answer: text,
        cite: cite.as_deref(),
    };
    let recorded = Recorded {
        question: q.id.clone(),
        case,
        decision: DecisionLog {
            feature: q.feature_id.clone(),
            heading: entry_heading(&q.id, case),
            text: entry_text(&entry),
        },
        approval: own.approval.map(|file| (idea.id.clone(), file)),
        assumptions: own.assumptions,
    };
    let why = format!(
        "question {} is answered by its owner {}: {}",
        q.id,
        q.owner,
        case.as_str()
    );
    let data = json!({
        "question_id": q.id,
        "case": case.as_str(),
        "cite": cite,
        "answer": text,
        "effects": recorded.effects(&idea.id),
    });
    record(
        tx,
        ctx.guard,
        EventKind::Question,
        Entity::Task,
        &q.task_id,
        by_fleetd(&why),
        &data,
    )?;
    if matches!(case, Case::Covered | Case::OwnDecision) {
        questions::answered(tx, &q, Some(text))?;
    }
    Ok(Answered::Recorded(recorded))
}

/// The files of each committed answer that are not written yet, from the
/// `question` events of the ledger, oldest first (RR-8, FM-11). Each
/// [`Recorded`] holds only the writes that are still pending: write them in
/// this order after a crash between the commit of an answer and its
/// [`Recorded::write`].
pub fn pending_answer_files(conn: &Connection, root: &Path, owner: &User) -> Result<Vec<Recorded>> {
    let mut stmt = conn.prepare(
        "SELECT data_json FROM events WHERE kind = ?1 AND entity = ?2
           AND json_extract(data_json, '$.effects') IS NOT NULL
         ORDER BY event_id",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![EventKind::Question.as_str(), Entity::Task.as_str()],
        |r| r.get::<_, String>(0),
    )?;
    let mut sha_after_pending: HashMap<String, Option<String>> = HashMap::new();
    let mut out = Vec::new();
    for raw in rows {
        let data: Value = serde_json::from_str(&raw?)?;
        let Some(mut r) = Recorded::of_event(&data) else {
            continue;
        };
        let decision = r.decision.with_entry(root, owner)?.is_some();
        r.assumptions = match r.assumptions.take() {
            Some(a) => {
                let sha = match sha_after_pending.get(&a.idea) {
                    Some(sha) => sha.clone(),
                    None => assumptions_sha(&IdeaFolder::new(
                        &root.join(IDEAS),
                        &a.idea,
                        owner.clone(),
                    )?)?,
                };
                let pending = sha == a.base_sha256;
                let after = if pending {
                    Some(sha256_bytes(a.text.as_bytes()).hex())
                } else {
                    sha
                };
                sha_after_pending.insert(a.idea.clone(), after);
                pending.then_some(a)
            }
            None => None,
        };
        r.approval = match r.approval.take() {
            Some((idea, file)) if approval_pending(conn, root, owner, &idea, &file)? => {
                Some((idea, file))
            }
            _ => None,
        };
        if decision || r.assumptions.is_some() || r.approval.is_some() {
            out.push(r);
        }
    }
    Ok(out)
}

fn approval_pending(
    conn: &Connection,
    root: &Path,
    owner: &User,
    idea: &str,
    file: &ApprovalFile,
) -> Result<bool> {
    if idea_row(conn, idea)?.approval_sha256.as_deref() != Some(file.sha256.as_str()) {
        return Ok(false);
    }
    let folder = IdeaFolder::new(&root.join(IDEAS), idea, owner.clone())?;
    match folder.read_approval() {
        Ok(text) => Ok(text != file.text),
        Err(FolderError::Missing { .. }) => Ok(true),
        Err(e) => Err(e.into()),
    }
}

fn assumptions_sha(folder: &IdeaFolder) -> Result<Option<String>> {
    match folder.sha256_of(ASSUMPTIONS_MD) {
        Ok(sha) => Ok(Some(sha)),
        Err(FolderError::Missing { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Default)]
struct OwnDecisionFiles {
    approval: Option<ApprovalFile>,
    assumptions: Option<AssumptionsWrite>,
}

/// Each question that went to you with a `revise` answer and whose idea you
/// approved since: it is answered with the text of the answer, and its task
/// goes on. The ids of the questions, in the open transaction `tx`.
pub fn settle_revisions(tx: &Connection) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for q in store::unanswered(tx)? {
        if q.state != State::Escalated {
            continue;
        }
        let Some((Case::Revise, answer)) = last_case(tx, &q)? else {
            continue;
        };
        if !PAST_APPROVAL.contains(&task_idea(tx, &q)?.state.as_str()) {
            continue;
        }
        questions::answered(tx, &q, Some(&answer))?;
        out.push(q.id);
    }
    Ok(out)
}

/// Each task in `blocked_on_user` whose question has a `replan` answer and
/// waits for its replan file, each once, oldest question first.
pub fn pending_replans(conn: &Connection) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for q in store::unanswered(conn)? {
        if out.contains(&q.task_id)
            || tasks::get(conn, &q.task_id)?.status != TaskStatus::BlockedOnUser.as_str()
        {
            continue;
        }
        if matches!(last_case(conn, &q)?, Some((Case::Replan, _))) {
            out.push(q.task_id);
        }
    }
    Ok(out)
}

/// The question of task `task_id` that waits for an answer and has a
/// `replan` answer, with the text of that answer.
pub fn replan_question(conn: &Connection, task_id: &str) -> Result<Option<(Question, String)>> {
    for q in store::of_task(conn, task_id)? {
        if q.state == State::Answered {
            continue;
        }
        if let Some((Case::Replan, answer)) = last_case(conn, &q)? {
            return Ok(Some((q, answer)));
        }
    }
    Ok(None)
}

fn case_of(case: AnswerCase) -> Case {
    match case {
        AnswerCase::Covered => Case::Covered,
        AnswerCase::Revise => Case::Revise,
        AnswerCase::Replan => Case::Replan,
        AnswerCase::OwnDecision => Case::OwnDecision,
    }
}

/// `covered`: the citation names a line that exists, in assumptions.md or
/// spec.md of an idea of the feature of `q`.
fn covered(
    tx: &Connection,
    ctx: &Answering<'_>,
    q: &Question,
    idea: &IdeaRow,
    cite: Option<&str>,
) -> Result<Checked<()>> {
    let Some(cite) = cite else {
        return Ok(Err("a covered answer needs a citation <file>:<line>".into()));
    };
    let c = match parse_cite(cite) {
        Ok(c) => c,
        Err(reason) => return Ok(Err(reason)),
    };
    let cited = c.idea.clone().unwrap_or_else(|| idea.id.clone());
    match ideas_store::idea(tx, &cited)? {
        Some(row) if row.feature_id == q.feature_id => {}
        _ => {
            return Ok(Err(format!(
                "the citation `{cite}` names idea {cited}, which is not an idea of feature {}",
                q.feature_id
            )))
        }
    }
    let text = match folder(ctx, &cited)?.read_text(c.file) {
        Ok(text) => text,
        Err(e) => return Ok(Err(format!("the citation `{cite}`: {e}"))),
    };
    if !line_exists(&text, c.line) {
        return Ok(Err(format!(
            "the citation `{cite}` names line {} of {}/{}, which has {} lines",
            c.line,
            cited,
            c.file,
            text.lines().count()
        )));
    }
    Ok(Ok(()))
}

/// `revise`: the idea goes to you, and `q` waits for your approval.
fn revise(
    tx: &Connection,
    ctx: &Answering<'_>,
    q: &Question,
    idea: &IdeaRow,
    text: &str,
) -> Result<Checked<()>> {
    let state = idea.state.as_str();
    if PAST_APPROVAL.contains(&state) {
        let why = format!(
            "the answer of question {} changes an approved assumption, so the idea goes to you: {}",
            q.id,
            one_line(text)
        );
        transition(
            tx,
            ctx.guard,
            Entity::Idea,
            &idea.id,
            AWAITING_APPROVAL,
            by_fleetd(&why),
            &NeedsYouList,
        )?;
    } else if state != AWAITING_APPROVAL && state != REVISED_TO {
        return Ok(Err(format!(
            "idea {} is {state}: a revise needs an idea that you approved",
            idea.id
        )));
    }
    store::set_escalated(tx, &q.id)?;
    Ok(Ok(()))
}

/// `replan`: the task of `q` is a task of a committed plan, so it has a
/// replan file.
fn replan(tx: &Connection, q: &Question) -> Result<Checked<()>> {
    if committed_task(tx, &q.task_id)?.is_none() {
        return Ok(Err(format!(
            "task {} is not a task of a committed plan, so it has no replan",
            q.task_id
        )));
    }
    Ok(Ok(()))
}

fn own(
    tx: &Connection,
    ctx: &Answering<'_>,
    q: &Question,
    idea: &IdeaRow,
    text: &str,
) -> Result<Checked<OwnDecisionFiles>> {
    let folder = folder(ctx, &idea.id)?;
    let disk = match folder.read_text(ASSUMPTIONS_MD) {
        Ok(file) => Some(file),
        Err(FolderError::Missing { .. }) => None,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let before = intended_assumptions(tx, &idea.id, disk)?;
    let (base, new) = match own_decision(before.as_deref(), &idea.id, text, &q.id) {
        Ok(files) => files,
        Err(e) => {
            return Ok(Err(format!(
                "{ASSUMPTIONS_MD} of idea {} is not valid: {}",
                idea.id, e.reason
            )))
        }
    };
    if new.len() as u64 > FILE_LIMIT {
        return Ok(Err(format!(
            "{ASSUMPTIONS_MD} of idea {} would hold more than {FILE_LIMIT} bytes",
            idea.id
        )));
    }
    let approval = follow_approval(tx, ctx, &folder, idea, q, &base, &new)?;
    let assumptions = (before.as_deref() != Some(new.as_str())).then(|| AssumptionsWrite {
        idea: idea.id.clone(),
        base_sha256: before.map(|b| sha256_bytes(b.as_bytes()).hex()),
        text: new,
    });
    Ok(Ok(OwnDecisionFiles {
        approval,
        assumptions,
    }))
}

fn intended_assumptions(
    conn: &Connection,
    idea: &str,
    disk: Option<String>,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT data_json FROM events WHERE kind = ?1 AND entity = ?2
           AND json_extract(data_json, '$.effects.idea') = ?3
           AND json_extract(data_json, '$.effects.assumptions') IS NOT NULL
         ORDER BY event_id",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![EventKind::Question.as_str(), Entity::Task.as_str(), idea],
        |r| r.get::<_, String>(0),
    )?;
    let mut file = disk;
    for raw in rows {
        let data: Value = serde_json::from_str(&raw?)?;
        let Some(a) = Recorded::of_event(&data).and_then(|r| r.assumptions) else {
            continue;
        };
        if file.as_ref().map(|f| sha256_bytes(f.as_bytes()).hex()) == a.base_sha256 {
            file = Some(a.text);
        }
    }
    Ok(file)
}

/// When you approved assumptions.md of `idea` as `base`, record the same
/// approval of `new` (D13). The approval.json to write after the commit.
fn follow_approval(
    tx: &Connection,
    ctx: &Answering<'_>,
    folder: &IdeaFolder,
    idea: &IdeaRow,
    q: &Question,
    base: &str,
    new: &str,
) -> Result<Option<ApprovalFile>> {
    let approval = trusted_approval(tx, ctx.guard, folder, &idea.id)?;
    let Some((file, sha)) = approved_target(idea.approved_sha256.as_deref(), approval.as_ref())
    else {
        return Ok(None);
    };
    if file != ASSUMPTIONS_MD || sha != sha256_bytes(base.as_bytes()).hex() {
        return Ok(None);
    }
    let sha256 = sha256_bytes(new.as_bytes()).hex();
    let (approved_at, by) = match approval {
        Some(a) => (a.approved_at, a.by),
        None => (util::now(), BY_OPERATOR.to_string()),
    };
    let follows = Approval {
        idea: idea.id.clone(),
        file: ASSUMPTIONS_MD.to_string(),
        sha256: sha256.clone(),
        approved_at,
        by,
    };
    let why = format!(
        "the approval of idea {} follows the own decision of question {}: it needs no new approval",
        idea.id, q.id
    );
    Ok(Some(record_approval(
        tx,
        ctx.guard,
        &follows,
        &sha256,
        by_fleetd(&why),
    )?))
}

/// A refusal of an answer to a question that waits for none: nothing to
/// record and no ring.
fn quiet(question: &str, reason: String) -> Answered {
    Answered::Refused(Refused {
        question: question.to_string(),
        reason,
        ring: None,
    })
}

/// Refuse the answer of `q` for `reason`: 1 `note` event of its task, and
/// 1 doorbell to the live process of its feature.
fn refuse(tx: &Connection, ctx: &Answering<'_>, q: &Question, reason: String) -> Result<Answered> {
    let text = format!("the answer of question {} is refused: {reason}", q.id);
    let data = json!({ "question_id": q.id, "refused": reason });
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::Task,
        &q.task_id,
        by_fleetd(&text),
        &data,
    )?;
    let ring = match live_owner(tx, &q.feature_id)? {
        Some((attempt, pane)) => {
            let data = json!({
                "fleet_clock": ctx.clock.0,
                "question_ids": [q.id],
                "refused": reason,
            });
            record(
                tx,
                ctx.guard,
                EventKind::Doorbell,
                Entity::Attempt,
                &attempt,
                by_fleetd(&text),
                &data,
            )?;
            let event = door::facts_of(tx, Entity::Attempt, &attempt, EventKind::Doorbell)?
                .last()
                .map(|f| f.event_id)
                .context("the doorbell event that this answer recorded")?;
            Some(Ring {
                attempt,
                pane,
                event,
                questions: vec![q.id.clone()],
            })
        }
        None => None,
    };
    Ok(Answered::Refused(Refused {
        question: q.id.clone(),
        reason,
        ring,
    }))
}

/// The live process of `feature` in a pane: (attempt, pane).
pub(crate) fn live_owner(conn: &Connection, feature: &str) -> Result<Option<(String, String)>> {
    let mut found = None;
    for idea in ideas_store::of_feature(conn, feature)? {
        for (a, _) in attempts::idea_attempts(conn, &idea.id)? {
            if a.state == AttemptState::Live {
                found = a.pane.map(|pane| (a.id, pane));
            }
        }
    }
    Ok(found)
}

/// The case and the answer text of the last accepted answer of `q`.
fn last_case(conn: &Connection, q: &Question) -> Result<Option<(Case, String)>> {
    let mut found = None;
    for fact in tasks::task_facts(conn, &q.task_id, EventKind::Question)? {
        if fact["question_id"].as_str() != Some(q.id.as_str()) {
            continue;
        }
        let case = fact["case"].as_str().and_then(Case::parse);
        if let (Some(case), Some(answer)) = (case, fact["answer"].as_str()) {
            found = Some((case, answer.to_string()));
        }
    }
    Ok(found)
}

fn folder(ctx: &Answering<'_>, idea: &str) -> Result<IdeaFolder> {
    Ok(IdeaFolder::new(
        &ctx.root.join(IDEAS),
        idea,
        ctx.owner.clone(),
    )?)
}

fn idea_row(conn: &Connection, idea: &str) -> Result<IdeaRow> {
    ideas_store::idea(conn, idea)?.with_context(|| format!("no idea {idea} in the ledger"))
}

/// The idea of the task of `q`.
fn task_idea(conn: &Connection, q: &Question) -> Result<IdeaRow> {
    let idea = attempts::idea_of_task(conn, &q.task_id)?
        .with_context(|| format!("task {} has no idea", q.task_id))?;
    idea_row(conn, &idea)
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
