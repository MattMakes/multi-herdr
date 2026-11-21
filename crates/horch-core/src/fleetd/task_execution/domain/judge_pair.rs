use super::retry::Outcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    Critical,
    High,
    Medium,
    Low,
}

impl Rank {
    fn from_tag(word: &str) -> Option<Rank> {
        match word {
            "CRITICAL" => Some(Rank::Critical),
            "HIGH" => Some(Rank::High),
            "MEDIUM" => Some(Rank::Medium),
            "LOW" => Some(Rank::Low),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictWord {
    Pass,
    CriticalOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rank: Rank,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub word: VerdictWord,
    pub findings: Vec<Finding>,
}

impl Verdict {
    pub fn outcome(&self) -> Option<Outcome> {
        match self.word {
            VerdictWord::Pass => None,
            VerdictWord::CriticalOpen => Some(Outcome::CriticalOpen),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerdictError {
    VerdictWords { count: usize },
    UnknownVerdict { word: String },
    FindingWithoutRank { line: usize },
    CriticalNotOpen,
    CriticalOpenWithoutCritical,
}

impl std::fmt::Display for VerdictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerdictError::VerdictWords { count } => {
                write!(f, "the verdict has {count} words, not 1")
            }
            VerdictError::UnknownVerdict { word } => {
                write!(f, "the verdict `{word}` is not `pass` or `critical_open`")
            }
            VerdictError::FindingWithoutRank { line } => {
                write!(f, "the finding at line {line} has no rank")
            }
            VerdictError::CriticalNotOpen => {
                write!(f, "a CRITICAL item is open and the verdict is `pass`")
            }
            VerdictError::CriticalOpenWithoutCritical => {
                write!(f, "the verdict is `critical_open` and no item is CRITICAL")
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Other,
    Verdict,
    Findings,
}

pub fn parse_verdict(text: &str) -> Result<Verdict, Vec<VerdictError>> {
    let mut errors = Vec::new();
    let mut words: Vec<&str> = Vec::new();
    let mut findings = Vec::new();
    let mut section = Section::Other;
    for (i, line) in text.lines().enumerate() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = match heading.trim() {
                "Verdict" => Section::Verdict,
                "Findings" => Section::Findings,
                _ => Section::Other,
            };
            continue;
        }
        match section {
            Section::Verdict => {
                words.extend(line.split_whitespace().map(|w| w.trim_matches('`')));
            }
            Section::Findings => {
                if let Some(item) = list_item(line) {
                    match finding(item) {
                        Some(f) => findings.push(f),
                        None => errors.push(VerdictError::FindingWithoutRank { line: i + 1 }),
                    }
                }
            }
            Section::Other => {}
        }
    }
    let word = verdict_word(&words, &mut errors);
    let critical = findings.iter().any(|f: &Finding| f.rank == Rank::Critical);
    match (word, critical) {
        (Some(VerdictWord::Pass), true) => errors.push(VerdictError::CriticalNotOpen),
        (Some(VerdictWord::CriticalOpen), false) => {
            errors.push(VerdictError::CriticalOpenWithoutCritical)
        }
        _ => {}
    }
    match word {
        Some(word) if errors.is_empty() => Ok(Verdict { word, findings }),
        _ => Err(errors),
    }
}

fn verdict_word(words: &[&str], errors: &mut Vec<VerdictError>) -> Option<VerdictWord> {
    match words {
        ["pass"] => Some(VerdictWord::Pass),
        ["critical_open"] => Some(VerdictWord::CriticalOpen),
        [word] => {
            errors.push(VerdictError::UnknownVerdict {
                word: (*word).to_string(),
            });
            None
        }
        _ => {
            errors.push(VerdictError::VerdictWords { count: words.len() });
            None
        }
    }
}

fn list_item(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(rest);
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    line[digits..].strip_prefix(". ")
}

fn finding(item: &str) -> Option<Finding> {
    let rest = item.trim_start().strip_prefix('[')?;
    let (tag, text) = rest.split_once(']')?;
    Some(Finding {
        rank: Rank::from_tag(tag)?,
        text: text.trim().to_string(),
    })
}
