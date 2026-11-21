# CI FIX-10: fix the timing/process failure in horch-core tests/gate_jobs.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test gate_jobs --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/gate_jobs.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Fix the unexplained class `f?` failure of `gate_at_its_limit_is_abandoned_and_its_group_killed` in `crates/horch-core/tests/gate_jobs.rs:958` and prove that it passes 3 times in a row through `scripts/dev-sandbox.sh`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/tests/gate_jobs.rs` (lines 874 to 970).

## Test covered (1 named timing test of FIX-10)

- `gate_at_its_limit_is_abandoned_and_its_group_killed` (`crates/horch-core/tests/gate_jobs.rs:958`, Linux, class `f?`): `the gate group was killed`

## Steps

1. Run `gate_at_its_limit_is_abandoned_and_its_group_killed` 3 times locally through `scripts/dev-sandbox.sh`:
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test gate_jobs -- gate_at_its_limit_is_abandoned_and_its_group_killed --locked || exit 1; done`
2. Inspect `crates/horch-core/tests/gate_jobs.rs:874-970` to find why `the gate group was killed` failed on Linux CI:
   - Check how the test waits for `sleep.pid` and how it checks whether the child `sleep 600` process in the gate's process group is dead after `kill_group` (on Linux, a killed child process can remain a zombie `Z` in `/proc/<pid>/stat` until its parent or subreaper reaps it, so `kill(pid, 0)` / `procid::alive` still returns `true` for a zombie, or `bwrap` without `--die-with-parent` / PID namespace leaves the child or reports zombie state).
3. Fix the cause in `crates/horch-core/tests/gate_jobs.rs` without weakening any assert. A flaky test is a bug.
4. Run the Check (which runs `gate_jobs` 3 times in a row and checks formatting).

## Out of scope

- Any file outside `crates/horch-core/tests/gate_jobs.rs`. (If the root cause is a product fault outside `**Own:**`, stop and send `BLOCKED:` to the orchestrator with the exact `file:line` and evidence.)

## Done when

`gate_at_its_limit_is_abandoned_and_its_group_killed` and the full `gate_jobs` test binary pass 3 times in a row, and the Check passes.

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
