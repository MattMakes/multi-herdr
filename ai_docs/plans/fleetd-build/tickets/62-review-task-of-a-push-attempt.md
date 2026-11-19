# A code-sweep review task that fleetd launches by push runs in the worktree of the reviewed task, gives its findings, and ends done

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --lib taskledger && cargo test -p horch-e2e --test p4_review_push && cargo test -p horch-e2e --test taskledger the_whole_flow_on_the_fake_herdr && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p4_review_push.rs`

## Goal

When a code task with `Review: light` or `full` is in `verifying`, each code-sweep review task of
it runs as a push attempt to its end: the reviewer reads the change of the reviewed task, its
findings reach the review step of the reviewed task, the review task ends `done`, and the
reviewed task lands or gets its fix round, as the review scripts of today do.

## Context

Found by T-893 (opus-hardening-194) with the e2e test `the_whole_flow_on_the_fake_herdr`. The
facts on the run branch:

- T-2 waits in `verifying` for its 3 code-sweep review tasks.
- fleetd launches each review task by push, with no worktree, in the project checkout on the
  feature branch.
- The agent of the review task runs `horch result`. It exits 1: 'the task ledger has no worktree
  of <review task>; give --commit'. The attempt ends `no_result` and is launched again for ever.
- If a review task gets a result, it moves to `verifying`, and the verifier runs a `task_check`
  job and the review steps for the review task itself. No code path ends a pushed review task
  `done`.
- The fake agent commits on the feature branch in the project checkout, because it plays
  'commit' for a review task.

The design (`docs/specs/fleetd/design.md`, 'Done for each kind of task', code kind): 'The task's
check runs. Then today's review scripts run unchanged: the code-sweep rounds, the findings F<n>,
the answers file, the waivers and the verification profiles. Their limits stay: at most 2
requeues, and review rounds of 1 (light) and 3 (full)' (row V-14 of
`docs/specs/fleetd/preservation-05-contracts-and-values.md`).

Read first: `horch ledger show T-893`, `T-890`, `T-892`, `T-402`, `T-871`; the review code of
today (`crates/horch-core/src/taskledger/finish/codesweep.rs` and the files beside it); the
result command (`resultcmd.rs`); the verifier and the push dispatch in
`crates/horch-core/src/fleetd/task_execution/`; the preservation rows about code-sweep.

## Steps

1. Write down, in 1 NOTE to the orchestrator, how a review task works today from spawn to its
   end (where it runs, what it can write, how its findings and its verdict reach the step, what
   ends it), and the smallest change that gives the same contract for a push attempt. Name each
   point where the design does not decide. Then go on; do not wait for an answer.
2. Write the red tests first in the new file `crates/horch-e2e/tests/p4_review_push.rs`, through
   the binary with a served test fleetd: (a) a code task with `Review: light` and a clean review
   lands, and its review task is `done` with 1 attempt; (b) a review with 1 finding gives the
   reviewed task its fix round, and the second round lands it; (c) the review attempt makes no
   commit and changes no file of the feature branch or of the project checkout; (d) a review
   task gets no `task_check` job and no review task of its own.
3. Make the product do it. Expected parts: the launch gives the review attempt the worktree (or
   a read-only view) of the reviewed task and not the project checkout; `horch result` of a
   review attempt needs no commit and no worktree of its own; the verifier ends a review task
   by its findings file only; the review step of the reviewed task reads the result. Keep the
   review scripts and their limits unchanged (V-14).
4. Make the fake agent play 'review' (no commit) for a review task.
5. Make `the_whole_flow_on_the_fake_herdr` green and take its line off the expected-failures
   list. If it is still red for another cause, name the cause in a QUESTION with 2 options.
6. Find the cause of this extra fault of T-893: `horch ledger status --json` writes to a
   read-only database. A read command must not write. Fix it if the fix is small (1 test); else
   name the cause in the done summary.
7. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit. No schema
  step; if you need one, send QUESTION with 2 options and end your turn.
- A reviewer is a fleet process: it cannot approve, land or write the ledger (FR-10, T-898). Do
  not add an exception to the authorize rule or to the arch rule, and do not make an assert
  weaker.
- Shared files (`pass.rs`, `dispatch.rs`, `resultcmd.rs`, the verifier): small functions, few
  shared lines, merge the run branch each 30 minutes and before finish.
- An e2e test serves a test fleetd (`h.serve_fleetd()`, `pass_period_s = 1`), makes its
  workspace with `h.fleet_workspace()`, puts the ledger and the fleet root outside the state
  folder (`HORCH_LEDGER_DB`, `FLEET_ROOT`), writes `[gates]` fast and full in the project
  fleet.toml, and sets `HORCH_TEST_DOCS_FIXTURE` to an empty local folder. Each test command
  has a time limit. Run only your own test files. Send `horch ledger progress` each 5 min.
- Windows code stays small. Name each Windows risk in the done summary.

## Done when

- The 4 tests of step 2 and `the_whole_flow_on_the_fake_herdr` pass, and the Check passes.
- The done summary names: the contract of step 1, each point that the design does not decide
  and your choice, the answer of step 6, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
