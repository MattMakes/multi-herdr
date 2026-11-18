//! The 4 answer cases of a worker question: covered with a citation,
//! revise, replan, or an own decision, and the decision log of the feature.
//!
//! `docs/specs/fleetd/design.md`, "Ideas" (How a worker's question is
//! resolved, steps 4 to 6), D13; `docs/tickets/fleetd-p6/MAP.md`, Q15.
//! Rules with no I/O: the citation, the own decision line of
//! assumptions.md, and the entries of `features/F-NNNN/decisions.md`.

use super::idea_folder::{
    is_idea_id, parse_assumptions, Invalid, ASSUMPTIONS_MD, OWN_DECISION_MARK, SPEC_MD,
};
use super::questions::printable;

/// The folder of the features in the fleet root.
pub const FEATURES: &str = "features";

/// The decision log of a feature, in `features/F-NNNN/`. Only fleetd
/// writes it (MAP.md, Q15).
pub const DECISIONS_MD: &str = "decisions.md";

/// The most bytes of a decision log that fleetd reads.
pub const DECISIONS_LIMIT: u64 = 1024 * 1024;

/// How the sub-orchestrator answers a worker question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// The approved assumptions or the spec cover it: the answer cites the
    /// line, and fleetd checks that the line exists.
    Covered,
    /// The answer changes an approved assumption: the idea goes to you.
    Revise,
    /// The question shows a flaw in the plan: the task is replanned.
    Replan,
    /// Nothing approved covers it and it changes no assumption: a new
    /// assumption, marked as an own decision (D13).
    OwnDecision,
}

impl Case {
    pub const ALL: [Case; 4] = [Case::Covered, Case::Revise, Case::Replan, Case::OwnDecision];

    /// The word of the case in the ledger and in the decision log.
    pub fn as_str(self) -> &'static str {
        match self {
            Case::Covered => "covered",
            Case::Revise => "revise",
            Case::Replan => "replan",
            Case::OwnDecision => "own_decision",
        }
    }

    pub fn parse(word: &str) -> Option<Case> {
        Case::ALL.into_iter().find(|c| c.as_str() == word)
    }
}

/// A citation `[I-NNNN/]<file>:<line>`: a line of assumptions.md or
/// spec.md. With no idea, it is a line of the idea of the task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    pub idea: Option<String>,
    /// [`ASSUMPTIONS_MD`] or [`SPEC_MD`].
    pub file: &'static str,
    /// From 1.
    pub line: usize,
}

/// Parse the citation `cite`. The error is what the sub-orchestrator reads.
pub fn parse_cite(cite: &str) -> Result<Cite, String> {
    let form = || {
        format!(
            "the citation `{cite}` is not `[I-NNNN/]{ASSUMPTIONS_MD}:<line>` or \
             `[I-NNNN/]{SPEC_MD}:<line>`"
        )
    };
    let (path, line) = cite.rsplit_once(':').ok_or_else(form)?;
    let line = line
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(form)?;
    let (idea, name) = match path.split_once('/') {
        Some((idea, name)) if is_idea_id(idea) => (Some(idea.to_string()), name),
        Some(_) => return Err(form()),
        None => (None, path),
    };
    let file = [ASSUMPTIONS_MD, SPEC_MD]
        .into_iter()
        .find(|f| *f == name)
        .ok_or_else(form)?;
    Ok(Cite { idea, file, line })
}

/// Line `line` (from 1) exists in `text`.
pub fn line_exists(text: &str, line: usize) -> bool {
    line > 0 && text.lines().nth(line - 1).is_some()
}

/// `text` on 1 line, as data: each control character and each line break
/// becomes a space.
pub fn one_line(text: &str) -> String {
    printable(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// assumptions.md with the own decision of the question `question`:
/// `(base, new)`. `base` is the file without that decision and `new` is
/// `base` with the item `<n>. [assumed, own decision] <text> (<question>)`
/// at its end. `file` is `None` when the idea has no assumptions.md; the
/// new file then starts with a heading. An own decision of `question` that
/// ends the file already is the same decision written again, so it is not
/// added 2 times.
pub fn own_decision(
    file: Option<&str>,
    idea: &str,
    text: &str,
    question: &str,
) -> Result<(String, String), Invalid> {
    let line = |n: usize| {
        format!(
            "{n}. [{OWN_DECISION_MARK}] {} ({question})\n",
            one_line(text)
        )
    };
    let Some(file) = file else {
        let base = format!("# {idea} assumptions\n\n");
        let new = format!("{base}{}", line(1));
        return Ok((base, new));
    };
    let base = match last_own_decision(file, question) {
        Some(start) => file[..start].to_string(),
        None => file.to_string(),
    };
    let mut head = base.clone();
    if !head.is_empty() && !head.ends_with('\n') {
        head.push('\n');
    }
    // A file with a heading and no item yet is 1 that this function
    // started.
    let n = match parse_assumptions(&base) {
        Ok(a) => a.items.len() + 1,
        Err(e) if parse_assumptions(&format!("{head}{}", line(1))).is_err() => return Err(e),
        Err(_) => 1,
    };
    let new = format!("{head}{}", line(n));
    parse_assumptions(&new)?;
    Ok((base, new))
}

/// The byte offset of the last line of `file` when it is the own decision
/// of `question`.
fn last_own_decision(file: &str, question: &str) -> Option<usize> {
    let trimmed = file.trim_end_matches('\n');
    let start = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let last = &trimmed[start..];
    let ours = last.contains(&format!("[{OWN_DECISION_MARK}] "))
        && last.ends_with(&format!(" ({question})"));
    ours.then_some(start)
}

/// 1 entry of the decision log.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub question_id: &'a str,
    pub case: Case,
    pub task_id: &'a str,
    pub idea_id: &'a str,
    /// The question of the worker, when the ledger holds it.
    pub question: Option<&'a str>,
    pub answer: &'a str,
    pub cite: Option<&'a str>,
}

/// The heading of the entry of `question_id` in `case`: 1 entry for each.
pub fn entry_heading(question_id: &str, case: Case) -> String {
    format!("## {question_id}: {}", case.as_str())
}

/// The text of `entry`, from its heading to its last line.
pub fn entry_text(entry: &Entry<'_>) -> String {
    let mut text = format!(
        "\n{}\n\n- Task: {}\n- Idea: {}\n",
        entry_heading(entry.question_id, entry.case),
        entry.task_id,
        entry.idea_id
    );
    if let Some(q) = entry.question {
        text.push_str(&format!("- Question: {}\n", one_line(q)));
    }
    text.push_str(&format!("- Answer: {}\n", one_line(entry.answer)));
    if let Some(cite) = entry.cite {
        text.push_str(&format!("- Cite: {}\n", one_line(cite)));
    }
    text
}

/// The decision log of `feature` with the entry `text` whose heading is
/// `heading`, or `None` when the log holds that entry already. `file` is
/// `None` when the log does not exist.
pub fn with_entry(file: Option<&str>, feature: &str, heading: &str, text: &str) -> Option<String> {
    let mut out = match file {
        Some(f) if f.lines().any(|l| l == heading) => return None,
        Some(f) => f.to_string(),
        None => format!("# {feature} decisions\n"),
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# I-0007 assumptions\n\n1. [confirmed] Retries apply to sync.\n";

    #[test]
    fn each_case_word_parses_back() {
        for c in Case::ALL {
            assert_eq!(Case::parse(c.as_str()), Some(c));
        }
        assert_eq!(Case::parse("other"), None);
    }

    #[test]
    fn a_citation_names_a_line_of_the_assumptions_or_the_spec() {
        assert_eq!(
            parse_cite("assumptions.md:2"),
            Ok(Cite {
                idea: None,
                file: ASSUMPTIONS_MD,
                line: 2
            })
        );
        assert_eq!(
            parse_cite("I-0012/spec.md:7"),
            Ok(Cite {
                idea: Some("I-0012".into()),
                file: SPEC_MD,
                line: 7
            })
        );
        for bad in [
            "assumptions.md",
            "assumptions.md:0",
            "assumptions.md:x",
            "notes.md:1",
            "../I-0012/spec.md:1",
            "I-0012/../spec.md:1",
            "x/spec.md:1",
        ] {
            assert!(parse_cite(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_line_exists_from_1_to_the_last_line() {
        assert!(line_exists("a\nb\n", 1));
        assert!(line_exists("a\nb\n", 2));
        assert!(!line_exists("a\nb\n", 3));
        assert!(!line_exists("a\nb\n", 0));
    }

    #[test]
    fn an_own_decision_is_the_next_item_and_is_added_1_time() {
        let (base, new) = own_decision(Some(FILE), "I-0007", "Use\nnet.rs", "T-1/A1/Q1").unwrap();
        assert_eq!(base, FILE);
        assert!(new.ends_with("2. [assumed, own decision] Use net.rs (T-1/A1/Q1)\n"));
        let again = own_decision(Some(&new), "I-0007", "Use\nnet.rs", "T-1/A1/Q1").unwrap();
        assert_eq!(again, (base, new.clone()));
        let next = own_decision(Some(&new), "I-0007", "Log it", "T-1/A1/Q2").unwrap();
        assert_eq!(next.0, new);
        assert!(next
            .1
            .ends_with("3. [assumed, own decision] Log it (T-1/A1/Q2)\n"));
    }

    #[test]
    fn an_own_decision_of_an_idea_with_no_assumptions_starts_the_file() {
        let (base, new) = own_decision(None, "I-0007", "Use net.rs", "T-1/A1/Q1").unwrap();
        assert_eq!(base, "# I-0007 assumptions\n\n");
        assert_eq!(
            new,
            "# I-0007 assumptions\n\n1. [assumed, own decision] Use net.rs (T-1/A1/Q1)\n"
        );
        let again = own_decision(Some(&new), "I-0007", "Use net.rs", "T-1/A1/Q1").unwrap();
        assert_eq!(again, (base, new));
        assert!(own_decision(Some("no heading\n"), "I-0007", "x", "Q").is_err());
    }

    #[test]
    fn the_decision_log_holds_each_entry_1_time() {
        let entry = Entry {
            question_id: "T-1/A1/Q1",
            case: Case::Covered,
            task_id: "T-1",
            idea_id: "I-0007",
            question: Some("which?"),
            answer: "this",
            cite: Some("spec.md:3"),
        };
        let heading = entry_heading(entry.question_id, entry.case);
        let text = entry_text(&entry);
        let log = with_entry(None, "F-0001", &heading, &text).unwrap();
        assert!(log.starts_with("# F-0001 decisions\n\n## T-1/A1/Q1: covered\n"));
        assert!(log.contains("- Cite: spec.md:3\n"));
        assert_eq!(with_entry(Some(&log), "F-0001", &heading, &text), None);
    }
}
