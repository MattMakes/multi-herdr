//! What `horch tell` types into a pane, as opposed to how it types it.
//!
//! Two rules protect the channel's one trust boundary: a worker line carries a
//! `[<role>]` tag, and everything else is the human operator.
//!
//! - A worker's message always starts with its own tag, even when the worker
//!   forgot it.
//! - A long message is not typed at all. It goes to a file, and the pane gets
//!   one short tagged line that names the file. A short line is far less exposed
//!   to a busy TUI splitting it, and even a split one starts with the tag.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Longest message typed into a pane as is. Anything longer goes by reference.
pub const MAX_INLINE: usize = 600;

/// How much of a long message the reference line quotes: enough for the tag,
/// the keyword and the outcome sentence.
const HEAD: usize = 240;

/// `message`, starting with `[role] ` unless it already does.
pub fn ensure_tag(message: &str, role: &str) -> String {
    let tag = format!("[{role}]");
    let trimmed = message.trim_start();
    if trimmed.starts_with(&tag) {
        trimmed.to_string()
    } else {
        format!("{tag} {trimmed}")
    }
}

/// `summary` without a leading `[role]` and `DONE:`, which `horch done` adds
/// itself. Workers are told to open every report with both, so they often do.
pub(crate) fn strip_done_prefix<'a>(summary: &'a str, role: &str) -> &'a str {
    let mut s = summary.trim_start();
    if let Some(rest) = s.strip_prefix(&format!("[{role}]")) {
        s = rest.trim_start();
    }
    if s.get(..5).is_some_and(|k| k.eq_ignore_ascii_case("DONE:")) {
        s = s[5..].trim_start();
    }
    s
}

/// Directory under `state_root` (`RuntimeContext::paths.state_root`) that
/// holds messages delivered by reference.
pub fn spool_dir(state_root: &Path) -> PathBuf {
    state_root.join("messages")
}

/// Write `message` to a new file under `dir` and return its path.
pub fn spool(dir: &Path, from: &str, to: &str, message: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!(
        "{stamp}-{}-to-{}-{}.md",
        file_safe(from),
        file_safe(to),
        &id[..8]
    ));
    std::fs::write(&path, message).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// The one line typed into the pane for a message spooled at `path`: the head
/// of the message, so the tag and keyword stay first, then where the rest is.
pub fn reference_line(message: &str, path: &Path) -> String {
    let flat: String = message
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let head = cut(&flat, HEAD);
    format!(
        "{head} ... [message truncated for delivery: {} chars in total. Read the full text in {}]",
        message.trim().chars().count(),
        path.display()
    )
}

/// At most `max` chars of `s`, cut at the last space when there is one nearby.
fn cut(s: &str, max: usize) -> &str {
    let Some((end, _)) = s.char_indices().nth(max) else {
        return s;
    };
    let head = &s[..end];
    match head.rfind(' ') {
        Some(space) if space > max / 2 => head[..space].trim_end(),
        _ => head,
    }
}

fn file_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_tag_adds_a_missing_tag_and_keeps_a_present_one() {
        assert_eq!(ensure_tag("NOTE: x", "sonnet-1"), "[sonnet-1] NOTE: x");
        assert_eq!(
            ensure_tag("  [sonnet-1] NOTE: x", "sonnet-1"),
            "[sonnet-1] NOTE: x"
        );
        // Another role's tag is not this worker's tag.
        assert_eq!(
            ensure_tag("[opus-1] hi", "sonnet-1"),
            "[sonnet-1] [opus-1] hi"
        );
    }

    #[test]
    fn strip_done_prefix_removes_what_done_adds() {
        assert_eq!(
            strip_done_prefix("[opus-3] DONE: The report.", "opus-3"),
            "The report."
        );
        assert_eq!(
            strip_done_prefix("DONE: The report.", "opus-3"),
            "The report."
        );
        assert_eq!(
            strip_done_prefix("done:The report.", "opus-3"),
            "The report."
        );
        assert_eq!(
            strip_done_prefix("[opus-3] The report.", "opus-3"),
            "The report."
        );
        assert_eq!(strip_done_prefix("The report.", "opus-3"), "The report.");
        assert_eq!(
            strip_done_prefix("[opus-2] DONE: x", "opus-3"),
            "[opus-2] DONE: x"
        );
        assert_eq!(strip_done_prefix("DONE", "opus-3"), "DONE");
        assert_eq!(strip_done_prefix("ééé", "opus-3"), "ééé");
    }

    #[test]
    fn reference_line_keeps_the_tag_first_and_names_the_file() {
        let long = format!(
            "[sonnet-1] DONE: {}",
            "Brandon and nova were other authors. ".repeat(40)
        );
        let dir = tempfile::tempdir().unwrap();
        let path = spool(dir.path(), "sonnet-1", "orchestrator", &long).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), long);
        let line = reference_line(&long, &path);
        assert!(line.starts_with("[sonnet-1] DONE: Brandon"));
        assert!(line.ends_with(&format!("Read the full text in {}]", path.display())));
        assert!(line.contains(&format!("{} chars in total", long.trim().chars().count())));
        assert!(line.len() < MAX_INLINE);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn cut_respects_char_boundaries() {
        let s = "é".repeat(300);
        assert_eq!(cut(&s, HEAD).chars().count(), HEAD);
        assert_eq!(cut("short", HEAD), "short");
    }
}
