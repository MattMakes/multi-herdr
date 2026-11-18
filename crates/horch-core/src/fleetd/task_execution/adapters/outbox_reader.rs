//! Reads the outbox files of an attempt through the protected file reader,
//! with the owner and mode checks, and refuses a foreign-owned outbox:
//! `progress.jsonl` from its recorded offset, and the handoff path of a
//! handoff result.
//!
//! Requirements: SR-6, RR-8, FM-23. `docs/specs/fleetd/design.md`,
//! "Workspace layout and file protocol", protocol rules 1, 2 and 7;
//! `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q4. `result.json` and the
//! question files are read by `result_reader.rs`.
//!
//! - `progress.jsonl` is an append-only log. This reader reads it from the
//!   offset that the ledger recorded, only to its last complete line, so a
//!   half-written line waits for the next pass (rule 1).
//! - The file is rejected only when it fails a check of the protected file
//!   reader (a link, not a regular file, another owner, a wide mode, a real
//!   path outside its outbox, over `progress_max_bytes`) or when it is
//!   shorter than its recorded offset: an append-only log never shrinks.
//! - A line that fails (over `progress_line_max_bytes`, not a valid line,
//!   or of another task or attempt) is skipped with its reason. The lines
//!   after `progress_max_lines` are skipped with 1 reason.

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use serde::Deserialize;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::outbox::{
    LineKind, ProgressLine, OUTBOX, PROGRESS_JSONL,
};

/// The limits of `progress.jsonl` (`fleet.toml`, protocol rule 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressLimits {
    pub max_bytes: u64,
    pub max_lines: u64,
    /// The newline counts.
    pub line_max_bytes: u64,
}

/// 1 complete line of `progress.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The number of the line in the file, from 1.
    pub number: u64,
    /// The line, or the reason why it is skipped.
    pub read: Result<ProgressLine, String>,
}

/// What the reader found in `progress.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressRead {
    /// The new complete lines from byte `from` to byte `to`.
    Lines {
        from: u64,
        to: u64,
        lines: Vec<Line>,
    },
    /// The file failed a check. `from` is the recorded offset.
    Rejected { from: u64, reason: String },
}

/// `progress.jsonl` of 1 attempt and what the reader found in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressFile {
    /// The path under the fleet root, as the ledger records it.
    pub path: PathBuf,
    pub read: ProgressRead,
}

/// The path of `progress.jsonl` of `attempt` under `fleet_root`.
pub fn progress_path(fleet_root: &Path, attempt: &AttemptRef) -> PathBuf {
    attempt.outbox(fleet_root).join(PROGRESS_JSONL)
}

/// Read `progress.jsonl` of `attempt` under `fleet_root` from byte `offset`,
/// the offset that the ledger recorded (0 for a file with no row). `owner`
/// is the user that owns the ledger. `None`: no file, or no new complete
/// line.
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
    // Nothing was appended: the pass reads nothing.
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

/// The bytes of the log `path` through the protected file reader, with the
/// outbox as its root. The real path check of the outbox comes first: the
/// outbox itself must not be a link out of its attempt folder.
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

/// The complete lines of `bytes` after byte `offset`, and the byte after
/// the last of them. `None` when no line after `offset` is complete.
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

/// The fields of 1 line as a worker writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    task: String,
    attempt: String,
    at: String,
    kind: String,
    text: String,
}

/// 1 line of `attempt`, with its newline, or the reason of its refusal.
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

/// The real path of the handoff file `path` of a handoff result, when it is
/// inside 1 of `roots` (the worktree and the task folder of the attempt).
/// Else the reason why fleetd does not use it. The path is an untrusted
/// worker path (Q4): only its real path counts, so a link out of a root is
/// not used.
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
