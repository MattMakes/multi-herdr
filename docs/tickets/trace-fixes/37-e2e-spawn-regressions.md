# 37: Fix the 4 horch-e2e spawn and claim tests that fail on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --test taskledger -- a_spawn_joins_the_run_of_its_own_workspace queue_claim_and_status_through_the_cli spawn_run_names_the_run_the_worker_joins the_whole_flow_on_the_fake_herdr`
**Own:** `crates/horch-e2e/tests/taskledger.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after a report from ticket 15.
Waits for: nothing.

## Goal

On `fleet/trace-fixes` at `df6a2859`, these 4 tests in `crates/horch-e2e/tests/taskledger.rs` fail:
`a_spawn_joins_the_run_of_its_own_workspace`, `queue_claim_and_status_through_the_cli`,
`spawn_run_names_the_run_the_worker_joins`, `the_whole_flow_on_the_fake_herdr`.
Find the root cause of each failure and fix the cause, not the symptom.

## Steps

1. Run the 4 tests on the run-branch tip. Record each failure message in the report.
2. Suspect first: ticket 19 added `refuse_over_plan` in `crates/horch/src/cmd/spawn.rs` (a task-less spawn refuses when no startable task needs it), and ticket 12 made a task with no check refuse to land. Find which landed change breaks each test with `git log --oneline fleet/trace-fixes -- <file>` and the test output.
3. Decide per test: the test pins old behaviour that the spec changed (update the test to the new rule and keep what it proves), or the change has a bug (fix the code).
4. A spawn that the test makes before it queues a task is old behaviour if the spec (D1, gap G-A) says such a spawn now refuses. Then queue the task first in the test.
5. Run the Check, then `cargo test -p horch-e2e --test taskledger` in full. Name any other failure and its owner ticket in the report.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

Out of scope: `a_live_worker_that_does_not_claim_is_reported_once` (ticket 15) and the run-close tests (ticket 13).

## Done when

The 4 tests pass, and the report names the root cause of each.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/37.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
