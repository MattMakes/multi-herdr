# R2: end review of phases 2 and 3

**Persona:** primary `codex-sol`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/end-review-p2-p3.md && grep -q '^## Watchlist lines covered' docs/specs/fleetd/reviews/end-review-p2-p3.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/end-review-p2-p3.md`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (wave F).

## Goal

Review the code of fleetd for phases 2 and 3 (leases, the stall ladder, waiting_quota, questions, worktrees, rescue, triage) as an engineer who did not write it. Find each fault that can lose work, break a state machine, or let a worker act as the operator. The orchestrator writes 1 fix ticket for each Critical and Important finding.

## Read first

- `docs/specs/fleetd/design.md`: the sections that your scope names, the table "Where each rule becomes green", and "Failure modes and effects".
- `ai_docs/plans/fleetd-build/for-operator/end-review-watchlist.md` in your worktree: 77 lines. Each line is 1 point that an earlier worker marked for this review.
- `ai_docs/plans/fleetd-build/for-operator/main-findings-from-T177-review.md`.
- The audit of your phases in `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/`: it maps each rule to its code path and test.
- `ai_docs/plans/fleetd-build/decisions.md`: search it for the T-number of a watchlist point to get the ruling.

## Scope

- `crates/horch-core/src/fleetd/task_execution/` (leases, stall, requeue, cancel).
- The worktree reconciler, the rescue job, `worktree_add`, `worktree_remove`, `worktree_list`, the triage_apply job.
- Watchlist points to own for sure: T-376, T-888/T-887 (both lines), T-881, T-469, T-397 (repair_start).

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Read the watchlist. List each line that names code of your scope. You own those lines.
2. For each owned line: read the code, decide if the risk is real, and write a finding or write "checked, no fault" with the file and line that proves it.
3. Then review the scope beyond the watchlist. For each rule of your phases, read the code path and its test. Ask: does the test prove the rule, or only a part? Can the test pass while the rule is broken?
4. Look for these fault classes in each file of the scope:
   - a decision of the pass that reads a file or a client payload, not the ledger;
   - a ledger change and its effect in the wrong order (the row must be first);
   - a state with no exit, or a retry with no bound;
   - a path where 1 bad row fails the whole pass;
   - a timer that does not use the fleet clock;
   - a worker-writable file that fleetd trusts (size, type, owner, link, hash);
   - a request kind whose class or payload a worker can forge;
   - a secret in a log, an env or a file that a worker can read;
   - code under `cfg(target_os)` outside `crates/horch-platform`.
5. Do not run `cargo test`. You can run `cargo check` through `scripts/dev-sandbox.sh` only when a claim needs it.
6. Write the report. Commit it with the subject `R2: the end review of phases 2 and 3 (leases, the stall ladder, waiting_quota, questions, worktrees, rescue, triage)`.

## Report

Write `docs/specs/fleetd/reviews/end-review-p2-p3.md`:
- `## Summary`: counts of Critical, Important and Minor findings, in 5 lines or less.
- `## Findings`: 1 subsection each, with an id `R2-F<n>`, the rank, the design rule id, file:line, what goes wrong with 1 concrete case, and the smallest fix. Rank Critical when work is lost, a worker gets operator rights, or a live fleet of the user can be harmed. Rank Important when a rule of the design is not met or a state can hang. Rank Minor for the rest.
- `## Watchlist lines covered`: 1 row for each owned line: the line number, the T-number, and "finding R2-F<n>" or "checked, no fault: file:line".
- `## Tests that do not prove their rule`: rule id, test name, what is not proven.
- `## Operator decisions`: each point that needs a product or design decision, with your recommendation.
Mark each claim `verified` or `not verified`.

## Out of scope

- Do not fix code. Do not edit a test, a ticket or the design.
- Do not review the 9 task branches that are not yet on the run branch. Review `R2` covers only the code of your worktree.

## Done when

The Check passes, each owned watchlist line has 1 row, and the report is committed.
