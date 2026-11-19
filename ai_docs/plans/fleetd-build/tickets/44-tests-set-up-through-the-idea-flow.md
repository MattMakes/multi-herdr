# Each test that used a retired run or plan command as setup uses 1 shared setup helper, and no such test is red

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test --workspace --locked --no-fail-fast && cargo fmt --all --check`
**Own:** `crates/horch/tests/`, `crates/horch-e2e/tests/`, `crates/horch-e2e/src/`, `crates/horch-core/tests/`, `crates/horch-core/src/taskledger/` (test modules only)

## Goal

No test is red because a run or plan command of `horch ledger` is retired, or because
`tasks::add` refuses a task with no idea (FR-12). Each such test makes its run, idea, feature and
tasks through 1 shared setup helper.

## Context

- T-549 (ticket 48 of `docs/tickets/fleetd-p6/`) retired the run and plan request kinds. About 30
  test files used those commands as setup, for example `crates/horch-e2e/tests/taskledger.rs`
  (68 calls), `crates/horch/tests/w3_ledger_cli_pins.rs` (28 calls) and
  `crates/horch/tests/trf_11_add_map_cli.rs` (12 calls). T-549 also made `tasks::add` refuse a
  task with no idea, so lib tests that add a task to a bare run are red. The orchestrator let
  T-549 land with these tests red, and this ticket corrects them.
- Read the done summaries of T-549, T-550, T-880 and T-871 first (`horch ledger show T-<n>`).
  T-880 made the test fleetd helper, and T-871 made the bind helper. Reuse them.
- Other tasks run beside this one. Merge the run branch often.

## Steps

1. Run the Check with `--no-fail-fast` and list each red test with its cause. Put the count in a
   progress note.
2. Write 1 shared setup helper for each crate that needs it. It makes a run, an idea in flight,
   its feature and its tasks through the paths that Phase 6 keeps (`horch idea add --direct` with
   a ticket file, or the store functions that fleetd uses). Check: 1 test of the helper.
3. Move each red test to the helper. Keep what the test proves. Do not make an assert weaker.
4. A test that proves only a retired command: remove it, and name it in the done summary with
   the reason. A test that pins a retired command as refused stays.
5. A red test with another cause: fix it if the fix is small and name it, or send QUESTION.
6. Run the Check. Each red test is yours to fix.

## Constraints

- Simplified Technical English. No em dash.
- Test code and test support only. If product code has a fault, send QUESTION.
- Do not add a test to an expected-failures list, and do not add an exemption to a guard test.
- No test depends on the speed of the host. Each test uses temporary folders. Never use the live
  ledger. Never install a development build.
- Use a timeout on each test command. A test run that does not end in 30 minutes hangs: stop it
  and find the cause.
- NEVER use `ANTHROPIC_API_KEY`.
- Send `horch ledger progress` each 5 min in long test runs.

## Done when

- The Check passes with no red test.
- The done summary names the helper, the count of moved tests, each removed test with its reason,
  and each test fixed for another cause.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
