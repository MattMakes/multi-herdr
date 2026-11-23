# 17: D2 SR-3 external-message boundary: test and enforce external messaging denial from a verifier and from a worker

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test sr3_no_agent_push && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p4_safety && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs`, `crates/horch-core/tests/sr3_no_agent_push.rs`, `crates/horch-e2e/tests/p4_safety.rs`, `crates/horch-platform/src/sandbox.rs`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 item L9; section 3 unit D2), `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/phase-4.md` (lines 19, 138, 149), and `docs/tickets/fleetd-p4/22-agents-never-push.md`.

## Goal

Prove SR-3 ("No agent shall push to a protected branch, deploy, or message anyone outside the fleet") for external messaging commands (such as `gh issue comment`, `gh pr comment`, and `curl` to a webhook) from both a verifier check sandbox and a worker profile, and fix the boundary in `launch_env.rs` / `sandbox.rs` if the test is red.

## Requirements

- **L9 / SR-3**:
  1. Write tests in `crates/horch-core/tests/sr3_no_agent_push.rs` and `crates/horch-e2e/tests/p4_safety.rs` that test external-message commands (`gh issue comment`, `gh pr comment`, `gh discussion`, and an outbound `curl` / webhook call) from a worker profile and from a verifier check sandbox (`crates/horch-platform/src/sandbox.rs`).
  2. If `DENIED_COMMANDS` in `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs` or the verifier sandbox in `crates/horch-platform/src/sandbox.rs` permits an external message command, extend the deny list and sandbox rules so that external messaging is refused by default.

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

1. Inspect `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs` (`DENIED_COMMANDS`), `crates/horch-platform/src/sandbox.rs` (`Network`), `crates/horch-core/tests/sr3_no_agent_push.rs`, and `crates/horch-e2e/tests/p4_safety.rs`.
   - Check: write tests in `crates/horch-core/tests/sr3_no_agent_push.rs` and `crates/horch-e2e/tests/p4_safety.rs` that assert worker profile denial and verifier check-sandbox refusal for external messaging (`gh issue comment`, `gh pr comment`, and `curl` to a local test HTTP listener acting as a webhook).
2. Run the new tests in the sandbox env. If any test is red, update `DENIED_COMMANDS` in `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs` (and `crates/horch-platform/src/sandbox.rs` if needed) so that worker profiles deny external messaging commands (`gh issue comment`, `gh pr comment`, `gh pr review`) and the verifier check sandbox blocks outbound webhook messaging.
   - Check: `cargo test -p horch-core --test sr3_no_agent_push` passes in the sandbox env.
3. Build the workspace binaries and run `crates/horch-e2e/tests/p4_safety.rs` in the sandbox env:
   - Check: `cargo test -p horch-e2e --test p4_safety` passes.
4. Run the ticket Check.

## Out of scope

- Modifying `crates/horch-core/src/harness/claude.rs` or `crates/horch-core/src/harness/codex.rs` (owned by unit D3).

## Done when

Tests in `sr3_no_agent_push.rs` and `p4_safety.rs` prove that external messaging from a worker and from a verifier is refused, and the Check passes.
