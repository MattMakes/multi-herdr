# 16: 2 first openers of a new ledger never collide in Store::open

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test execution_store`
**Own:** `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/tests/execution_store.rs`

## Goal

`taskledger::Store::open` succeeds when 2 threads or processes open 1 new ledger file at the same time. The test `store_dirlock_excludes_concurrent_writers` never fails with "database is locked".

## Evidence

- GitHub CI run 42360731045 (job `test (ubuntu-latest)`, commit 42c4f4b3) failed in 1 test: `store_dirlock_excludes_concurrent_writers` in `crates/horch-core/tests/execution_store.rs:303`.
- The error is "database is locked" (SQLite code 5).
- Diagnosis by worker opus-22: `Store::open` in `crates/horch-core/src/taskledger/store.rs:139` sets `PRAGMA journal_mode = WAL` and migrates the schema at each open. 2 first openers can collide.
- The run on 86b21d11 passed. The fault is a latent race, not a fault of PR #342.

## Steps

1. Read `Store::open` and the test. Reproduce the race: run the test in a loop (for example 200 times) and count failures. Write the count in your report.
2. Find the root cause. Confirm or correct the diagnosis above.
3. Fix the cause. Options: a busy timeout set before the WAL switch, the WAL switch and the migration inside 1 exclusive transaction, or the existing dir lock held across them. Do not add a retry loop around the test.
4. Run the loop of step 1 again. It must show 0 failures.
5. Add a test that opens 1 new ledger file from 8 threads at once and expects 8 successes.
6. Run the check.

## Constraints

- Ticket 15 of this run also owns `store.rs`. The ledger runs this ticket after ticket 15 lands. Rebase on the run branch before you start.
- Do not change the schema.

## Done when

- The loop shows 0 failures in 200 runs. The new test passes. The check passes.
- The DONE report names the root cause and the failure count before and after.
