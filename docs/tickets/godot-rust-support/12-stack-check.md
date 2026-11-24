# 12: Suggest the default check for the project stack

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_14 && cargo test -p horch-core --lib -- roster && cargo test -p horch -- taskledger`
**Own:** `crates/horch-core/src/roster/stack_check.rs`, `crates/horch-core/src/roster/mod.rs`, `crates/horch/src/cmd/taskledger.rs`, `skills/orchestrate/SKILL.md`, `crates/horch-core/tests/godot_rust_support/stack_check.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 5, third item).
Waits for: 01 (Scaffold the godot_rust_support test binary); 08 (godot-check.sh fails on script errors that exit 0). Wave 5.

## Goal

`horch ledger run new` without `--check` names the right check for a Rust, Godot, hybrid or Swift project.

## Requirements

- **GRS-14**: `suggested_check(facts)` returns the canonical stack check command for Rust (`Cargo.toml`), Godot (`project.godot`), hybrid Godot+Rust, and Swift (`Package.swift`) projects; `horch ledger run new` without `--check` and missing-check error messages print the suggested command, and `skills/orchestrate/SKILL.md` documents the stack check table. Tests: grs_14_suggested_check_proposes_canonical_command_per_stack.

## Steps

1. Write `grs_14_suggested_check_proposes_canonical_command_per_stack` first.
2. Add `pub fn suggested_check(facts: &ProjectFacts) -> Option<&'static str>` in the new `roster/stack_check.rs` and export it from `roster/mod.rs`. Return the 4 commands of the spec. The hybrid case wins over Godot alone.
3. In `crates/horch/src/cmd/taskledger.rs`, print a `NOTE:` with the suggested command when `run new` has no `--check`, and add it to the missing-check error. Keep `--check` and `**Check:** none` as they are.
4. Add the stack check table to `skills/orchestrate/SKILL.md` section 0 step 3.
5. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
6. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
7. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. `horch ledger run new` without `--check` in a Cargo project prints the Rust command.
