# The Context warning repeats on each progress line of a worker in a fleetd attempt

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test context_warn_once && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_cli.rs`, `crates/horch-e2e/tests/context_warn_once.rs`

## Fault

Found by task T-15 (opus-35) on 2026-11-20.

- A worker in a live fleetd attempt runs `horch ledger progress`.
- `note_check` in `crates/horch/src/cmd/messaging.rs` (near line 872) writes the context-warned event with `SessionLedger::record_event`.
- `refuse_fleet_caller` refuses that write: `refused inside a fleet attempt ... (session_records)`.
- The code ignores the error. Thus the Context warning prints again on each progress line.

The rule is: the warning shows 1 time per session past the limit (G5, the 256k context limit).

## Goal

A worker in a live fleetd attempt gets the Context warning 1 time. The record of the warning goes through a path that the design permits for an attempt (a request to fleetd, as `worker_progress` of `handlers_cli.rs` does). Do not weaken `refuse_fleet_caller`. Do not ignore a refused write without a record: a refused write is an error in the log or a designed no-op.

## Steps

1. Make the new test file `crates/horch-e2e/tests/context_warn_once.rs`: a worker in a live attempt, past the context limit, sends 3 progress lines. Assert: the warning shows 1 time; the event is recorded 1 time. See the test fail.
2. Fix the path. See the test pass 3 times.
3. If the fix needs a file outside Own, send `QUESTION:` with the file.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test.
6. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The new test passes 3 times in a row and the Check passes.
