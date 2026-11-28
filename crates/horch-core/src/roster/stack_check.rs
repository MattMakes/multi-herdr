use super::ProjectFacts;

const RUST: &str = "cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings";
const GODOT: &str = "bash skills/godot-build-verify/scripts/godot-check.sh";
const SWIFT: &str = "swift test";

pub fn suggested_check(facts: &ProjectFacts) -> Option<&'static str> {
    [("Cargo.toml", RUST), ("project.godot", GODOT), ("Package.swift", SWIFT)]
        .into_iter()
        .find(|(marker, _)| facts.has_match(marker))
        .map(|(_, command)| command)
}
