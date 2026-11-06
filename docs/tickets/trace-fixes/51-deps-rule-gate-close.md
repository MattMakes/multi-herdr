# 51: The dependency rule knows the horch rusqlite dependency, and a git spawn error does not abort run close

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core --test nfr && cargo test -p horch-core --test arch_scan && ./scripts/check-deps.sh && cargo test -p horch-core -- runs trf_52`
**Own:** `crates/horch-core/tests/nfr.rs`, `scripts/check-deps.sh`, `crates/horch-core/src/taskledger/runs.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 48.
Prior work: ticket 36 (arc_25), ticket 48, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/48.md`, section "Follow-ups outside scope".
Waits for: 48.

## Goal

Two gaps from report 48.

1. `nfr_05_no_new_runtime_crates` in `crates/horch-core/tests/nfr.rs` fails on the run branch. Ticket 36 (commit `a69b0507`) gave the `horch` crate a direct `rusqlite` dependency so `taskledger_wake.rs` can name `Connection` and `Transaction` without the shim that `arc_25_no_shim_modules` forbids. `rusqlite` is already an approved runtime crate (ADR 0001, `docs/adr/0001-machine-wide-sqlite-task-ledger.md`) of `horch-core`, with the same version and features, so the binary links no new crate. The orchestrator decided: allow `rusqlite` in the `horch` list too.
2. `remove_gate_worktree` in `runs.rs` runs `git.try_run(...)?` inside the close transaction. When git cannot be spawned, the `?` aborts the whole `run close`, and the documented `NOTE:` (the gate folder stays) is not printed.

## Requirements

- **TRF-52**: `run close` never fails because the gate folder cannot be removed. Any git error (refusal or spawn error) gives the 1 `NOTE:` line, the folder and its open row stay, and the run closes. Test names start with `trf_52_`.

## Steps

1. Add `"rusqlite"` to the `horch` allowed list in `nfr.rs` and to `allowed_horch` in `scripts/check-deps.sh`, each with a comment: same crate and version as horch-core (ADR 0001); the direct dependency replaces a re-export that arc_25 forbids (ticket 36). Change nothing else in either list.
2. In `remove_gate_worktree`, turn an `Err` from `try_run` into the same `NOTE:` path as `!out.ok`, naming the error. Do not mark the row cleaned then.
3. Test `trf_52_a_git_spawn_error_keeps_the_gate_and_closes_the_run`: a `Git` whose program does not exist, an open gate row whose folder exists; `close_run` succeeds, returns the NOTE, the row stays open.
4. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/51.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
