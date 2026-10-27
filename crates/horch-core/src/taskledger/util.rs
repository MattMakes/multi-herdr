//! Small helpers every task ledger module uses.

use anyhow::Result;
use rusqlite::Connection;

use super::model::{Actor, EventKind};

/// A fresh row id.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// "Now" as the ledger writes it: the one clock (`HORCH_NOW` pins it).
pub fn now() -> String {
    crate::clock::now_stamp()
}

/// Seconds from `from` to `to`, both ledger stamps; `None` when one does not
/// parse.
pub fn seconds_between(from: &str, to: &str) -> Option<i64> {
    let a = crate::clock::parse(from)?;
    let b = crate::clock::parse(to)?;
    Some((b - a).num_seconds())
}

/// The ids an event is about.
#[derive(Debug, Clone, Default)]
pub struct About<'a> {
    pub run: Option<&'a str>,
    pub task: Option<&'a str>,
    pub worker: Option<&'a str>,
}

/// Append 1 event.
pub fn event(
    conn: &Connection,
    about: &About<'_>,
    actor: Actor,
    kind: EventKind,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            now(),
            about.run,
            about.task,
            about.worker,
            actor.as_str(),
            kind.as_str(),
            text
        ],
    )?;
    Ok(())
}

/// This machine's name, for `workers.host`. A PID names a process only on
/// the host that recorded it.
pub fn host_name() -> String {
    if let Some(h) = std::env::var_os("HORCH_HOSTNAME").filter(|v| !v.is_empty()) {
        return h.to_string_lossy().into_owned();
    }
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: the buffer is 256 bytes and gethostname writes at most that.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if rc == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Some(v) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            return v.to_string_lossy().into_owned();
        }
    }
    "localhost".to_string()
}

/// Whether 2 repo-relative paths overlap: equal, or 1 is a directory that
/// holds the other. `src/a` and `src/a/b.rs` overlap; `src/a` and `src/ab`
/// do not.
pub fn paths_overlap(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

/// A JSON array of strings, for a TEXT column.
pub fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

/// The strings of a JSON-array TEXT column; empty on anything else.
pub fn parse_json_list(text: Option<&str>) -> Vec<String> {
    text.and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_by_path_component() {
        assert!(paths_overlap("src/a", "src/a"));
        assert!(paths_overlap("src/a", "src/a/b.rs"));
        assert!(paths_overlap("src/a/b.rs", "src/a/"));
        assert!(!paths_overlap("src/a", "src/ab"));
        assert!(!paths_overlap("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn seconds_between_stamps() {
        assert_eq!(
            seconds_between("2026-10-27T10:00:00Z", "2026-10-27T10:10:00Z"),
            Some(600)
        );
        assert_eq!(seconds_between("x", "2026-10-27T10:10:00Z"), None);
    }
}
