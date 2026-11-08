//! `horch doctor` warns when an offered harness has no timer format and no
//! transcript lookup (#203).

use horch_e2e::harness::{copy_tree, repo_root, Harness};

/// The doctor output for the repo roster, with the teammates in `to_claude`
/// moved from antigravity to claude. An overlay cannot remove a built-in, so
/// the files are rewritten instead.
fn doctor_text(name: &str, to_claude: &[&str]) -> String {
    let mut h = Harness::new(name);
    let roster = h.root.join("roster");
    copy_tree(&repo_root().join("teammates"), &roster);
    for teammate in to_claude {
        let file = roster.join(format!("{teammate}.md"));
        let text = std::fs::read_to_string(&file).unwrap();
        let moved: Vec<String> = text
            .lines()
            .map(|l| match l {
                "agent: antigravity" => "agent: claude".to_string(),
                l if l.starts_with("model: ") => "model: sonnet".to_string(),
                l => l.to_string(),
            })
            .collect();
        std::fs::write(&file, moved.join("\n") + "\n").unwrap();
    }
    h.set("HORCH_TEAMMATES_DIR", roster.to_string_lossy());
    let o = h.run(&["doctor"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn doctor_warns_for_a_harness_the_watchdog_cannot_see() {
    let text = doctor_text("extras-watchdog-agy", &[]);
    assert!(
        text.contains("warning: harness antigravity has no timer format and no transcript lookup"),
        "{text}"
    );
}

/// Every repo teammate that runs on antigravity. The list comes from the
/// roster files, so a new antigravity teammate does not break the test.
fn antigravity_teammates() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(repo_root().join("teammates"))
        .unwrap()
        .filter_map(|e| {
            let path = e.unwrap().path();
            let text = std::fs::read_to_string(&path).ok()?;
            text.lines()
                .any(|l| l == "agent: antigravity")
                .then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    names
}

#[test]
fn doctor_is_quiet_when_no_offered_teammate_uses_such_a_harness() {
    let moved = antigravity_teammates();
    // The scan must find the antigravity teammates, or the test proves nothing.
    for known in [
        "antigravity",
        "audio-reviewer",
        "spec-writer",
        "ticket-writer",
    ] {
        assert!(moved.iter().any(|m| m == known), "{known} not in {moved:?}");
    }
    let moved: Vec<&str> = moved.iter().map(String::as_str).collect();
    let text = doctor_text("extras-watchdog-quiet", &moved);
    assert!(!text.contains("no timer format"), "{text}");
}
