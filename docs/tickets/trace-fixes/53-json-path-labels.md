# 53: No label or diagnostic names the retired JSON session ledger

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch -- trf_45 recipes cost smoke sessions done && cargo build --workspace --bins && cargo test -p horch-e2e --test lifecycle --test messaging`
**Own:** `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/cost.rs`, `crates/horch/src/cmd/smoke.rs`, `crates/horch/src/cmd/sessionscmd.rs`, `crates/horch/src/main.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 29.
Prior work: ticket 29, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/29.md`, section "Follow-ups outside scope".
Waits for: 29.

## Goal

Ticket 29 moved session records from `<state>/<slug>.json` into the `session_records` table (`ExecutionStore::db()`, `stored_json()` in `crates/horch-core/src/execution/store.rs`). Nothing writes the JSON file now, but some text still points at it.

## Requirements

- **TRF-45** (continued): no user-visible label or diagnostic names the JSON session-ledger file as the place where sessions live. Test names start with `trf_45_`.

## Steps

1. `recipes.rs:535` prints `Session ledger: <path>`. Print the database path (`store.db()`) with a label that says it is the task ledger's session records.
2. `cost.rs:157`: the same change.
3. `smoke.rs:351` `ledger_dump` reads the JSON file on a failed smoke. Print `stored_json()` instead.
4. `sessionscmd.rs:255` (`horch ledger legacy path`): print the database path, and say in 1 line that old `<slug>.json` files are read only.
5. `main.rs` `done_command` still picks `done_cmd` for a ledger worker; `messaging::done` does that now. Make `done_command` call `messaging::done` only.
6. 1 `trf_45_` test per changed label. Run the Check (it builds the binaries before the e2e tests, which run `target/debug/horch`).

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

Out of scope: docs (ticket 30).

## Done when

The Check passes, and `git grep -n "Session ledger:" crates/` finds nothing.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/53.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
