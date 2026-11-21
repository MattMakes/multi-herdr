# SR-8: no agent code runs before fleetd has recorded the process of its attempt

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked launcher && scripts/dev-sandbox.sh cargo test -p horch-core --test peer_process --test p6_authorize --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p6_fr10_approval --test p5_operator_boundary --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`, `crates/horch-core/tests/peer_process.rs`, `crates/horch-core/tests/p6_authorize.rs`, `crates/horch-e2e/tests/p6_fr10_approval.rs`, `crates/horch-e2e/tests/p5_operator_boundary.rs`

## Goal

The rule `fleet_process` decides who is a fleet process from the recorded pids and start times of the attempts. An attempt in `launching` has no recorded process for some time, and the agent can run in that time. Task T-163 (ticket 154) closed this window for the idea approval only (the approval is refused while an attempt is launching with no recorded process). Each other request of class operator stays open to an agent in the window, because the launch helper of fleetd (`horch spawn --attempt`) itself sends operator kinds (`session_records`, `worker_launch_failed`) on the channel before its process is recorded, so a blanket refusal blocks each launch. Close the window for all kinds (SR-8, FM-32).

## Read first

- The commits of task T-163 on the run branch (`git log --grep T-163`): the launch-window flag, the test hook `hold-launching`, and the commit message with the evidence.
- The commit of task T-157 (launcher start time) and `Launcher::launch`.
- `docs/specs/fleetd/design.md`: FR-10, SR-8, FM-32, the attempt states, and the request classes.
- Ticket `156-standin-fleetd-inherits-caller-env.md` (other task; do not do its work).

## Steps

1. List each request kind of class operator, and for each one say who sends it during a launch (the launch helper, the wrapper, the agent) and if a refusal in the launch window breaks the launch. Put the table in the commit message.
2. Write the failing tests first: an agent of an attempt in `launching` with no recorded process sends each operator kind of step 1 that an agent must never send (raw request on the channel, and by request file); each one must be refused and recorded. Use the test hook `hold-launching` of T-163.
3. Fix so that the window does not exist or does not matter. Preferred: the process of the attempt is recorded (pid and start time, in the ledger) before the child can execute agent code: the launcher starts the child stopped at a gate (for example it waits on a pipe), fleetd records the process, then opens the gate. If the launch helper must send requests before that, give the helper its own proof that does not depend on the window (for example a one-time launch token that fleetd made for this attempt and passes only to the helper, checked in `authorize.rs`), so that "launch helper" and "agent" are different callers.
4. Then remove the special case of T-163 if the new rule covers it, or keep it and say why.
5. Run the 2 e2e test files 10 times before finish.

## Out of scope

The stand-in start (ticket 156).

## Done when

The tests of step 2 pass, the launch of each harness still works (the Check), and the commit message has the table of step 1.

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

