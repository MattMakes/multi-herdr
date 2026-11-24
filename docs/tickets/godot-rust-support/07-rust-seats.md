# 07: Rust seats: rust-developer, rust-reviewer, rust-qa-engineer

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_06 grs_07 && cargo test -p horch-core --test skills_catalog && cargo test -p horch-core --test baseline_oracles`
**Own:** `teammates/rust-developer.md`, `teammates/rust-reviewer.md`, `teammates/rust-qa-engineer.md`, `crates/horch-core/tests/baseline_oracles.rs`, `crates/horch-core/tests/skills_catalog.rs`, `teammates/README.md`, `docs/skills-and-teams.md`, `crates/horch-core/tests/godot_rust_support/rust_team.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 3, first item).
Waits for: 01 (Scaffold the godot_rust_support test binary); 04 (doctor: Rust requirement, project-fact tool checks and Git LFS); 06 (Rust skills: perf, fuzz, testing). Wave 3.

## Goal

An orchestrator in a Cargo project gets a Rust builder, a read-only Rust reviewer and a Rust QA seat.

## Requirements

- **GRS-06**: The roster includes `rust-developer` (Opus implementation builder), `rust-reviewer` (Opus validation reviewer denying `Edit`, `Write`, `NotebookEdit`), and `rust-qa-engineer` (Sonnet validation QA), each setting `offer_when: ["Cargo.toml"]`, `requires: [rust]`, and `fallbacks: [codex-sol]`. Tests: grs_06_rust_team_seats_are_offered_on_cargo_toml_and_reviewer_is_read_only.
- **GRS-07**: The bundled skill catalog includes seven own-text Rust skills (`rust-unsafe-review`, `rust-async`, `rust-cargo-workspace`, `rust-perf`, `rust-fuzz`, `rust-testing`, `rust-error-handling`) within the size budget, with `"copied_files": []` in `skills/copied.json`, and every `rust-*` skill is expected or available on at least one `rust-*` seat. Tests: grs_07_seven_rust_skills_are_bundled_own_text_and_assigned_to_seats.

## Steps

1. Write the tests first: `grs_06_rust_team_seats_are_offered_on_cargo_toml_and_reviewer_is_read_only` and `grs_07_seven_rust_skills_are_bundled_own_text_and_assigned_to_seats`.
2. Create the 3 teammate files with the exact frontmatter in the spec. Write a short persona body for each.
3. Add the 3 names in alphabetical order to `SKIP_NEW_TEAMMATES` in `baseline_oracles.rs` and `skills_catalog.rs`.
4. Document the Rust team in `teammates/README.md` and `docs/skills-and-teams.md`.
5. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
6. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
7. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. `horch teammates` in a directory with `Cargo.toml` offers the 3 seats. `rust-reviewer` denies `Edit`, `Write` and `NotebookEdit`.
