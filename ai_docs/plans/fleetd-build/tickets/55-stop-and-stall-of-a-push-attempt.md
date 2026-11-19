# A stop and a stall of a push attempt end its agent: worker_stop ends the live attempt, each stop is recorded, and a stalled task stays stalled while the old agent lives

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_pass_p2 && cargo test -p horch-e2e --test p5_push_stop && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p5_push_stop.rs`

## Goal

For an attempt that fleetd launched by push: `horch ledger stop <worker>` ends the live attempt
and its agent, each stop that fleetd sends is recorded so that a second stop sends nothing, and a
task whose attempt stalled stays `stalled` while a process of the old attempt lives (FM-14). The 3
tests of `crates/horch-e2e/tests/taskledger.rs` that T-870 left on the list with owner T-893 are
green.

## Context

The worker of T-870 (opus-hardening-178) moved 4 old pull-flow tests to the push flow. 1 is green.
3 found product gaps (read `horch ledger show T-870` first; it names the tests and the evidence):

- F1, test `stopping_one_of_three_workers_leaves_the_others_running`: `worker_stop` in
  `handlers_worker.rs` ends only the worker row. The push attempt stays live, so `orphans()` in
  `domain/task.rs` never stops its agent. The agent lives 30 s after the stop, and check-ins
  continue.
- F2, same test: the pass stops orphan processes but writes no `stops` row. The test asserts
  stops rows (agent TERM sent, pane `pane_closed`) and a second stop that sends nothing.
- F3, test `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid`: after the
  ladder pass with the old agent alive, the task is `ready`, not `stalled`. FM-14 and ticket
  `ai_docs/plans/fleetd-build/tickets/27-stalled-waits-for-the-stop.md` (landed) say: a task stays
  `stalled` until the process group of its old attempt is gone.
- F4, test `the_whole_flow_on_the_fake_herdr`: needs `horch result` from a pushed agent. T-892
  (a push attempt records its wrapper process) makes this work. This test is yours only after
  T-892 is landed: make it green then.

Read in `docs/specs/fleetd/design.md`: the stop ladder (rungs 3 to 6), FM-14, the state table of a
task and of an attempt, rule A24 (effects after the commit), and what the design says about the
record of a stop. Read the done summaries of T-892, T-881, T-550 and T-884.

## Steps

1. Reproduce each of F1, F2 and F3 through the binary in the new file
   `crates/horch-e2e/tests/p5_push_stop.rs`, 1 test each, with the push launch of the test fleetd
   and no stand-in wrapper. Check: each test is red, and a progress note gives the evidence.
2. F3 first: find why the rule of ticket 27 does not hold for a push attempt (most likely the
   process facts of the pass do not have the PID of a push attempt; T-892 records it). Merge the
   run branch when T-892 is landed and run the test again before you change code. Check: a
   progress note names the cause.
3. F1: `worker_stop` ends the live attempt of that worker through `transition()`, so that the pass
   stops its agent with the ladder. Only the named worker is stopped. Check: the F1 test is green
   up to the stops asserts.
4. F2: follow the design. If the design records each stop that fleetd sends (a `stops` row or its
   new form), write it in the same transaction as the decision, and send the signal as an effect
   after the commit, 1 time for each rung. If the design names another record, send QUESTION with
   the place in the design before you change an assert of the old test. Check: a second stop sends
   nothing.
5. Make the 3 tests of `taskledger.rs` with owner T-893 green and remove their lines from
   `crates/horch-e2e/expected-failures/taskledger.txt`. `the_whole_flow_on_the_fake_herdr` needs
   T-892 on the run branch.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit.
- No schema step. If you think that you need one, send QUESTION and end your turn.
- Do not make an assert weaker and do not add an arch exception.
- `pass.rs`, `handlers_worker.rs` and `cmd/fleetd.rs` are changed by other tasks at the same time
  (T-892, T-402, T-554, T-890). Put your code in small functions, change few shared lines, merge
  the run branch each 30 minutes and before finish, and keep both parts in a conflict.
- Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min in long test runs.
- Windows code stays small. Name each Windows risk in the done summary.

## Done when

- The tests of step 1 pass, the 3 tests of `taskledger.rs` are green and on no list, and the Check
  passes.
- The done summary names: the cause of F3, the record of a stop that you used and its place in
  the design, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
