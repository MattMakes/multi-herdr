# 14: Codex panes get a writable CARGO_HOME

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_17 && cargo test -p horch-core --lib -- harness::codex`
**Own:** `crates/horch-core/src/harness/codex.rs`, `docs/live-checks/codex-cargo-home.md`, `docs/live-checks/README.md`, `crates/horch-core/tests/godot_rust_support/codex_cargo.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 6, second item (G12)).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

`cargo build` and `cargo clippy` run in a `codex-sol` pane under `workspace-write`.

## Requirements

- **GRS-17**: Codex worker launches ensure Cargo commands can run under `workspace-write` when `~/.cargo` is read-only by providing a writable `CARGO_HOME` fallback (or verified sandbox rules), documented with a live check. Tests: grs_17_codex_launch_provides_writable_cargo_home_or_rule.

## Steps

1. Find out in a real Codex pane whether `cargo check` works under `workspace-write` with a fetched and an unfetched dependency. Record the commands and output in `docs/live-checks/codex-cargo-home.md`, and list the file in `docs/live-checks/README.md`.
2. When `~/.cargo` is not writable, set `CARGO_HOME` for Codex launches to a writable directory under the workspace or state root in `harness/codex.rs`.
3. Write `grs_17_codex_launch_provides_writable_cargo_home_or_rule` against the launch environment.
4. Never use `ANTHROPIC_API_KEY`.
5. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
6. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
7. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The live check shows `cargo check` passing in a Codex pane.
