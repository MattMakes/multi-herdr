//! OpenCode-specific glue: recovering a session id after launch.
//!
//! OpenCode mints its own `ses_...` ids and offers no way to supply one, so a
//! fresh worker's resume handle has to be recovered the way codex's is. Unlike
//! codex there are no rollout files to scan: `opencode session list` is the
//! supported interface, it takes `--format json`, and every record carries the
//! `directory` it belongs to and when it was created. That is enough to tell one
//! worker's session from another's without reading OpenCode's database.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::agent;

/// A session that could belong to this worker.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCandidate {
    pub session_id: String,
    pub created: SystemTime,
}

/// Sessions created at or after `since` whose directory is `project_dir`,
/// newest first.
///
/// The directory check is what keeps two projects' concurrent OpenCode sessions
/// apart, exactly as the cwd check does for codex rollouts.
pub fn find_sessions(project_dir: &str, since: SystemTime) -> Vec<SessionCandidate> {
    let output = Command::new(agent::opencode_bin())
        .args(["session", "list", "--format", "json"])
        .current_dir(project_dir)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_sessions(&String::from_utf8_lossy(&output.stdout), project_dir, since)
}

/// Split out from [`find_sessions`] so the filtering is testable without an
/// OpenCode install.
pub fn parse_sessions(json: &str, project_dir: &str, since: SystemTime) -> Vec<SessionCandidate> {
    let Ok(records) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    // The launch marker's timestamp has whole-second resolution on some
    // filesystems while OpenCode records milliseconds, so a session created in
    // the same second as the marker would otherwise be missed.
    let since = since - Duration::from_secs(1);
    let mut found: Vec<SessionCandidate> = records
        .iter()
        .filter(|r| same_directory(r.get("directory").and_then(|d| d.as_str()), project_dir))
        .filter_map(|r| {
            let created = millis(r.get("created")?)?;
            Some(SessionCandidate {
                session_id: r.get("id")?.as_str()?.to_string(),
                created,
            })
        })
        .filter(|c| c.created >= since)
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.created));
    found
}

/// Whether a record's directory is the project this worker runs in.
///
/// Compared after resolving symlinks when possible: a herdr pane's cwd and the
/// path OpenCode records can differ by `/tmp` versus `/private/tmp` on macOS,
/// and a string compare would silently find nothing.
fn same_directory(recorded: Option<&str>, project_dir: &str) -> bool {
    let Some(recorded) = recorded else {
        return false;
    };
    if recorded == project_dir {
        return true;
    }
    let canonical = |p: &str| std::fs::canonicalize(Path::new(p)).ok();
    match (canonical(recorded), canonical(project_dir)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn millis(value: &serde_json::Value) -> Option<SystemTime> {
    let ms = value.as_u64()?;
    Some(UNIX_EPOCH + Duration::from_millis(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    const LIST: &str = r#"[
        {"id":"ses_new","title":"t","updated":1788648167866,"created":1788648167000,
         "projectId":"global","directory":"/proj/mine"},
        {"id":"ses_old","title":"t","updated":1788648100000,"created":1700000000000,
         "projectId":"global","directory":"/proj/mine"},
        {"id":"ses_elsewhere","title":"t","updated":1788648167900,"created":1788648167900,
         "projectId":"global","directory":"/proj/other"}
    ]"#;

    /// The two filters that keep workers apart: another project's session is not
    /// this worker's, and neither is one that existed before it launched.
    #[test]
    fn only_this_project_and_only_after_launch() {
        let found = parse_sessions(LIST, "/proj/mine", at(1788648160000));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].session_id, "ses_new");
    }

    #[test]
    fn newest_first() {
        let json = r#"[
            {"id":"ses_a","created":1000000,"directory":"/proj/mine"},
            {"id":"ses_c","created":3000000,"directory":"/proj/mine"},
            {"id":"ses_b","created":2000000,"directory":"/proj/mine"}
        ]"#;
        let found = parse_sessions(json, "/proj/mine", at(0));
        let ids: Vec<&str> = found.iter().map(|c| c.session_id.as_str()).collect();
        assert_eq!(ids, ["ses_c", "ses_b", "ses_a"]);
    }

    /// A session created in the same second the marker was written still counts:
    /// the marker's mtime can be coarser than OpenCode's millisecond stamp.
    #[test]
    fn a_session_from_the_launch_second_is_not_missed() {
        let found = parse_sessions(
            r#"[{"id":"ses_a","created":1788648167100,"directory":"/proj/mine"}]"#,
            "/proj/mine",
            at(1788648167900),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    /// Output that is not the JSON this expects must yield nothing rather than
    /// panicking a background thread nobody is watching.
    #[test]
    fn unparseable_output_is_empty_not_fatal() {
        assert!(parse_sessions("not json", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions("[]", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions(r#"[{"id":"x"}]"#, "/proj/mine", at(0)).is_empty());
    }
}
