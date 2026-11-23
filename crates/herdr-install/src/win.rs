pub fn prepend_path_entry(current: &str, entry: &str) -> Option<String> {
    let normalize = |s: &str| s.trim().trim_end_matches('\\').to_ascii_lowercase();
    let target = normalize(entry);

    let parts: Vec<&str> = current
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .collect();
    let kept: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| normalize(p) != target)
        .collect();
    let already_first = current
        .split(';')
        .next()
        .is_some_and(|p| !p.trim().is_empty() && normalize(p) == target);
    if already_first && kept.len() + 1 == parts.len() {
        return None;
    }

    Some(
        std::iter::once(entry)
            .chain(kept)
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_platform::path::{inspect_junction_target, plan_junction, JunctionAction};

    #[test]
    fn prepend_adds_a_missing_entry_at_the_front() {
        let next = prepend_path_entry(r"C:\Windows;C:\Windows\System32", r"C:\herdr\bin").unwrap();
        assert_eq!(next, r"C:\herdr\bin;C:\Windows;C:\Windows\System32");
    }

    #[test]
    fn prepend_is_a_noop_when_already_first() {
        assert_eq!(
            prepend_path_entry(r"C:\herdr\bin;C:\Windows", r"C:\herdr\bin"),
            None
        );
    }

    #[test]
    fn prepend_matches_case_insensitively_and_ignores_trailing_slashes() {
        assert_eq!(
            prepend_path_entry(r"c:\herdr\bin\;C:\Windows", r"C:\Herdr\Bin"),
            None
        );
    }

    #[test]
    fn prepend_moves_an_existing_entry_rather_than_duplicating_it() {
        let next = prepend_path_entry(r"C:\Windows;C:\herdr\bin", r"C:\herdr\bin").unwrap();
        assert_eq!(next, r"C:\herdr\bin;C:\Windows");
        assert_eq!(next.matches(r"herdr\bin").count(), 1);
    }

    #[test]
    fn prepend_handles_an_empty_and_ragged_path() {
        assert_eq!(prepend_path_entry("", r"C:\h\bin").unwrap(), r"C:\h\bin");
        assert_eq!(
            prepend_path_entry(r";;C:\Windows;", r"C:\h\bin").unwrap(),
            r"C:\h\bin;C:\Windows"
        );
    }

    #[test]
    fn prepend_rewrites_when_a_leading_empty_part_precedes_the_entry() {
        assert_eq!(
            prepend_path_entry(r";C:\herdr\bin;C:\Windows", r"C:\herdr\bin").unwrap(),
            r"C:\herdr\bin;C:\Windows"
        );
    }

    #[test]
    fn prepend_removes_a_later_duplicate_of_the_first_entry() {
        assert_eq!(
            prepend_path_entry(r"C:\herdr\bin;C:\Windows;c:\herdr\bin\", r"C:\herdr\bin").unwrap(),
            r"C:\herdr\bin;C:\Windows"
        );
    }

    #[test]
    fn junction_plan_covers_every_state() {
        assert_eq!(plan_junction(false, false, false), JunctionAction::Create);
        assert_eq!(plan_junction(true, true, false), JunctionAction::Replace);
        assert_eq!(plan_junction(true, false, true), JunctionAction::Replace);
        assert_eq!(plan_junction(true, false, false), JunctionAction::Refuse);
    }

    #[test]
    fn inspecting_a_missing_path_says_create() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            inspect_junction_target(&tmp.path().join("absent")),
            JunctionAction::Create
        );
    }

    #[test]
    fn inspecting_an_empty_directory_says_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(inspect_junction_target(&dir), JunctionAction::Replace);
    }

    #[test]
    fn inspecting_a_directory_with_files_says_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("mine");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("important.txt"), "keep me").unwrap();
        assert_eq!(inspect_junction_target(&dir), JunctionAction::Refuse);
    }

    #[cfg(unix)]
    #[test]
    fn inspecting_a_symlink_says_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("f"), "x").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(inspect_junction_target(&link), JunctionAction::Replace);
    }
}
