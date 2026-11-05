# 07: complexity-sweep covers Rust

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- finish::complexity trf_07`
**Own:** `crates/horch-core/src/taskledger/finish/complexity.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Make complexity-sweep check changed Rust functions instead of skipping them (D5).

## Requirements

- **TRF-07**: complexity-sweep reports changed Rust functions over the cognitive-complexity threshold and passes when there are none. Test name starts with `trf_07_`.

## Steps

1. For `.rs` files, run clippy with `clippy::cognitive_complexity` at warn and keep only findings inside functions the branch changed, the way the ts/py path filters.
2. Read the threshold from `clippy.toml` when present, else clippy's default.
3. Keep the ts/js/py analyser path unchanged.

## Done when

A fixture with 1 over-complex changed function fails the step and names it; a clean one passes.

## Report

Write `ai_docs/reports/trace-fixes/07.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
