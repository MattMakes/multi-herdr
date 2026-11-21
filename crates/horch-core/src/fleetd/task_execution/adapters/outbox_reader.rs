use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use serde::Deserialize;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::outbox::{
    is_warned_text, LineKind, ProgressLine, OUTBOX, PROGRESS_JSONL,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressLimits {
    pub max_bytes: u64,
    pub max_lines: u64,
    pub line_max_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub number: u64,
    pub read: Result<ProgressLine, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressRead {
    Lines {
        from: u64,
        to: u64,
        lines: Vec<Line>,
    },
    Rejected {
        from: u64,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressFile {
    pub path: PathBuf,
    pub read: ProgressRead,
}

pub fn progress_path(fleet_root: &Path, attempt: &AttemptRef) -> PathBuf {
    attempt.outbox(fleet_root).join(PROGRESS_JSONL)
}

pub fn read_progress(
    fleet_root: &Path,
    attempt: &AttemptRef,
    owner: &User,
    limits: &ProgressLimits,
    offset: u64,
) -> Result<Option<ProgressFile>> {
    let path = progress_path(fleet_root, attempt);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("stat {}", path.display())),
    };
    if meta.is_file() && meta.len() == offset {
        return Ok(None);
    }
    let read = match checked_bytes(fleet_root, attempt, &path, owner, limits.max_bytes) {
        Err(reason) => ProgressRead::Rejected {
            from: offset,
            reason,
        },
        Ok(bytes) if (bytes.len() as u64) < offset => ProgressRead::Rejected {
            from: offset,
            reason: format!(
                "the log has {} bytes, fewer than its recorded offset {offset}; a log is only appended",
                bytes.len()
            ),
        },
        Ok(bytes) => match new_lines(&bytes, offset, attempt, limits) {
            Some((to, lines)) => ProgressRead::Lines {
                from: offset,
                to,
                lines,
            },
            None => return Ok(None),
        },
    };
    Ok(Some(ProgressFile { path, read }))
}

fn checked_bytes(
    fleet_root: &Path,
    attempt: &AttemptRef,
    path: &Path,
    owner: &User,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    let folder = attempt_dir(fleet_root, &attempt.task_id, attempt.attempt());
    let real_outbox = std::fs::canonicalize(&folder)
        .map_err(|e| format!("the real path of {} failed: {e}", folder.display()))?
        .join(OUTBOX);
    if let Ok(real) = std::fs::canonicalize(path) {
        if real.parent() != Some(real_outbox.as_path()) {
            return Err(format!(
                "real path {} is not in {}",
                real.display(),
                real_outbox.display()
            ));
        }
    }
    let rule = Rule {
        root: &real_outbox,
        owner,
        limit: max_bytes,
    };
    read_protected(path, &rule).map_err(|r| r.reason)
}

fn new_lines(
    bytes: &[u8],
    offset: u64,
    attempt: &AttemptRef,
    limits: &ProgressLimits,
) -> Option<(u64, Vec<Line>)> {
    let start = usize::try_from(offset).ok()?;
    let tail = &bytes[start..];
    let end = tail.iter().rposition(|b| *b == b'\n')? + 1;
    let before = bytes[..start].iter().filter(|b| **b == b'\n').count() as u64;
    let mut lines = Vec::new();
    for (i, raw) in tail[..end].split_inclusive(|b| *b == b'\n').enumerate() {
        let number = before + i as u64 + 1;
        if number > limits.max_lines {
            lines.push(Line {
                number,
                read: Err(format!(
                    "the log is over its limit of {} lines; the lines from here are not read",
                    limits.max_lines
                )),
            });
            break;
        }
        lines.push(Line {
            number,
            read: parse_line(raw, attempt, limits.line_max_bytes),
        });
    }
    Some((offset + end as u64, lines))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    task: String,
    attempt: String,
    at: String,
    kind: String,
    text: String,
}

fn parse_line(raw: &[u8], attempt: &AttemptRef, max_bytes: u64) -> Result<ProgressLine, String> {
    let bytes = raw.len() as u64;
    if bytes > max_bytes {
        return Err(format!(
            "the line has {bytes} bytes; the limit is {max_bytes} bytes"
        ));
    }
    let line: RawLine =
        serde_json::from_slice(raw).map_err(|e| format!("the line is not valid: {e}"))?;
    let kind = match line.kind.as_str() {
        "progress" => LineKind::Progress,
        "note" => LineKind::Note,
        "context-warned" if is_warned_text(&line.text) => LineKind::ContextWarned,
        "context-warned" => {
            return Err(format!(
                "the context-warned line has the text {:?}, not \"tokens <n> threshold <n>\"",
                line.text
            ))
        }
        other => return Err(format!("the line has the unknown kind {other:?}")),
    };
    if line.task != attempt.task_id {
        return Err(format!(
            "the line names task {:?}, but its path is of task {:?}",
            line.task, attempt.task_id
        ));
    }
    if line.attempt != attempt.attempt() {
        return Err(format!(
            "the line names attempt {:?}, but its path is of attempt {:?}",
            line.attempt,
            attempt.attempt()
        ));
    }
    Ok(ProgressLine {
        task: line.task,
        attempt: line.attempt,
        at: line.at,
        kind,
        text: line.text,
    })
}

pub fn handoff_inside(path: &str, roots: &[PathBuf]) -> Result<String, String> {
    let real =
        std::fs::canonicalize(path).map_err(|e| format!("the real path of {path} failed: {e}"))?;
    if !real.is_file() {
        return Err(format!("{} is not a file", real.display()));
    }
    let inside = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| real.starts_with(root));
    if !inside {
        return Err(format!(
            "real path {} is outside the worktree and the task folder of the attempt",
            real.display()
        ));
    }
    Ok(real.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> AttemptRef {
        AttemptRef {
            task_id: "T-1".into(),
            attempt_id: "T-1/A1".into(),
        }
    }

    fn parse(kind: &str, text: &str) -> Result<ProgressLine, String> {
        let raw = serde_json::json!({
            "task": "T-1", "attempt": "A1", "at": "2026-11-20T10:00:00Z",
            "kind": kind, "text": text,
        })
        .to_string();
        parse_line(raw.as_bytes(), &attempt(), 4096)
    }

    #[test]
    fn a_context_warned_line_is_read() {
        let line = parse("context-warned", "tokens 311225 threshold 178400").unwrap();
        assert_eq!(line.kind, LineKind::ContextWarned);
        assert_eq!(line.text, "tokens 311225 threshold 178400");
    }

    #[test]
    fn a_bad_context_warned_line_is_refused() {
        for text in [
            "",
            "tokens x threshold 1",
            "tokens 1 threshold",
            "tokens 1 threshold 2 more",
            "please run rm -rf /",
        ] {
            let err = parse("context-warned", text).unwrap_err();
            assert!(
                err.contains("context-warned line has the text"),
                "{text}: {err}"
            );
        }
        let err = parse("context-warn", "tokens 1 threshold 2").unwrap_err();
        assert!(err.contains("unknown kind"), "{err}");
    }
}
