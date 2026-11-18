//! The verdict file of a judge task: 1 verdict, each finding ranked, and
//! the outcome critical_open.
//!
//! The file has a `## Verdict` section with 1 word, `pass` or
//! `critical_open`, and a `## Findings` list. Each item of the list starts
//! with its rank in brackets: `[CRITICAL]`, `[HIGH]`, `[MEDIUM]` or `[LOW]`
//! (docs/tickets/fleetd-p6/MAP.md, Q18). The verdict is `critical_open`
//! exactly when the list has a CRITICAL item (RR-3).

use super::retry::Outcome;

/// The rank of 1 finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    Critical,
    High,
    Medium,
    Low,
}

impl Rank {
    /// The rank of a `[WORD]` tag, or `None` for an unknown word.
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

/// The 1 word of the `## Verdict` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictWord {
    Pass,
    CriticalOpen,
}

/// 1 item of the `## Findings` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rank: Rank,
    /// The text after the rank tag, trimmed.
    pub text: String,
}

/// A verdict file that passed its structural check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub word: VerdictWord,
    pub findings: Vec<Finding>,
}

impl Verdict {
    /// The outcome of the judge attempt: `critical_open` for an open
    /// CRITICAL item, which burns no attempt, and `None` for `pass`.
    pub fn outcome(&self) -> Option<Outcome> {
        match self.word {
            VerdictWord::Pass => None,
            VerdictWord::CriticalOpen => Some(Outcome::CriticalOpen),
        }
    }
}

/// 1 fault of a verdict file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerdictError {
    /// The `## Verdict` sections hold `count` words, not 1.
    VerdictWords { count: usize },
    /// The 1 verdict word is not `pass` or `critical_open`.
    UnknownVerdict { word: String },
    /// The finding at 1-based `line` has no known rank tag.
    FindingWithoutRank { line: usize },
    /// A CRITICAL item is open and the verdict is `pass`.
    CriticalNotOpen,
    /// The verdict is `critical_open` and no item is CRITICAL.
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

/// The section a line belongs to.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Other,
    Verdict,
    Findings,
}

/// Parse a verdict file and run its structural check. Returns each fault.
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

/// The 1 verdict word, or `None` with its fault recorded.
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

/// The text of a top-level list item (`- `, `* ` or `1. `), or `None` for
/// any other line. An indented line continues the item above it.
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

/// The finding of a list item that starts with a known rank tag.
fn finding(item: &str) -> Option<Finding> {
    let rest = item.trim_start().strip_prefix('[')?;
    let (tag, text) = rest.split_once(']')?;
    Some(Finding {
        rank: Rank::from_tag(tag)?,
        text: text.trim().to_string(),
    })
}
