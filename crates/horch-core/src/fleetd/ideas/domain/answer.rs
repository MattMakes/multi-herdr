use super::idea_folder::{
    is_idea_id, parse_assumptions, Invalid, ASSUMPTIONS_MD, OWN_DECISION_MARK, SPEC_MD,
};
use super::questions::printable;

pub const FEATURES: &str = "features";

pub const DECISIONS_MD: &str = "decisions.md";

pub const DECISIONS_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Covered,
    Revise,
    Replan,
    OwnDecision,
}

impl Case {
    pub const ALL: [Case; 4] = [Case::Covered, Case::Revise, Case::Replan, Case::OwnDecision];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    pub idea: Option<String>,
    pub file: &'static str,
    pub line: usize,
}

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

pub fn line_exists(text: &str, line: usize) -> bool {
    line > 0 && text.lines().nth(line - 1).is_some()
}

pub fn one_line(text: &str) -> String {
    printable(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

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
    let n = match parse_assumptions(&base) {
        Ok(a) => a.items.len() + 1,
        Err(e) if parse_assumptions(&format!("{head}{}", line(1))).is_err() => return Err(e),
        Err(_) => 1,
    };
    let new = format!("{head}{}", line(n));
    parse_assumptions(&new)?;
    Ok((base, new))
}

fn last_own_decision(file: &str, question: &str) -> Option<usize> {
    let trimmed = file.trim_end_matches('\n');
    let start = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let last = &trimmed[start..];
    let ours = last.contains(&format!("[{OWN_DECISION_MARK}] "))
        && last.ends_with(&format!(" ({question})"));
    ours.then_some(start)
}

#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub question_id: &'a str,
    pub case: Case,
    pub task_id: &'a str,
    pub idea_id: &'a str,
    pub question: Option<&'a str>,
    pub answer: &'a str,
    pub cite: Option<&'a str>,
}

pub fn entry_heading(question_id: &str, case: Case) -> String {
    format!("## {question_id}: {}", case.as_str())
}

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
