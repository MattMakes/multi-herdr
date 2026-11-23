use std::path::{Path, PathBuf};

use anyhow::Result;

pub const MAX_INLINE: usize = 600;

const HEAD: usize = 240;

pub fn ensure_tag(message: &str, role: &str) -> String {
    let tag = format!("[{role}]");
    let trimmed = message.trim_start();
    if trimmed.starts_with(&tag) {
        trimmed.to_string()
    } else {
        format!("{tag} {trimmed}")
    }
}

pub fn with_mail_id(line: &str, from: &str, short_id: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with(&format!("[{from} #")) {
        return trimmed.to_string();
    }
    let tagged = format!("[{from} #{short_id}]");
    match trimmed.strip_prefix(&format!("[{from}]")) {
        Some(rest) => format!("{tagged}{rest}"),
        None => format!("{tagged} {trimmed}"),
    }
}

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

pub fn spool_dir(state_root: &Path, workspace: &str) -> PathBuf {
    state_root
        .join("messages")
        .join(super::file_safe(workspace))
}

pub fn spool(dir: &Path, workspace: &str, from: &str, to: &str, message: &str) -> Result<PathBuf> {
    crate::messaging::private::ensure_dir(dir)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!(
        "{stamp}-{}-to-{}-{}.md",
        super::file_safe(from),
        super::file_safe(to),
        &id[..8]
    ));
    let contents = format!("<!-- horch workspace={workspace} from={from} to={to} -->\n{message}");
    crate::messaging::private::write(&path, contents.as_bytes())?;
    Ok(path)
}

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

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn spool_dir_is_0700_and_file_0600() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let dir = spool_dir(root.path(), "w1");
        let path = spool(&dir, "w1", "opus-1", "orchestrator", "long report").unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);
    }

    use super::*;

    #[test]
    fn ensure_tag_adds_a_missing_tag_and_keeps_a_present_one() {
        assert_eq!(ensure_tag("NOTE: x", "sonnet-1"), "[sonnet-1] NOTE: x");
        assert_eq!(
            ensure_tag("  [sonnet-1] NOTE: x", "sonnet-1"),
            "[sonnet-1] NOTE: x"
        );
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
        let path = spool(dir.path(), "w1", "sonnet-1", "orchestrator", &long).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().ends_with(&long));
        let line = reference_line(&long, &path);
        assert!(line.starts_with("[sonnet-1] DONE: Brandon"));
        assert!(line.ends_with(&format!("Read the full text in {}]", path.display())));
        assert!(line.contains(&format!("{} chars in total", long.trim().chars().count())));
        assert!(line.len() < MAX_INLINE);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn r1_spool_dir_is_per_workspace() {
        let root = Path::new("/state");
        assert_eq!(spool_dir(root, "w16"), root.join("messages").join("w16"));
        assert_ne!(spool_dir(root, "w14"), spool_dir(root, "w16"));
        assert_eq!(
            spool_dir(root, "../w16"),
            root.join("messages").join("___w16")
        );
    }

    #[test]
    fn r1_spool_file_names_its_workspace_on_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = spool(dir.path(), "w16", "opus-1", "orchestrator", "[opus-1] hi").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "<!-- horch workspace=w16 from=opus-1 to=orchestrator -->\n[opus-1] hi"
        );
    }

    #[test]
    fn cut_respects_char_boundaries() {
        let s = "é".repeat(300);
        assert_eq!(cut(&s, HEAD).chars().count(), HEAD);
        assert_eq!(cut("short", HEAD), "short");
    }

    #[test]
    fn trf_03_a_tagged_line_carries_the_mail_id_in_its_tag() {
        assert_eq!(
            with_mail_id("[sonnet-1] NOTE: x", "sonnet-1", "0a1b2c3d"),
            "[sonnet-1 #0a1b2c3d] NOTE: x"
        );
        assert_eq!(
            with_mail_id("  [horch] DONE: y\nmore", "horch", "0a1b2c3d"),
            "[horch #0a1b2c3d] DONE: y\nmore"
        );
    }

    #[test]
    fn trf_03_an_untagged_line_gets_a_tag_with_the_mail_id() {
        assert_eq!(
            with_mail_id("the reviews are in", "horch", "0a1b2c3d"),
            "[horch #0a1b2c3d] the reviews are in"
        );
        assert_eq!(
            with_mail_id("[opus-1] hi", "sonnet-1", "0a1b2c3d"),
            "[sonnet-1 #0a1b2c3d] [opus-1] hi"
        );
    }

    #[test]
    fn trf_03_a_line_that_carries_a_mail_id_keeps_it() {
        let line = "[sonnet-1 #0a1b2c3d] NOTE: x";
        assert_eq!(with_mail_id(line, "sonnet-1", "ffffffff"), line);
    }
}
