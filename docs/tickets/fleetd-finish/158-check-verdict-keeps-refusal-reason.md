# The verdict of a failed task check does not show why the sandbox refused (the check stderr is dropped)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test check_sandbox --test verifier_review --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p4_verifier --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/tests/check_sandbox.rs`, `crates/horch-e2e/tests/p4_verifier.rs`

## Goal

Task T-17 gave the check sandbox the rule `Network::Local`: a task check has no off-host network (SR-3). A check that needs an off-host connection fails. Task T-52 found: the verdict of the failed check holds no text of the network rule, because the `task_check` job drops the stderr of the check (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, about line 1130). The operator sees a failed check and cannot see that the sandbox refused a connection. The same is true for a write outside the paths (SR-4).

## Steps

1. Write the failing tests first. In `crates/horch-e2e/tests/p4_verifier.rs`: make the assert of `sr3_check_that_connects_off_host_is_refused` stronger: the verdict (the text that the operator and the worker of the next attempt get) names SR-3 and the network rule. Add the same assert for `sr4_check_that_writes_outside_its_paths_is_refused` if its verdict has no reason now.
2. Fix: the `task_check` job keeps the end of the check output (stdout and stderr) in its result, with a size limit (use the limit that the design gives for a job result; if none, 8 KiB of the tail). When the sandbox denied an act, the verdict has 1 line of horch that names the rule (SR-3 network, SR-4 write); find how the sandbox reports a denial on macOS and Linux (`crates/horch-platform/src/sandbox.rs`; do not edit it without a QUESTION to the orchestrator).
3. The check output is untrusted text of a worker (SR-6). Each place that shows it escapes control characters and cannot be read as a line of horch. Name the function that does this in the commit message.

## Out of scope

The sandbox rules themselves.

## Done when

The stronger asserts pass and the Check passes.

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

