use std::path::PathBuf;

use horch_core::roster::{suggested_check, ProjectFacts};

const RUST: &str = "cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings";
const GODOT: &str = "bash skills/godot-build-verify/scripts/godot-check.sh";
const SWIFT: &str = "swift test";

#[test]
fn grs_14_suggested_check_proposes_canonical_command_per_stack() {
    for (names, want) in [
        (vec!["Cargo.toml", "src"], Some(RUST)),
        (vec!["project.godot", "player.gd"], Some(GODOT)),
        (vec!["project.godot", "Cargo.toml", "rust"], Some(RUST)),
        (vec!["Package.swift", "Sources"], Some(SWIFT)),
        (vec!["package.json", "README.md"], None),
        (vec![], None),
    ] {
        let facts = ProjectFacts::from_names(names.clone());
        assert_eq!(suggested_check(&facts), want, "{names:?}");
    }

    let skill = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/orchestrate/references/stack-checks.md"),
    )
    .unwrap();
    for command in [RUST, GODOT, SWIFT] {
        assert!(
            skill.contains(&format!("`{command}`")),
            "skills/orchestrate/references/stack-checks.md must name `{command}`"
        );
    }
}
