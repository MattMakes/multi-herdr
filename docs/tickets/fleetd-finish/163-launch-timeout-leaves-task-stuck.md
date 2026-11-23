# Lost work: an attempt that times out in launching leaves its task running with no lease and no attempt

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test fleetd_pass_p2 && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p6_questions && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/tests/fleetd_pass_p2.rs`, `crates/horch-e2e/tests/p6_questions.rs`

## Goal

Task T-12 found this in the test `fm21_one_replan_then_escalation`. In `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (about line 1584), `LaunchStep::TimedOut` reaps the attempt with `task: None`. The task stays `running`. Then rule I1 deletes the lease of the ended attempt. No pass rule moves a running task that has no lease and no attempt, so the task never gets a new attempt, and only the stuck item `stuck:F-...` opens. This is lost work (FM-21, and the rule that each ended attempt gives its task a next state).

## Steps

1. Write the failing test first in `crates/horch-core/tests/fleetd_pass_p2.rs`: a task attempt in `launching` passes the start time limit on the fake clock; after the next passes the task has a new attempt (or is queued again with its retry count), and no task is `running` with no live attempt. Name: `a_launch_timeout_of_a_task_attempt_gives_the_task_a_retry`.
2. Fix: the TimedOut reap of a task attempt gives the task the same result as a dead attempt (`AfterFailure::Retry`, as `dead_attempts` does), with the limits of the design (retry count, fallback profile, escalation). Read the design for the launch failure of an attempt (FM-13, FM-21) before you choose.
3. Add an invariant test: after each pass of the suite of step 1, no task is `running` without a live attempt or a lease. If the pass has an invariants list (I1...), add the rule there and say so.
4. The test file `p6_questions.rs`: task T-12 made `World::drive` move the clock 60 s per pass to avoid the race. Add 1 e2e test that makes the launch timeout on purpose and expects the retry.

## Out of scope

The launcher (ticket 157) and the stand-in start (ticket 156).

## Done when

The new tests pass and the Check passes.

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
