# A stalled task stays stalled until its old process is gone, so no second attempt starts beside a live one (FM-14)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_pass_p2 && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --lib taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/task_execution/application/stall.rs`, `crates/horch-core/src/fleetd/task_execution/domain/stall.rs`, `crates/horch-core/src/fleetd/task_execution/application/retry.rs`, `crates/horch-core/src/fleetd/task_execution/domain/retry.rs`, `crates/horch-core/tests/fleetd_pass_p2.rs`

## Goal

A task whose lease expired stays `stalled` while a process of its old attempt is alive. It moves to
`ready` (rung 6), or to `failed`, only in a pass that sees that the process group of the old
attempt is gone. So a claim can never start attempt 2 beside a live process of attempt 1.

## Context

The worker of T-271 (opus-64) found this fault with its e2e test
`fm14_no_second_attempt_starts_while_the_old_process_lives` in
`crates/horch-e2e/tests/p2_leases.rs`. The facts of that test:

- The old agent ignores SIGTERM, and `stop_grace_s` is 600.
- The lease expires. The same pass moves the task `stalled -> ready` with `not_before` = clock + 30.
- The polite stop is sent, but the old process lives in its grace time.
- 120 s of fleet clock later a new worker runs `horch ledger claim`. The claim passes (attempt 2)
  while the old process is alive.

The design does not allow this. `docs/specs/fleetd/design.md`, the state table (about line 1333):
"`stalled` | fleetd | The process group of the attempt is stopped (rung 3) -> rung 6: `ready` with
`not_before`, or `failed`." FM-14 says that the old attempt is stopped before the next one starts.
With the default times (grace 20 s, backoff 30 s) the timing hides the fault.

Related work on the run branch: T-263 (leases, ticket 08), T-265 (rung 6, ticket 10), T-269 (the
pass of Phase 2, ticket 14), T-698 (the door releases the lease of each exit from a leased state).

## Steps

1. Read the fault first. Write 1 test in `crates/horch-core/tests/fleetd_pass_p2.rs` that fails on
   the run branch: a lease expires, the old process is alive after the stop was sent, the pass
   runs again, and the task must still be `stalled` with no new attempt possible. Check: the test
   is red before your change.
2. Change the pass and the ladder so that the move `stalled -> ready` or `stalled -> failed`
   happens only when the process facts of the pass show that no process of the old attempt is
   alive. The stop (polite, then the hard stop after the grace) is sent 1 time for each rung, as an
   effect after the commit (design rule A24). Check: the test of step 1 is green.
3. Keep these cases correct, with 1 test each when no test proves it today:
   - The process is gone in the same pass as the expiry: the task moves at once, as today.
   - A valid result that is present in the same pass still wins (`verifying`).
   - A cancel still moves the task to `cancelled` only after the process is gone (T-268).
   - The platform cannot tell (`NotSupported`, Windows): name the rule that you choose and why. The
     safe rule is: stay `stalled` until the hard stop was sent and its grace passed, then move.
   - The backoff `not_before` starts when the task moves to `ready`, not at the expiry.
4. Check that the task does not stay `stalled` for ever: when the hard stop was sent and the
   process is still alive after a bound that you name, the pass opens 1 needs-you item and keeps
   the task `stalled`. Add 1 test.
5. Run the guard tests and the full suite: `cargo test --workspace --locked --no-fail-fast`. Name
   each red test. The test of T-271 is not on the run branch yet; opus-64 runs it on your commit.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- 1 door: each state change goes through `transition()`. No direct UPDATE of a task state.
- No new schema step. If you think that you need one, send QUESTION and end your turn.
- Do not change the claim so that it only refuses; the rule of the design is on the state (the
  task is not `ready`). A second guard in the claim is acceptable only as an addition, and only
  if it is small.
- Send `horch ledger progress` each 5 min in long test runs.
- Windows code stays small. Name each Windows risk in the done summary.

## Done when

- The new tests of steps 1, 3 and 4 pass, and the Check passes.
- `cargo test --workspace --locked --no-fail-fast` has no red test.
- The done summary names: the rule for a process that the platform cannot see, the bound of step
  4, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
