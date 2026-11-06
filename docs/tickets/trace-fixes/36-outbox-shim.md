# 36: Remove the rusqlite re-export from the outbox (arc_25)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core --test arch_scan && cargo test -p horch-core -- outbox trf_21 && cargo test -p horch -- wake trf_21`
**Own:** `crates/horch-core/src/taskledger/outbox.rs`, `crates/horch/src/cmd/taskledger_wake.rs`, `crates/horch/Cargo.toml`, `Cargo.lock`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 21.
Prior work: ticket 14, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/14.md`.
Waits for: nothing.

## Goal

The run branch fails `arc_25_no_shim_modules` in `crates/horch-core/tests/arch_scan.rs`. The cause is
`pub use rusqlite::{Connection, Transaction};` at `crates/horch-core/src/taskledger/outbox.rs:12`, which ticket 14
added so `crates/horch/src/cmd/taskledger_wake.rs` can name the types. Remove the shim.

## Steps

1. Read `arc_25_no_shim_modules` and `arc_25_scan_tells_shims_from_module_apis` in `crates/horch-core/tests/arch_scan.rs`. Learn what the scan accepts.
2. Delete the `pub use rusqlite::{...}` line in `outbox.rs`.
3. In `taskledger_wake.rs`, name the types another way that the scan accepts. Prefer a direct `rusqlite` dependency of the `horch` crate, with the same version and features as `crates/horch-core/Cargo.toml`, if the horch crate does not have one yet. If `crates/horch/Cargo.toml` already depends on rusqlite, only change the `use` lines.
4. Change no behaviour. Every `trf_21_*` test still passes.
5. Run the Check.

Out of scope: wake naming for parked tasks (ticket 22).

## Done when

`cargo test -p horch-core --test arch_scan` passes on the run branch, and the Check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/36.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
