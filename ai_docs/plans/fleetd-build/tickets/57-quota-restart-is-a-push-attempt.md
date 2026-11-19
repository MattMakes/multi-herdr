# A task that restarts on its fallback after a usage limit starts as a push attempt, not by the old claim spawn (RR-11)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-e2e --test p7_exec && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/quota.rs`

## Goal

When an attempt ends with the outcome `quota`, fleetd restarts the task on the fallback persona as
a push attempt through the same launch path as each other attempt. The restart attempt has its
`task.yaml` and `START.md`, runs the task, and burns no attempt. The 2 tests of
`crates/horch-e2e/tests/p7_exec.rs` that are listed with this cause are green.

## Context

Found by T-892 (opus-hardening-179) with the tests of T-603 (opus-148). After T-892, a usage limit
ends attempt A1 with `quota`, the task goes to the fallback, and no attempt burns. Then:

- `quota.rs` `move_to_fallback` (about line 285, `store::insert_attempt`) writes the restart
  attempt with `via=claim`.
- `quota::restart` starts it with `horch spawn <persona>` (the old claim spawn; T-550 retired the
  worker kinds of that path).
- That spawn writes no `task.yaml` and no `START.md` in `attempts/A2-fallback` (the folder has
  only `exit.json`, code 0). The agent has no task and exits 0. The worker ends `launch_failed`
  'agent exited with code 0', and A2-fallback ends `timed_out` in `launching`.

The red tests: `exec_usage_limit_restarts_on_the_fallback_and_burns_no_attempt` and
`new_attempt_gets_the_256000_window` in `crates/horch-e2e/tests/p7_exec.rs`, listed in
`crates/horch-e2e/expected-failures/p7_exec.txt`.

Read first: `horch ledger show T-892`, `T-603`, `T-890` and `T-550`; fleetd-p7 tickets 17 and 33
(`docs/tickets/fleetd-p7/`) and MAP.md Q14; in `docs/specs/fleetd/design.md` RR-11, the quota
outcome, the fallback and the launch states of an attempt.

## Steps

1. Run the 2 red tests on the run branch (with T-890, T-892 and T-603 landed). Check: they fail at
   the restart, and a progress note gives the evidence (the attempt row, `via`, the folder of
   A2-fallback).
2. Write 1 unit or pass test that fails today: after the outcome `quota`, the restart attempt is a
   push attempt (`via=push`) of the fallback persona in state `launching`, and no row of the old
   claim path is written.
3. Fix it: `move_to_fallback` writes the restart attempt as a push attempt, and the pass launches
   it by the launch path of `launch.rs` (the task files step writes `task.yaml` and `START.md`).
   Remove `quota::restart` and each `horch spawn <persona>` call that has no caller left. Do not
   keep 2 launch paths. Check: the test of step 2 is green.
4. Search for each other place that writes an attempt with `via=claim` or starts
   `horch spawn <persona>` in production code. For each: move it to the push path in the same
   way, or name it in a NOTE with the reason that it stays. Check: the NOTE has the list.
5. Remove the 2 lines from `crates/horch-e2e/expected-failures/p7_exec.txt` and make the tests
   green 3 times in a row. Check: the file has no line with this cause.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit. No schema
  step.
- Do not make an assert weaker and do not add an arch exception.
- `pass.rs` and `launch.rs` are shared files: small functions, few shared lines, merge the run
  branch each 30 minutes and before finish.
- An e2e test that launches a push attempt serves a test fleetd (`Harness::serve_fleetd`), makes
  its workspace with `Harness::fleet_workspace`, and sets `HORCH_TEST_DOCS_FIXTURE` to an empty
  local folder. Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min in long test runs.

## Done when

- The tests of steps 2 and 5 pass, and the Check passes.
- The done summary names: the list of step 4, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
