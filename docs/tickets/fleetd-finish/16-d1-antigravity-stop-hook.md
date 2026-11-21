# 16: D1 Antigravity Stop hook: add the Stop hook in antigravity.rs and its test in harness_defaults.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib harness::antigravity && scripts/dev-sandbox.sh cargo test -p horch-core --test harness_defaults && cargo fmt --all --check`
**Own:** `crates/horch-core/src/harness/antigravity.rs`, `crates/horch-core/tests/harness_defaults.rs`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 item L8; section 3 unit D1), `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/phase-4.md` (section "Design gaps"), and `docs/tickets/fleetd-p4/14-stop-hooks.md`.

## Goal

Add the Phase 4 `Stop` hook (`horch result --check`) to the Antigravity adapter in `crates/horch-core/src/harness/antigravity.rs` for a push attempt (and omit it for a claim attempt), and add its adapter tests in `crates/horch-core/tests/harness_defaults.rs`.

## Requirements

- **L8 / FM-6**: A push attempt launched with `HarnessKind::Antigravity` configures a `Stop` hook that runs `<horch> result --check` (matching `STOP_HOOK` in `crates/horch-core/tests/harness_defaults.rs:789`), while a claim attempt (`env.task_dir = None`) has no `Stop` hook.

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

1. Inspect `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/antigravity.rs`, and `crates/horch-core/tests/harness_defaults.rs:780-860` (`claude_push_launch_has_a_stop_hook_that_runs_result_check`, `codex_push_launch_has_a_trusted_stop_hook`, `claim_attempt_has_no_stop_hook`).
   - Check: write `antigravity_push_launch_has_a_stop_hook_that_runs_result_check` and update `claim_attempt_has_no_stop_hook` in `crates/horch-core/tests/harness_defaults.rs` first, and see the test fail.
2. In `crates/horch-core/src/harness/antigravity.rs`, when `env.task_dir.is_some()` (a push attempt), seed or pass the `Stop` hook that runs `<env.bins.horch> result --check` (merging with any existing teammate settings/hooks, as `claude.rs` does in `hooks.Stop`). When `env.task_dir.is_none()` (a claim attempt), add no `Stop` hook.
   - Check: run `cargo test -p horch-core --lib harness::antigravity` and `cargo test -p horch-core --test harness_defaults` with the sandbox env.
3. Run the ticket Check.

## Out of scope

- Changing `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, or `crates/horch-core/src/fleetd/worker_profiles/domain/stop_gate.rs` (unit D3 checks those files of Phase 4 ticket 14).

## Done when

`antigravity.rs` configures the `Stop` hook on a push attempt and omits it on a claim attempt, the tests in `crates/horch-core/tests/harness_defaults.rs` pass, and the Check passes.
