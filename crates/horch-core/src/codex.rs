//! Codex-specific glue: execpolicy rules, and recovering a session id after
//! launch.
//!
//! Codex differs from Claude in two ways that matter to the fleet. It loads
//! execpolicy rules at startup, so the fleet commands must be explicitly allowed
//! to run outside its sandbox or a worker can neither report back nor shut itself
//! down. And it mints its session id itself, only revealing it after the process
//! starts, so the ledger has to harvest it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::prompts;
use crate::teammates::Roster;

/// Path to codex's rules file: `~/.codex/rules/default.rules`.
pub fn rules_path(home: &Path) -> PathBuf {
    home.join(".codex").join("rules").join("default.rules")
}

/// The directory codex writes rollout files under.
pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join(".codex").join("sessions")
}

/// Append any missing fleet `prefix_rule`s to codex's rules file.
///
/// Idempotent: a rule whose pattern is already present is left alone, so the
/// user's own rules and formatting survive.
pub fn ensure_rules(home: &Path, roster: &Roster) -> Result<()> {
    let path = rules_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut appended = String::new();
    for rule in roster.exec_rules() {
        // Match on the rendered pattern list, exactly as the bash `grep -qF` did.
        if existing.contains(&format!("[{}]", rule.pattern)) {
            continue;
        }
        appended.push_str(&prompts::codex_rule_block(rule));
    }
    if appended.is_empty() {
        return Ok(());
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&appended);
    std::fs::write(&path, next).with_context(|| format!("writing {}", path.display()))
}

/// Pull the session uuid out of a rollout filename.
///
/// Codex names them `rollout-<timestamp>-<uuid>.jsonl`. Returns `None` for
/// anything that does not end in a well-formed uuid, which is what stops a
/// partially written or unrelated file from being recorded as a session.
pub fn session_id_from_rollout(file_name: &str) -> Option<&str> {
    let stem = file_name
        .strip_prefix("rollout-")?
        .strip_suffix(".jsonl")?;
    // The uuid is the trailing 36 characters, preceded by the separating dash.
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    if stem.len() > 36 && !stem[..stem.len() - 36].ends_with('-') {
        return None;
    }
    is_uuid(candidate).then_some(candidate)
}

fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    if groups.len() != 5 {
        return false;
    }
    [8, 4, 4, 4, 12]
        .iter()
        .zip(&groups)
        .all(|(len, g)| g.len() == *len && g.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

/// A rollout file that could belong to this worker.
#[derive(Debug, Clone)]
pub struct RolloutCandidate {
    pub path: PathBuf,
    pub session_id: String,
    pub modified: SystemTime,
}

/// Rollout files created after `since` whose head names `project_dir` as its cwd,
/// newest first.
///
/// The cwd check is what keeps two projects' concurrent codex sessions apart; the
/// bash version grepped the first 16KB for the same JSON fragment.
pub fn find_rollouts(
    sessions_dir: &Path,
    project_dir: &str,
    since: SystemTime,
) -> Vec<RolloutCandidate> {
    let needle = format!("\"cwd\":{}", serde_json::Value::from(project_dir));
    let mut found = Vec::new();
    collect_rollouts(sessions_dir, since, &needle, &mut found);
    // Newest first, matching `ls -t`.
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    found
}

fn collect_rollouts(
    dir: &Path,
    since: SystemTime,
    needle: &str,
    out: &mut Vec<RolloutCandidate>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_rollouts(&path, since, needle, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else { continue };
        if modified <= since {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(session_id) = session_id_from_rollout(name) else {
            continue;
        };
        if !head_contains(&path, needle) {
            continue;
        }
        out.push(RolloutCandidate {
            session_id: session_id.to_string(),
            path,
            modified,
        });
    }
}

/// Does the first 16KB of `path` contain `needle`?
fn head_contains(path: &Path, needle: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 16 * 1024];
    let Ok(read) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(read);
    String::from_utf8_lossy(&buf).contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn extracts_session_ids_from_rollout_filenames() {
        let uuid = "0f10e145-3a7f-4b21-9c8d-2552aabbccdd";
        assert_eq!(
            session_id_from_rollout(&format!("rollout-2026-07-21T10-30-00-{uuid}.jsonl")),
            Some(uuid)
        );
    }

    #[test]
    fn rejects_filenames_without_a_wellformed_uuid() {
        for name in [
            "rollout-2026-07-21-notauuid.jsonl",
            "rollout-.jsonl",
            "rollout-2026-07-21T10-30-00-0F10E145-3A7F-4B21-9C8D-2552AABBCCDD.jsonl",
            "session-0f10e145-3a7f-4b21-9c8d-2552aabbccdd.jsonl",
            "rollout-0f10e145-3a7f-4b21-9c8d-2552aabbccdd.json",
            // A truncated uuid must not be accepted.
            "rollout-x-0f10e145-3a7f-4b21-9c8d-2552aabbccd.jsonl",
        ] {
            assert_eq!(session_id_from_rollout(name), None, "should reject {name}");
        }
    }

    /// The bash version required the uuid to be dash-separated from the
    /// timestamp; a run-on prefix is not a session id.
    #[test]
    fn requires_a_separator_before_the_uuid() {
        let uuid = "0f10e145-3a7f-4b21-9c8d-2552aabbccdd";
        assert_eq!(
            session_id_from_rollout(&format!("rollout-stamp{uuid}.jsonl")),
            None
        );
    }

    fn write_rollout(dir: &Path, uuid: &str, cwd: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("rollout-2026-08-06T00-00-00-{uuid}.jsonl"));
        std::fs::write(&path, format!(r#"{{"cwd":"{cwd}","id":"x"}}"#)).unwrap();
        path
    }

    #[test]
    fn finds_only_rollouts_for_this_project() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        let since = SystemTime::now() - Duration::from_secs(60);

        write_rollout(&sessions.join("2026/08"), "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", "/proj/mine");
        write_rollout(&sessions.join("2026/08"), "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb", "/proj/other");

        let found = find_rollouts(&sessions, "/proj/mine", since);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].session_id, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa");
    }

    /// Sessions that predate this worker's launch belong to someone else.
    #[test]
    fn ignores_rollouts_older_than_the_launch_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        write_rollout(&sessions, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", "/proj/mine");

        let future = SystemTime::now() + Duration::from_secs(3600);
        assert!(find_rollouts(&sessions, "/proj/mine", future).is_empty());
    }

    /// A project path containing characters that need JSON escaping must still
    /// match, which is why the needle is built with a JSON encoder.
    #[test]
    fn matches_project_paths_that_need_json_escaping() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let uuid = "cccccccc-cccc-4ccc-cccc-cccccccccccc";
        std::fs::write(
            sessions.join(format!("rollout-2026-08-06T00-00-00-{uuid}.jsonl")),
            r#"{"cwd":"C:\\Users\\a b\\proj"}"#,
        )
        .unwrap();

        let since = SystemTime::now() - Duration::from_secs(60);
        let found = find_rollouts(&sessions, r"C:\Users\a b\proj", since);
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    #[test]
    fn ensure_rules_is_idempotent_and_preserves_user_content() {
        let home = tempfile::tempdir().unwrap();
        let path = rules_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# my own rules\nprefix_rule(pattern = [\"ls\"])").unwrap();

        ensure_rules(home.path(), &Roster::builtin().unwrap()).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.starts_with("# my own rules\n"));
        assert!(first.contains(r#"pattern = ["horch", "tell", "orchestrator"]"#));
        assert!(first.contains(r#"pattern = ["horch", "note"]"#));
        assert!(first.contains(r#"pattern = ["horch", "done"]"#));

        ensure_rules(home.path(), &Roster::builtin().unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "must not duplicate");
    }

    #[test]
    fn ensure_rules_creates_the_file_when_absent() {
        let home = tempfile::tempdir().unwrap();
        ensure_rules(home.path(), &Roster::builtin().unwrap()).unwrap();
        let written = std::fs::read_to_string(rules_path(home.path())).unwrap();
        assert_eq!(written.matches("prefix_rule(").count(), 3);
        assert!(written.contains(r#"decision = "allow""#));
    }
}
