use std::path::{Path, PathBuf};

use horch_core::roster::{offered_in, Phase, ProjectFacts, Requirement, Roster, Teammate};

const SEATS: [&str; 3] = ["rust-developer", "rust-reviewer", "rust-qa-engineer"];
const RUST_SKILLS: [&str; 7] = [
    "rust-unsafe-review",
    "rust-async",
    "rust-cargo-workspace",
    "rust-perf",
    "rust-fuzz",
    "rust-testing",
    "rust-error-handling",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn roster() -> Roster {
    let dir = repo().join("teammates");
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

fn seat<'a>(roster: &'a Roster, name: &str) -> &'a Teammate {
    roster
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not in the roster"))
}

#[test]
fn grs_06_rust_team_seats_are_offered_on_cargo_toml_and_reviewer_is_read_only() {
    let roster = roster();
    let cargo = ProjectFacts::from_names(["Cargo.toml"]);
    let other = ProjectFacts::from_names(["package.json"]);
    for name in SEATS {
        let t = seat(&roster, name);
        assert_eq!(t.offer_when, vec!["Cargo.toml".to_string()], "{name}");
        assert_eq!(t.requires, vec![Requirement::Rust], "{name}");
        assert_eq!(t.fallbacks, vec!["codex-sol".to_string()], "{name}");
        assert!(offered_in(t, Some(&cargo)), "{name} offered on Cargo.toml");
        assert!(!offered_in(t, Some(&other)), "{name} hidden elsewhere");
    }
    let dev = seat(&roster, "rust-developer");
    assert_eq!(dev.model.as_deref(), Some("opus"));
    assert_eq!(dev.phase, Some(Phase::Implementation));
    let reviewer = seat(&roster, "rust-reviewer");
    assert_eq!(reviewer.model.as_deref(), Some("opus"));
    assert_eq!(reviewer.phase, Some(Phase::Validation));
    for tool in ["Edit", "Write", "NotebookEdit"] {
        assert!(
            reviewer.disallowed_tools.iter().any(|d| d == tool),
            "rust-reviewer must deny {tool}"
        );
    }
    let qa = seat(&roster, "rust-qa-engineer");
    assert_eq!(qa.model.as_deref(), Some("sonnet"));
    assert_eq!(qa.phase, Some(Phase::Validation));
    for tool in ["Edit", "Write", "NotebookEdit"] {
        assert!(
            !qa.disallowed_tools.iter().any(|d| d == tool),
            "rust-qa-engineer may use {tool}"
        );
    }
}

#[test]
fn grs_07_seven_rust_skills_are_bundled_own_text_and_assigned_to_seats() {
    let copied: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo().join("skills/copied.json")).unwrap())
            .unwrap();
    let entries = copied["skills"].as_array().unwrap();
    for skill in RUST_SKILLS {
        let md = repo().join("skills").join(skill).join("SKILL.md");
        let len = std::fs::metadata(&md)
            .unwrap_or_else(|e| panic!("{}: {e}", md.display()))
            .len();
        assert!(len <= 12 * 1024, "{skill} SKILL.md is {len} bytes");
        let entry = entries
            .iter()
            .find(|e| e["name"] == skill)
            .unwrap_or_else(|| panic!("{skill} missing from copied.json"));
        assert_eq!(entry["copied_files"], serde_json::json!([]), "{skill}");
    }
    let roster = roster();
    let named: Vec<&str> = SEATS
        .iter()
        .flat_map(|n| {
            let t = seat(&roster, n);
            t.skills.iter().chain(&t.available_skills)
        })
        .map(String::as_str)
        .collect();
    for skill in RUST_SKILLS {
        assert!(named.contains(&skill), "{skill} is on no rust seat");
    }
    let on_disk = std::fs::read_dir(repo().join("skills"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("rust-"));
    for name in on_disk {
        assert!(named.contains(&name.as_str()), "{name} is on no rust seat");
    }
}
