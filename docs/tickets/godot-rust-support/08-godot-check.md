# 08: godot-check.sh fails on script errors that exit 0

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `python3 -m unittest discover -s scripts/godot/tests -p 'test_godot_check.py' && cargo test -p horch-core --test godot_rust_support -- grs_09 && cargo test -p horch-core --test skills_catalog`
**Own:** `skills/godot-build-verify/scripts/godot-check.sh`, `scripts/godot/tests/test_godot_check.py`, `skills/godot-build-verify/SKILL.md`, `skills/godot-testing/references/headless-gut.md`, `skills/orchestrate/SKILL.md`, `crates/horch-core/tests/skills_catalog.rs`, `crates/horch-core/tests/godot_rust_support/godot_check.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 4, first item).
Waits for: 01 (Scaffold the godot_rust_support test binary); 07 (Rust seats: rust-developer, rust-reviewer, rust-qa-engineer). Wave 4.

## Goal

A land check never passes when Godot exits 0 on a broken script or an empty GUT run.

## Requirements

- **GRS-09**: `skills/godot-build-verify/scripts/godot-check.sh` is an executable wrapper that runs `godot-run.sh` (headless import, project parse check, and optional `dotnet build` / GUT suite), strips ANSI escapes from the log, and exits non-zero (`3`) when any log line contains `SCRIPT ERROR`, `Parse Error`, `Failed to load script`, or `Nothing was run`, even if Godot exited `0`. Tests: grs_09_godot_check_fails_when_godot_exits_zero_with_script_or_gut_errors; Python: test_godot_check_catches_script_error_on_zero_exit, test_godot_check_strips_ansi_gut_errors.

## Steps

1. Write the tests first: `test_godot_check_catches_script_error_on_zero_exit` and `test_godot_check_strips_ansi_gut_errors` with a fake Godot, as `test_godot_run.py` does. Write `grs_09_godot_check_fails_when_godot_exits_zero_with_script_or_gut_errors` in the submodule.
2. Write `godot-check.sh`, mode `0755`, through `godot-run.sh`: import, `dotnet build --nologo` for a C# project, a parse check of every `.gd` outside `.godot/` and `addons/`, and the GUT suite when `.gutconfig.json` or `addons/gut/gut_cmdln.gd` exists or `--gut` is given.
3. Strip ANSI escapes. Exit `3` when a log has `SCRIPT ERROR`, `Parse Error`, `Failed to load script` or `Nothing was run`. Else pass through a non-zero engine or `dotnet` exit code.
4. Add `godot-build-verify/scripts/godot-check.sh` to `EXEMPT_FILES` in `skills_catalog.rs` and assert its mode is `0755`.
5. Name `godot-check.sh` in `skills/godot-build-verify/SKILL.md`, `skills/godot-testing/references/headless-gut.md` and `skills/orchestrate/SKILL.md`.
6. Warm every new fake executable once with a `warm` argument before a test runs it (ADR 0111, `crates/horch-core/tests/common/warm.rs`). A submodule that needs it declares `#[path = "../common/warm.rs"] mod warm;` itself.
7. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
8. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
9. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. A fake Godot that prints a coloured `SCRIPT ERROR` and exits 0 makes `godot-check.sh` exit 3.
