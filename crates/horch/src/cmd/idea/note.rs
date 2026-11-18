//! `horch idea note <id> [--question <qid> --case <case> [--cite <file:line>]]
//! [--dismiss <finding>] <text>`: the sub-orchestrator records 1 note in the
//! idea folder (`docs/specs/fleetd/design.md`, "Ideas"; protocol rule 5;
//! A35).
//!
//! The note is a file of class worker in `ideas/I-NNNN/notes/`: it never
//! uses the command channel or the request folder, and the command writes
//! no ledger row. Only the live sub-orchestrator attempt of the idea, found
//! by the parent chain of this process, may write it. fleetd ingests each
//! note 1 time.

use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Args, ValueEnum};
use horch_core::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts;
use horch_core::fleetd::ideas::adapters::idea_notes_reader::{self as notes, NoteBody, NoteWriter};
use horch_core::fleetd::task_execution::adapters::outbox_writer::NO_LIVE_ATTEMPT;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;

/// The separator of the question and the answer of an interview note.
const INTERVIEW_SEPARATOR: &str = " - ";

/// How an answer to a question is handled.
#[derive(Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum AnswerCase {
    /// The approved assumptions or the spec cover it; `--cite` names the line.
    Covered,
    /// The answer changes an approved assumption: it goes to you.
    Revise,
    /// The question shows a flaw in the plan: the task is replanned.
    Replan,
    /// Nothing approved covers it: a new assumption, marked as its own decision.
    OwnDecision,
}

#[derive(Args)]
pub struct NoteArgs {
    /// The idea id.
    pub id: String,
    /// The note: a question and its answer, or the reason of a dismiss.
    pub text: String,
    /// The id of the question that this note answers.
    #[arg(long, value_name = "QID", requires = "case")]
    pub question: Option<String>,
    /// How the answer is handled.
    #[arg(long, value_enum, requires = "question")]
    pub case: Option<AnswerCase>,
    /// The approved line that covers the answer.
    #[arg(long, value_name = "FILE:LINE", requires = "case")]
    pub cite: Option<String>,
    /// Dismiss this judge finding.
    #[arg(long, value_name = "FINDING")]
    pub dismiss: Option<String>,
}

impl From<AnswerCase> for notes::AnswerCase {
    fn from(case: AnswerCase) -> Self {
        match case {
            AnswerCase::Covered => notes::AnswerCase::Covered,
            AnswerCase::Revise => notes::AnswerCase::Revise,
            AnswerCase::Replan => notes::AnswerCase::Replan,
            AnswerCase::OwnDecision => notes::AnswerCase::OwnDecision,
        }
    }
}

/// `horch idea note`.
pub fn run(ctx: &RuntimeContext, args: NoteArgs) -> Result<ExitCode> {
    let body = body(&args)?;
    let Some(store) = taskledger::open_existing_read_only(ctx)? else {
        bail!("{NO_LIVE_ATTEMPT}: the ledger does not exist");
    };
    let writer = NoteWriter::of_process(
        std::process::id(),
        &ctx.paths.fleet_root.join("ideas"),
        &args.id,
        store.conn(),
        &PlatformFacts,
    )?;
    let path = writer.write(body)?;
    println!("{}", path.display());
    Ok(ExitCode::SUCCESS)
}

/// The note of `args`: a dismiss, an answer to a worker question, or else
/// 1 question of the interview and its answer as `<question> - <answer>`.
fn body(args: &NoteArgs) -> Result<NoteBody> {
    if let Some(finding) = &args.dismiss {
        if args.question.is_some() {
            bail!("--dismiss and --question cannot be used together");
        }
        return Ok(NoteBody::Dismiss {
            finding: finding.clone(),
            reason: args.text.clone(),
        });
    }
    if let (Some(question), Some(case)) = (&args.question, args.case) {
        return Ok(NoteBody::Answer {
            question: question.clone(),
            case: case.into(),
            cite: args.cite.clone(),
            text: args.text.clone(),
        });
    }
    let Some((question, answer)) = args.text.split_once(INTERVIEW_SEPARATOR) else {
        bail!("an interview note is \"<question>{INTERVIEW_SEPARATOR}<answer>\"");
    };
    Ok(NoteBody::Interview {
        question: question.to_string(),
        answer: answer.to_string(),
    })
}
