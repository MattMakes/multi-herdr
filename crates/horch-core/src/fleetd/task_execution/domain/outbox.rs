//! The outbox of an attempt: the names of its files, the closed lines of
//! `progress.jsonl`, and the rules of a hand-off file that a worker writes.
//!
//! Requirements: FR-7, RR-5, RR-8, SR-6. `docs/specs/fleetd/design.md`,
//! "Workspace layout and file protocol". The outbox is
//! `<fleet root>/tasks/<task id>/attempts/<attempt>/outbox/` and holds 3
//! kinds of file:
//!
//! - `result.json`, an immutable message. Its schema is
//!   [`crate::fleetd::task_execution::domain::result`], with the handoff
//!   keys of Q4 of `docs/tickets/fleetd-p5/MAP.md` ([`handoff_result`]).
//! - `questions/<n>.json`, an immutable message ([`Question`]).
//! - `progress.jsonl`, an append-only log of [`ProgressLine`]s.
//!
//! No I/O: the writer is `adapters/outbox_writer.rs`.

use super::result::{json_string, ResultFile, ResultStatus};

/// The outbox folder in an attempt folder.
pub const OUTBOX: &str = "outbox";
/// The result file in the outbox.
pub const RESULT_JSON: &str = "result.json";
/// The append-only log in the outbox.
pub const PROGRESS_JSONL: &str = "progress.jsonl";
/// The folder of the question files in the outbox.
pub const QUESTIONS: &str = "questions";
/// The suffix of a temp file. A reader ignores it (protocol rule 1).
pub const TMP_SUFFIX: &str = ".tmp";
/// The mode of the outbox and of its `questions` folder.
pub const DIR_MODE: u32 = 0o700;
/// The mode of each file in the outbox.
pub const FILE_MODE: u32 = 0o600;

/// The kind of a line of `progress.jsonl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// `horch ledger progress`.
    Progress,
    /// `horch ledger note`.
    Note,
}

impl LineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LineKind::Progress => "progress",
            LineKind::Note => "note",
        }
    }
}

/// A file or a line over its size limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooLarge {
    /// What is too large: `a progress line` or `a question file`.
    pub what: &'static str,
    pub bytes: u64,
    pub max: u64,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} has {} bytes; the limit is {} bytes",
            self.what, self.bytes, self.max
        )
    }
}

impl std::error::Error for TooLarge {}

/// 1 line of `progress.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressLine {
    pub task: String,
    /// The attempt, `A<n>`.
    pub attempt: String,
    pub at: String,
    pub kind: LineKind,
    pub text: String,
}

impl ProgressLine {
    /// The line, with its newline, when it has at most `max_bytes` bytes
    /// (`progress_line_max_bytes` of `fleet.toml`). The newline counts.
    pub fn to_line(&self, max_bytes: u64) -> Result<String, TooLarge> {
        let line = format!(
            "{{\"task\": {}, \"attempt\": {}, \"at\": {}, \"kind\": {}, \"text\": {}}}\n",
            json_string(&self.task),
            json_string(&self.attempt),
            json_string(&self.at),
            json_string(self.kind.as_str()),
            json_string(&self.text),
        );
        within(line, max_bytes, "a progress line")
    }
}

/// A question file, `questions/<n>.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub task: String,
    /// The attempt, `A<n>`.
    pub attempt: String,
    pub at: String,
    pub text: String,
}

impl Question {
    /// The JSON text of the file, when it has at most `max_bytes` bytes
    /// (`question_max_bytes` of `fleet.toml`).
    pub fn to_json(&self, max_bytes: u64) -> Result<String, TooLarge> {
        let text = format!(
            "{{\n  \"task\": {},\n  \"attempt\": {},\n  \"at\": {},\n  \"text\": {}\n}}\n",
            json_string(&self.task),
            json_string(&self.attempt),
            json_string(&self.at),
            json_string(&self.text),
        );
        within(text, max_bytes, "a question file")
    }
}

/// The name of question file `n`.
pub fn question_name(n: u32) -> String {
    format!("{n}.json")
}

/// The number of the question file `name`: `<n>.json` with `n` from 1.
pub fn question_number(name: &str) -> Option<u32> {
    let digits = name.strip_suffix(".json")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|n| *n > 0)
}

/// The `result.json` of `horch ledger handoff` (Q4): status `blocked`,
/// reason `handoff`, and the absolute path of the handoff file.
pub fn handoff_result(task: &str, attempt: &str, summary: &str, path: &str) -> ResultFile {
    ResultFile {
        task: task.to_string(),
        attempt: attempt.to_string(),
        status: ResultStatus::Blocked,
        summary: summary.to_string(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: Some(path.to_string()),
    }
}

fn within(text: String, max_bytes: u64, what: &'static str) -> Result<String, TooLarge> {
    let bytes = text.len() as u64;
    if bytes > max_bytes {
        return Err(TooLarge {
            what,
            bytes,
            max: max_bytes,
        });
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::task_execution::domain::result::{validate, Refusal};

    const MAX: u64 = 64 * 1024;

    fn line(text: &str) -> ProgressLine {
        ProgressLine {
            task: "T-1".into(),
            attempt: "A1".into(),
            at: "2026-11-18T12:00:00Z".into(),
            kind: LineKind::Progress,
            text: text.into(),
        }
    }

    #[test]
    fn progress_line_is_1_json_object_on_1_line() {
        let got = line("a \"quoted\"\nnew line").to_line(4096).unwrap();
        assert_eq!(
            got,
            "{\"task\": \"T-1\", \"attempt\": \"A1\", \"at\": \"2026-11-18T12:00:00Z\", \
             \"kind\": \"progress\", \"text\": \"a \\\"quoted\\\"\\nnew line\"}\n"
        );
        assert_eq!(got.matches('\n').count(), 1);
    }

    #[test]
    fn progress_line_at_the_limit_passes_and_1_byte_over_is_refused() {
        let at_limit = line("x").to_line(4096).unwrap().len() as u64;
        assert!(line("x").to_line(at_limit).is_ok());
        let err = line("x").to_line(at_limit - 1).unwrap_err();
        assert_eq!(err.bytes, at_limit);
        assert_eq!(err.max, at_limit - 1);
    }

    #[test]
    fn question_file_over_its_limit_is_refused() {
        let q = Question {
            task: "T-1".into(),
            attempt: "A1".into(),
            at: "2026-11-18T12:00:00Z".into(),
            text: "which file?".into(),
        };
        let text = q.to_json(MAX).unwrap();
        assert!(text.contains("\"text\": \"which file?\""));
        assert!(q.to_json(text.len() as u64 - 1).is_err());
    }

    #[test]
    fn question_names_are_numbered_from_1() {
        assert_eq!(question_name(3), "3.json");
        assert_eq!(question_number("3.json"), Some(3));
        for name in ["0.json", "x.json", ".json", "3.json.tmp", "+3.json", "3"] {
            assert_eq!(question_number(name), None, "{name}");
        }
    }

    #[test]
    fn handoff_result_round_trips_through_the_result_schema() {
        let file = handoff_result("T-1", "A1", "context limit", "/fleet/tasks/T-1/handoff.md");
        let read = validate(file.to_json().as_bytes(), "T-1", "A1", MAX).unwrap();
        assert_eq!(read, file);
        assert_eq!(read.status, ResultStatus::Blocked);
        assert!(file.to_json().contains("\"reason\": \"handoff\""));
    }

    #[test]
    fn handoff_keys_are_refused_unless_both_blocked_and_absolute() {
        let good = handoff_result("T-1", "A1", "s", "/abs/h.md").to_json();
        let cases = [
            good.replace("\"blocked\"", "\"completed\""),
            good.replace("\"reason\": \"handoff\"", "\"reason\": \"other\""),
            good.replace("\"/abs/h.md\"", "\"rel/h.md\""),
            good.replace(",\n  \"reason\": \"handoff\"", ""),
            good.replace(",\n  \"handoff\": \"/abs/h.md\"", ""),
        ];
        for text in cases {
            match validate(text.as_bytes(), "T-1", "A1", MAX) {
                Err(Refusal::Schema(_)) => {}
                other => panic!("{text}: {other:?}"),
            }
        }
    }
}
