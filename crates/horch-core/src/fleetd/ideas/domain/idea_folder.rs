use super::plan_validator::Requirement;

pub const FILE_LIMIT: u64 = 64 * 1024;

pub const IDEA_MD: &str = "idea.md";
pub const AMENDMENTS: &str = "amendments";
pub const INTERVIEW_MD: &str = "interview.md";
pub const ASSUMPTIONS_MD: &str = "assumptions.md";
pub const SPEC_MD: &str = "spec.md";
pub const REQUIREMENTS_YAML: &str = "requirements.yaml";

const FENCE: &str = "---";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub reason: String,
}

fn invalid(reason: impl Into<String>) -> Invalid {
    Invalid {
        reason: reason.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaFile {
    pub id: String,
    pub created: String,
    pub source: String,
    pub priority: i64,
    pub depends_on: Vec<String>,
    pub text: String,
}

pub fn split_front_matter(file: &str) -> Result<(&str, &str), Invalid> {
    let no_front = || invalid("idea.md has no front matter between 2 `---` lines");
    file.strip_prefix(&format!("{FENCE}\n"))
        .and_then(|rest| rest.split_once(&format!("\n{FENCE}\n")))
        .ok_or_else(no_front)
}

pub fn join_front_matter(front: &str, text: &str) -> String {
    format!("{FENCE}\n{front}{FENCE}\n{text}")
}

pub fn is_idea_id(id: &str) -> bool {
    id.strip_prefix("I-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn check_idea_id(file: &str, found: &str, folder: &str) -> Result<(), Invalid> {
    if found == folder {
        Ok(())
    } else {
        Err(invalid(format!(
            "{file} names idea {found}, not the idea of its folder {folder}"
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Confirmed,
    Assumed,
    OwnDecision,
    Open,
}

pub const OWN_DECISION_MARK: &str = "assumed, own decision";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    pub number: u32,
    pub mark: Mark,
    pub text: String,
    pub question: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumptions {
    pub feature: Option<String>,
    pub items: Vec<Assumption>,
}

impl Assumptions {
    pub fn ids(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|a| format!("A{}", a.number))
            .collect()
    }
}

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

fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn is_numbered_id(id: &str, prefix: char) -> bool {
    id.strip_prefix(prefix)
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n > 0 && !id[1..].starts_with('0'))
}

pub fn interview_heading(idea: &str) -> String {
    format!("# {idea} interview\n")
}

pub fn interview_entry(n: usize, question: &str, answer: &str) -> String {
    format!(
        "\n## Q{n}\n{}\n\nAnswer: {}\n",
        question.trim(),
        answer.trim()
    )
}

pub fn interview_answers(file: &str) -> Vec<String> {
    file.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .filter(|id| is_numbered_id(id, 'Q'))
        .map(str::to_string)
        .collect()
}

pub fn check_spec(file: &str) -> Result<(), Invalid> {
    if file.lines().any(|l| l.starts_with("## ")) {
        Ok(())
    } else {
        Err(invalid("spec.md has no `## ` heading"))
    }
}

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
