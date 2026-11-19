# The horch-e2e tests that used horch ledger claim as setup use the bind helper of T-871, and no such test is red

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --locked --no-fail-fast && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`

## Goal

Each test of `crates/horch-e2e` that used `horch ledger claim` as setup gets its bound task and
live attempt through the bind helper of T-871. No test of `crates/horch-e2e` is red for the
cause "claim is refused".

## Context

- T-476 made `horch ledger claim` refuse each caller. T-871 removed the task search, wrote the
  bind helper and moved the fixtures of `crates/horch-core` and `crates/horch`. About 100 uses
  in 20 files of `crates/horch-e2e/tests` stay. This ticket moves them.
- Read the done summaries of T-871 (the helper and how to use it), T-880 (the test fleetd helper
  and the tests that it removed) and T-389 (the fleetd launch of an attempt) first
  (`horch ledger show T-<n>`).
- Other tasks run beside this one in the same folder. Merge the run branch often.

## Steps

1. Run `grep -rn "ledger\", \"claim\|ledger claim" crates/horch-e2e` and list the files. Put the
   count in a progress note.
2. Move each use to the bind helper, file by file. Run the tests of the file after each move.
   Keep what the test proves. Do not make an assert weaker.
3. A test whose subject is the old claim itself: remove it, and name it in the done summary with
   the test that covers the new behavior. A test that pins the refusal stays.
4. A red test with another cause: fix it if the fix is small and name it, or name it in the done
   summary with its cause and the ticket that owns it (T-881, T-882, T-884, T-885).
5. Run the Check.

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

- The Check passes, or each red test left has its cause and its owner ticket in the done summary
  and the orchestrator accepted it by an answer to a QUESTION.
- The done summary names the count of moved uses and each removed test with its reason.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
