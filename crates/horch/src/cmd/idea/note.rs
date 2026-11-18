//! `horch idea note <id> [--question <qid> --case <case> [--cite <file:line>]]
//! [--dismiss <finding>] <text>`: the sub-orchestrator records 1 note in the
//! idea folder (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 25 of
//! fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, ValueEnum};
use horch_core::runtime::RuntimeContext;

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

/// `horch idea note`.
pub fn run(_ctx: &RuntimeContext, _args: NoteArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea note", 25)
}
