# 02: horch tell keeps orchestrator mail off the pane while the relay socket is live

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-bugfix` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo test --workspace`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch-e2e/`, `docs/specs/trace-fixes.md`, `CHANGELOG.md`

## Goal

`horch tell orchestrator` never types into the orchestrator pane while the relay socket is live. A refused or unconfirmed relay send leaves the mail for the courier, which redelivers it through the relay.

## Context

T-2 of this run (commit 34b66352) made the courier and the startup probe relay-first. Read its diff: `git show 34b66352`. The T-2 worker reported 1 gap outside its scope: `try_relay` in `crates/horch/src/cmd/messaging.rs` (near line 442) still returns the line for typing in 2 cases:

- `SendError::NotDelivered` while the socket file exists: it prints "typing it instead".
- `SendError::Unconfirmed`: it types the line again with a duplicate warning.

Typed text lands on the operator's draft in the orchestrator pane. The operator reported this interruption as a frequent problem.

## Steps

1. Confirm that `horch tell` enqueues durable mail before it calls `try_relay`. Find the enqueue in `messaging.rs`. If the mail is not durable before `try_relay`, stop and report with `horch tell orchestrator "BLOCKED: ..."`, then end your turn.
2. Write failing tests first in `crates/horch-e2e/tests/relay_first_delivery.rs` (the T-2 test file). Case A: a live relay that answers NotDelivered gives 0 pane writes, and the mail stays pending. Case B: a relay that does not confirm gives 0 pane writes, and the mail stays pending. Check: both tests fail on the current code.
3. Change `try_relay`. While `channel::is_live(&socket)` is true, return `None` for NotDelivered and Unconfirmed, and print 1 stderr line saying the courier redelivers through the relay. Keep pane typing when the relay is not live. Check: the step 2 tests pass.
4. Update `docs/specs/trace-fixes.md` section C2 with the new rule. Extend the CHANGELOG line that T-2 added.
5. Run `cargo test --workspace`. Check: it exits 0. Fix any existing test that pinned the old typing fallback, and name it in the DONE summary.

## Constraints

- Do not use `ANTHROPIC_API_KEY`.
- Do not change worker-pane delivery or the courier.
- Do not add a retry or a sleep.

## Done when

- Cases A and B pass, and `cargo test --workspace` exits 0.

## Report

Send the DONE summary with `horch ledger done`. Name the tests and every changed test.
