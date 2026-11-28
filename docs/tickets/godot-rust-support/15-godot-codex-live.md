# 15: Close the godot-run.sh Codex pane live check

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_18`
**Own:** `docs/live-checks/godot-codex.md`, `docs/specs/godot.md`, `crates/horch-core/tests/godot_rust_support/live_checks.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 6, third item (G13)).
Waits for: 01 (Scaffold the godot_rust_support test binary); 10 (C# skills on godot-code-reviewer and godot-qa-engineer). Wave 3.

## Goal

The last open item of the Godot wave is closed with evidence.

## Requirements

- **GRS-18**: `docs/live-checks/godot-codex.md` records a direct `godot-run.sh` verification inside a Codex sandbox environment and `docs/specs/godot.md` marks the pending Codex `godot-run.sh` live check complete. Tests: grs_18_godot_codex_live_check_is_recorded_and_spec_status_closed.

## Steps

1. Run `scripts/live/godot-codex.sh` inside a real Codex pane. Record the `godot-run.sh` run and its output in `docs/live-checks/godot-codex.md`.
2. Mark the Codex `godot-run.sh` live check complete on line 5 of `docs/specs/godot.md`.
3. Write `grs_18_godot_codex_live_check_is_recorded_and_spec_status_closed`.
4. If the host has no Codex or Godot, send BLOCKED with the command you need.
5. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
6. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The live-check file has a dated run with its output.
