# FR-10: an agent can approve an idea while its own attempt is launching and has no recorded pid

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test peer_process --test p6_authorize --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p6_fr10_approval --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, `crates/horch/src/cmd/idea/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`, `crates/horch-core/tests/peer_process.rs`, `crates/horch-core/tests/p6_authorize.rs`, `crates/horch-e2e/tests/p6_fr10_approval.rs`, `crates/horch-e2e/src/fr10_agent.rs`

## Goal

Rule FR-10: no process of a fleet attempt can approve an idea. The function `fleet_process` decides who is a fleet process from the recorded pids and start times of the attempts (`fleet_processes` in `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, about line 96; callers `crates/horch/src/cmd/idea/mod.rs` about line 135 and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` about line 411). Task T-160 found a hole: the fake idea-lead of I-0005 ran `horch idea approve I-0001` and fleetd applied it, while the attempt I-0005/clarifying/1 was in `launching` with pid NULL. An agent that runs before its process is recorded is not seen as a fleet process. Close the hole.

## Read first

- `docs/specs/fleetd/design.md`: FR-10, SR-8, FM-32, and the attempt states (`launching`, `live`).
- `git show ffa44584` and `git show dfc44bb4` (task T-4): `fleet_process`, `Fact::Yes`, `Fact::Unknown`, and how the note writer waits for the wrapper of a launching attempt.
- Ticket `152-launcher-start-time-unreadable.md` (task T-157): the launcher and the moment it records the pid and the start time.
- The test of T-160 in `crates/horch-e2e/tests/p6_fr10_approval.rs` (if landed): its fake waits until its attempt is live. Your test must not wait.

## Steps

1. Write the failing e2e test first: `fr10_e_agent_of_a_launching_attempt_with_no_recorded_process_cannot_approve`. The fake agent tries the approval at its first instruction, with no wait for `live`. Make the window deterministic (for example a fake launcher delay before the pid is recorded, or a test hook that holds the record step); never use host load. Write the same case as a unit test with fixed facts in `crates/horch-core/tests/peer_process.rs`.
2. Choose the fix by the design and write the reason in the commit message:
   - (a) preferred if the code allows it: no agent code runs before the wrapper pid and start time are recorded (the launch records the process before the child can execute the agent, for example the child waits on a pipe). Then the window does not exist. This needs `launcher.rs`, which is not in your Own: send `QUESTION:` to the orchestrator before you edit it.
   - (b) fail closed: while 1 attempt is in `launching` with no recorded process, `fleet_process` gives `Unknown` for a caller that it cannot prove to be outside the fleet, and the approval is refused with a reason that tells the operator to try again in some seconds. State the cost: the operator cannot approve during a launch window. Keep the window short and bounded (name the bound).
   - Both can be needed: (b) as the guard, (a) to make the window small.
3. The refusal must hold for the CLI path and for the command channel path (`check_peer` in `authorize.rs`).
4. Run `p6_fr10_approval` 10 times.

## Out of scope

Other request kinds than idea approval, unless the same guard function serves them (then say so in the commit message).

## Done when

The new e2e test and the unit test pass, 10 of 10 runs pass, and the Check passes.

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

