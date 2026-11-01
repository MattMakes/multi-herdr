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

#[test]
fn doctor_is_quiet_when_no_offered_teammate_uses_such_a_harness() {
    // The 3 teammates that run on antigravity.
    let text = doctor_text(
        "extras-watchdog-quiet",
        &["antigravity", "ticket-writer", "spec-writer"],
    );
    assert!(!text.contains("no timer format"), "{text}");
}
