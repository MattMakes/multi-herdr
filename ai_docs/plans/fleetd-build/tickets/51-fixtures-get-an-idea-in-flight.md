# The test fixtures give each run an idea in flight, so no test is red for FR-12

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test --workspace --locked --no-run && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`

## Goal

No test of `crates/horch-core` or `crates/horch` fails with the text "has no idea in flight of
its feature ... and a task needs an idea (FR-12)". The fix is in the test fixtures, in as few
places as possible. The product rule FR-12 does not change.

## Context

- URGENT. This fault makes the Check of many other tasks red. Work fast, in small commits, and
  land as soon as the Check passes.
- Commit 2fa277a9 (T-549, ticket 47 of `docs/tickets/fleetd-p6/`) made `tasks::add` refuse a task
  whose run has no idea in flight (FR-12). The old fixtures add no idea. At run tip 9456a215:
  `cargo test -p horch-core` has 656 red tests, and 616 of them have this cause. In
  `cargo test -p horch --bins`, 20 red tests have this cause.
- T-871 (opus-hardening-158) found this. It gives an idea in flight only in the fixtures that it
  moves for the claim. Its logs of the base run: `/tmp/t871-base-core.log` and
  `/tmp/t871-base-horch.log`. Read them for the list of red tests.
- T-871 and T-880 run beside you in the same test code. Merge the run branch often, and keep
  your change small so that it merges cleanly.
- Read the done summary of T-549 (`horch ledger show T-549`) for the 3 cases of `tasks::add`:
  the idea of the parent task, else the youngest idea in flight of the feature of the run, else
  the refusal.

## Steps

1. Find the few shared places where tests make a run or a feature: the test support functions
   and the `#[cfg(test)]` helpers (for example the helpers that make run `r1` and feature
   `F-0001`). Count how many red tests go through each one. Put the counts in a progress note.
2. Make each shared helper also make 1 idea in flight for the feature of its run, through the
   store functions that Phase 6 uses. Do not add a test-only branch to product code, and do not
   make `tasks::add` weaker.
3. Run `cargo test -p horch-core --no-fail-fast` and `cargo test -p horch --no-fail-fast` with a
   time limit. Fix each fixture that is still red for FR-12 and does not use a shared helper.
4. A test whose subject is the FR-12 refusal stays as it is. A red test with another cause is
   not yours: give the counts by cause in the done summary.

## Constraints

- Simplified Technical English. No em dash.
- Test code and test support only. If product code has a fault, send QUESTION.
- Do not make an assert weaker. Do not add a test to an expected-failures list.
- Use a time limit on each test command. Each test uses temporary folders. Never use the live
  ledger.
- NEVER use `ANTHROPIC_API_KEY`.
- Send `horch ledger progress` each 5 min.

## Done when

- The Check passes, and `grep -c "needs an idea (FR-12)"` on the output of the 2 full test runs
  gives only the tests whose subject is the refusal.
- The done summary gives the count of red tests of `cargo test -p horch-core` and `-p horch`
  before and after, by cause, and each helper that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
