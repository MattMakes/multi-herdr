# The boot conformance job of a test fleetd fails on a loaded host, and the FR-10 test then approves the wrong idea

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p6_fr10_approval --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p6_fr10_approval.rs`, `crates/horch-e2e/src/fr10_agent.rs`

## Goal

The land step of task T-6 failed in `fr10_a_agent_of_a_live_attempt_cannot_approve` (`crates/horch-e2e/tests/p6_fr10_approval.rs`) at a load average of 21. The worker read the log: the boot conformance job of the test fleetd failed and added the system idea I-0003 (`adapter-conformance-failed`); the launch of I-0001 failed 2 times (`worker_launch_failed`); I-0003 reached `awaiting_approval` first; the fake approved I-0003, and the test expected I-0001. A result that changes between runs is a bug. There are 2 faults: a cause (the job and the launch fail) and a weak test (it takes the first idea in `awaiting_approval`).

## Read first

- The evidence of the failed land (70 lines of the log, with the panic context): `ai_docs/plans/fleetd-finish/evidence/T6-4-land-fr10a-flake.log`.
- `crates/horch-e2e/tests/p6_fr10_approval.rs`, `crates/horch-e2e/src/fr10_agent.rs`, and how the harness starts fleetd (`crates/horch-e2e/src/harness.rs`: boot jobs `conformance` and `docs_sync`).
- Ticket `152-launcher-start-time-unreadable.md` (task T-157): `Launcher::launch` can fail with "the start time of process N is unreadable" on a loaded host. This can be the cause of `worker_launch_failed` here. Do not fix the launcher; read the failure reason and say in the commit message if it is the same.

## Steps

1. Find the reason of the failed boot conformance job and of each `worker_launch_failed` in the log (the reason text of the job row and of the attempt). If the log is gone, make the failure deterministic: a fake CLI whose conformance check fails, so that the system idea exists before the test idea is approved. Never start busy loops or extra builds to make load.
2. Fix the test: the fake and the test select the idea under test by its id, not by "the first idea in `awaiting_approval`". A system idea beside it must not change the result. Add 1 test: `a_system_idea_in_awaiting_approval_does_not_change_which_idea_the_agent_tries_to_approve`.
3. If the reason of step 1 is a product fault that is not the launcher fault of ticket 152 (for example a time limit of the conformance job that a slow host passes), do not edit product code: send `QUESTION:` to the orchestrator with the reason text, the file and line, and the fix you want.
4. Run the test file 10 times.

## Out of scope

`fleet_process`. The launcher (ticket 152). The harness teardown (done in T-114).

## Done when

The new test passes, 10 of 10 runs pass, the commit message names the reason of step 1, and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

