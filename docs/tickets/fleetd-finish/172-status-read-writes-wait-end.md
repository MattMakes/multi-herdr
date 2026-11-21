# `horch ledger status` writes on its read-only connection: `workers::waiting` ends a wait and sets `due_at`

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --locked --test status_read_only && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::workers && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-core/src/taskledger/status.rs`, `crates/horch/tests/status_read_only.rs`

## Fault

Task T-209 (ticket 169) did not find the cause. Task T-232 (qa-engineer-49) found it on 2026-11-20.

- `status.rs` `worker_line` (near line 407) calls `workers::waiting` on the read-only connection of the status command.
- `workers::waiting` in `crates/horch-core/src/taskledger/workers.rs` (near line 986) writes in 2 cases: when `waits::judge` gives `Verdict::Ends`, it calls `end_wait`, which inserts an event; when the verdict is `Holds` with a new time, it calls `waits_store::set_due_at`.
- Result: `horch ledger status --json` fails with "attempt to write a readonly database" when a wait of a worker must end (for example the author of a task waits for its reviews, and 1 review is cancelled).
- Evidence: the test `a_cancelled_review_is_queued_again_and_the_sweep_then_passes` of `crates/horch-e2e/tests/taskledger.rs` fails 3 of 3 runs with 6 tests in parallel; the SQL trace of T-209 (`HORCH_TEST_SQL_TRACE`) shows `Insert events` 3 times in `horch ledger status --json`.

## Goal

A read command writes nothing. The status command shows the correct state of a wait (a wait that must end is shown as ended or is not shown) with no write. The write of the wait end stays with a writer (the pass of fleetd or the command that already owns it): find the writer; if no writer ends such a wait, send `QUESTION:` with the evidence.

## Steps

1. Add the failing test to `crates/horch/tests/status_read_only.rs` (the file is from T-209; use its SQL trace hook): a worker has a wait whose verdict is `Ends`; `horch ledger status --json` exits 0 and the trace has 0 write actions. Add a second case for the `set_due_at` branch. See both fail.
2. Split `workers::waiting` into a pure read (the verdicts) and the write of the effects. The status path calls only the read. Each other caller keeps its behavior: list each caller in the commit message.
3. Run the tests 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and both new cases pass 3 times in a row.
