# The launcher fails with "the start time of process N is unreadable" on a loaded host

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked launcher && scripts/dev-sandbox.sh cargo test -p horch-core --test w3_finish_steps --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`

## Goal

The land step of task T-37 failed 1 time in `w3_finish_steps::a_land_with_a_remote_names_the_push` (`crates/horch-core/tests/w3_finish_steps.rs`) with `the launch failed: the start time of process 83409 is unreadable`, from `Launcher::launch` in `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`. The load average was 26. The test passed 3 of 3 runs alone. A result that changes between runs is a bug. Find the cause and fix it.

## Read first

- `Launcher::launch` and each place that reads the start time of a process it started.
- The platform function that reads a start time (`crates/horch-platform/src/`; do not edit it without a QUESTION to the orchestrator).
- `git show ffa44584` (task T-4): the same class of fault in `fleet_process` (a process fact read fails when the process ends or is not yet visible).
- `docs/specs/fleetd/design.md`: search "start time" for the rule (a pid is trusted only with its start time).

## Steps

1. Find when the read can fail for a process that the launcher just started: (a) the child ended before the read (a short job); (b) the child is not yet visible to the read; (c) another cause. Prove it with a deterministic test (a child that exits at once; an injected failed read), not with host load. Never start busy loops or extra builds to make load.
2. Fix by the cause. For (a): the launcher holds the child handle, so an ended child is a known fact; decide by the design what the launch result is for a child that already ended (the launch did happen; its exit is ingested as normal), and do not fail the launch. For (b): read the start time in a way that cannot race (for example before the child can exit: the child waits on a pipe until the parent recorded the start time). Do not add a sleep or a retry count without a reason from the design.
3. The security rule stays: no pid is recorded without its start time.
4. Run the new test and `w3_finish_steps` 10 times.

## Out of scope

Other tests of `w3_finish_steps.rs`.

## Done when

The deterministic test passes, 10 of 10 runs pass, the cause is in the commit message, and the Check passes.

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

