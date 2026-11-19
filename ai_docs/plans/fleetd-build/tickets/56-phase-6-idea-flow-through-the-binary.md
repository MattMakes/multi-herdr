# The idea flow of Phase 6 works through the binary: fleetd makes the feature branch, a planned task gets its Own rows, and each fake agent plays only its role

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-e2e --test p6_idea_flow_binary && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p6_idea_flow_binary.rs`

## Goal

`horch idea add --direct` through the test fleetd makes the feature and its feature branch, the
planned tasks of the idea have their Own rows, and the fake worker and the fake judge each do only
the work of their role. Then the e2e tests of Phase 6 (T-556 to T-563) can run to their own
asserts.

## Context

The worker of T-561 (opus-153) wrote 6 tests in `crates/horch-e2e/tests/p6_judge_pair.rs`
(commit 130f18ba on its task branch). All 6 are red for causes outside its ticket:

- Fault 1 (blocks each test of Phase 6): fleetd cannot make the feature branch of
  `horch idea add --direct`. Error: `git update-ref refs/heads/fleet/write-the-queue-guide ...
  unable to create directory for .git/refs/heads/fleet/write-the-queue-guide`. Cause that the
  worker found: `crates/horch/src/cmd/fleetd.rs` makes `CliWorktreeGit::new` (about lines 677 and
  929) with no `.writing(..)` call, so the git sandbox denies each write to the project `.git`.
- Fault 2 (found by code read; prove it with a test first): a planned task gets no Own rows in
  `task_files`. `planned_tasks::insert` writes none, so the structural check of a judgment kind
  always fails with no output.
- Fault 3 (found by code read; prove it first): the fake agents of fleetd-p6 ticket 53 (T-555)
  play all keys in each fake. The fake worker also writes `review.md`, and the fake judge runs
  `horch result` before it commits `review.md`.

Other faults of the run branch are in repair by other tasks; do not fix them: the pane of a push
launch and the fleet root of `horch spawn --attempt` (T-890), the wrapper PID of a push attempt
(T-892), the conformance suite on the fakes (T-891).

Read first: `docs/tickets/fleetd-p6/` tickets 14, 17, 51, 52 and 53 and the done summaries of
their tasks; in `docs/specs/fleetd/design.md` the parts on the feature branch of an idea, the git
sandbox of fleetd (which process may write the project `.git`, and which paths), planned tasks
and their Own, and the judge pair (RR-3).

## Steps

1. Fault 1: write 1 e2e test in the new file `crates/horch-e2e/tests/p6_idea_flow_binary.rs`:
   `horch idea add --direct` through the test fleetd; the feature row exists and the feature
   branch exists in git. Check: it is red with the error of the Context.
2. Fix fault 1 by the design. Give the git gateway of fleetd write access only to the paths that
   the design permits (the refs and objects of the project `.git` that fleetd must write). Do not
   open the whole project folder. Find each other `CliWorktreeGit::new` of the live fleetd that
   has the same fault and say in a progress note which ones write. Check: the test is green.
3. Fault 2: write 1 test that fails today (a planned task of a plan with `own:` paths has its
   rows in `task_files` after the plan commit, and the structural check of a judgment kind reads
   them). Fix `planned_tasks::insert` in the same transaction as the task row. Check: green.
4. Fault 3: write 1 test for each fake (the fake worker writes no `review.md`; the fake judge
   commits `review.md` before `horch result`). Fix the fakes in `crates/horch-e2e/src` so that a
   fake plays only the keys of its role. Check: green, and the other fake tests stay green.
5. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- The git sandbox is a security rule. Do not remove it and do not make it wider than the design
  permits. If the design does not say which paths fleetd may write, send QUESTION with 2 options
  and end your turn.
- 1 door: each state change goes through `transition()`. No schema step.
- `crates/horch/src/cmd/fleetd.rs` is changed by other tasks now (T-890). Change few shared lines,
  merge the run branch each 30 minutes and before finish, and keep both parts in a conflict.
- Each e2e test sets `HORCH_TEST_DOCS_FIXTURE` to an empty local folder, so no test uses the
  network. Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min in long test runs.
- Commit the fix of fault 1 alone and first, and send 1 NOTE with its commit id.

## Done when

- The tests of steps 1, 3 and 4 pass, and the Check passes.
- The done summary names: the paths that the git gateway of fleetd may write and the place in the
  design, each `CliWorktreeGit::new` that you changed, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
