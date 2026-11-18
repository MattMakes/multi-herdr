//! The idea folder: idea.md, the amendments, interview.md, assumptions.md,
//! spec.md and requirements.yaml, and the checks of their content.
//!
//! Requirements: FR-1 (the immutable idea file), FM-30 (an `[open]` item
//! refuses `assumptions.md`; each requirement cites an assumption or an
//! answer). The file checks of SR-6 are in the adapter `idea_folder_files`.

use super::plan_validator::Requirement;

/// The size limit of each file of an idea folder, in bytes.
pub const FILE_LIMIT: u64 = 64 * 1024;

/// The file names of an idea folder.
pub const IDEA_MD: &str = "idea.md";
pub const AMENDMENTS: &str = "amendments";
pub const INTERVIEW_MD: &str = "interview.md";
pub const ASSUMPTIONS_MD: &str = "assumptions.md";
pub const SPEC_MD: &str = "spec.md";
pub const REQUIREMENTS_YAML: &str = "requirements.yaml";

/// The line that opens and closes the front matter of idea.md.
const FENCE: &str = "---";

/// The content of a file fails a rule. `reason` is what the user reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub reason: String,
}

fn invalid(reason: impl Into<String>) -> Invalid {
    Invalid {
        reason: reason.into(),
    }
}

/// idea.md: its front matter and its text. Written 1 time, never changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaFile {
    /// `I-NNNN`.
    pub id: String,
    /// The time of `horch idea add`, RFC 3339.
    pub created: String,
    /// What wrote the idea, for example `horch idea add`.
    pub source: String,
    pub priority: i64,
    /// The ids of the ideas that this idea depends on.
    pub depends_on: Vec<String>,
    /// The text of the idea, after the front matter.
    pub text: String,
}

/// Split idea.md into its front matter and its text.
pub fn split_front_matter(file: &str) -> Result<(&str, &str), Invalid> {
    let no_front = || invalid("idea.md has no front matter between 2 `---` lines");
    file.strip_prefix(&format!("{FENCE}\n"))
        .and_then(|rest| rest.split_once(&format!("\n{FENCE}\n")))
        .ok_or_else(no_front)
}

/// Join the front matter `front` (YAML that ends with a newline) and `text`
/// into idea.md.
pub fn join_front_matter(front: &str, text: &str) -> String {
    format!("{FENCE}\n{front}{FENCE}\n{text}")
}

/// `id` is an idea id: `I-` and digits.
pub fn is_idea_id(id: &str) -> bool {
    id.strip_prefix("I-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The id in a file is the id of its folder (Protocol rule 7).
pub fn check_idea_id(file: &str, found: &str, folder: &str) -> Result<(), Invalid> {
    if found == folder {
        Ok(())
    } else {
        Err(invalid(format!(
            "{file} names idea {found}, not the idea of its folder {folder}"
        )))
    }
}

/// The mark of an item of assumptions.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Confirmed,
    Assumed,
    /// An assumption that the sub-orchestrator decided for a worker
    /// question (D13): it needs no new approval, and you can reverse it.
    OwnDecision,
    Open,
}

/// The mark word of an own decision of the sub-orchestrator (D13).
pub const OWN_DECISION_MARK: &str = "assumed, own decision";

/// 1 numbered item of assumptions.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    /// The number of the item; its id is `A<number>`.
    pub number: u32,
    pub mark: Mark,
    pub text: String,
    /// The answer of interview.md that the item cites: `Q<n>`.
    pub question: Option<String>,
}

/// assumptions.md, validated: no item is `[open]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumptions {
    /// The name of the feature branch, from the line `feature: <name>`.
    pub feature: Option<String>,
    pub items: Vec<Assumption>,
}

impl Assumptions {
    /// The id of each item: `A1`, `A2`, ...
    pub fn ids(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|a| format!("A{}", a.number))
            .collect()
    }
}

/// Parse and validate assumptions.md: a `# ` heading, an optional
/// `feature: <kebab-case name>` line, then items `<n>. [<mark>] <text>`
/// with an optional `(Q<n>)` at the end, numbered from 1. A file with an
/// `[open]` item does not validate.
pub fn parse_assumptions(file: &str) -> Result<Assumptions, Invalid> {
    let mut lines = file
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .peekable();
    match lines.next() {
        Some((_, l)) if l.starts_with("# ") => {}
        _ => return Err(invalid("assumptions.md does not start with a `# ` heading")),
    }
    let mut feature = None;
    if let Some(name) = lines.peek().and_then(|(_, l)| l.strip_prefix("feature:")) {
        let name = name.trim();
        if !is_kebab_case(name) {
            return Err(invalid(format!(
                "the feature name `{name}` is not kebab-case"
            )));
        }
        feature = Some(name.to_string());
        lines.next();
    }
    let mut items = Vec::new();
    for (index, line) in lines {
        let item = parse_item(line)
            .ok_or_else(|| invalid(format!("line {} is not an item: {line}", index + 1)))?;
        let expected = items.len() as u32 + 1;
        if item.number != expected {
            return Err(invalid(format!(
                "item {} is not numbered {expected}",
                item.number
            )));
        }
        items.push(item);
    }
    if items.is_empty() {
        return Err(invalid("assumptions.md has no item"));
    }
    let open: Vec<String> = items
        .iter()
        .filter(|a| a.mark == Mark::Open)
        .map(|a| format!("item {} is [open]", a.number))
        .collect();
    if !open.is_empty() {
        return Err(invalid(open.join("; ")));
    }
    Ok(Assumptions { feature, items })
}

/// 1 item line, or `None` when the line is not an item.
fn parse_item(line: &str) -> Option<Assumption> {
    let (number, rest) = line.split_once(". ")?;
    let number = number.parse::<u32>().ok()?;
    let (mark, rest) = rest.strip_prefix('[')?.split_once("] ")?;
    let mark = match mark {
        "confirmed" => Mark::Confirmed,
        "assumed" => Mark::Assumed,
        OWN_DECISION_MARK => Mark::OwnDecision,
        "open" => Mark::Open,
        _ => return None,
    };
    let mut text = rest.trim();
    let mut question = None;
    if let Some((before, cite)) = text
        .strip_suffix(')')
        .and_then(|t| t.rsplit_once(" ("))
        .filter(|(_, cite)| is_numbered_id(cite, 'Q'))
    {
        text = before.trim_end();
        question = Some(cite.to_string());
    }
    if text.is_empty() {
        return None;
    }
    Some(Assumption {
        number,
        mark,
        text: text.to_string(),
        question,
    })
}

/// `name` is lower-case words of letters and digits, joined by `-`.
fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// `id` is `<prefix><n>` with `n` a number from 1.
fn is_numbered_id(id: &str, prefix: char) -> bool {
    id.strip_prefix(prefix)
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n > 0 && !id[1..].starts_with('0'))
}

/// The heading of a new interview.md.
pub fn interview_heading(idea: &str) -> String {
    format!("# {idea} interview\n")
}

/// The entry of answer `Q<n>` in interview.md.
pub fn interview_entry(n: usize, question: &str, answer: &str) -> String {
    format!(
        "\n## Q{n}\n{}\n\nAnswer: {}\n",
        question.trim(),
        answer.trim()
    )
}

/// The id of each answer of interview.md: `Q1`, `Q2`, ...
pub fn interview_answers(file: &str) -> Vec<String> {
    file.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .filter(|id| is_numbered_id(id, 'Q'))
        .map(str::to_string)
        .collect()
}

/// spec.md validates when it has at least 1 `## ` heading.
pub fn check_spec(file: &str) -> Result<(), Invalid> {
    if file.lines().any(|l| l.starts_with("## ")) {
        Ok(())
    } else {
        Err(invalid("spec.md has no `## ` heading"))
    }
}

/// Validate the requirements of requirements.yaml: at least 1, each id
/// unique, and each requirement cites at least 1 item, each an id of
/// `assumptions` (`A<n>`) or of `answers` (`Q<n>`).
pub fn check_requirements(
    requirements: &[Requirement],
    assumptions: &[String],
    answers: &[String],
) -> Result<(), Invalid> {
    if requirements.is_empty() {
        return Err(invalid("requirements.yaml has no requirement"));
    }
    let mut problems = Vec::new();
    for (index, req) in requirements.iter().enumerate() {
        let id = &req.id;
        if requirements[..index].iter().any(|r| &r.id == id) {
            problems.push(format!("requirement {id} is not the only one with its id"));
        }
        if req.cites.is_empty() {
            problems.push(format!("requirement {id} cites nothing"));
        }
        for cite in &req.cites {
            if !assumptions.contains(cite) && !answers.contains(cite) {
                problems.push(format!(
                    "requirement {id} cites {cite}, which is not an assumption or an answer"
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(invalid(problems.join("; ")))
    }
}
