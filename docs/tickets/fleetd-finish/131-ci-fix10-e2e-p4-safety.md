# CI FIX-10: fix unexplained failure of fr5_worker_never_runs_a_task_search in horch-e2e tests/p4_safety.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test p4_safety --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p4_safety.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

After FIX-1 (ticket 100) and ticket 17 (`17-d2-sr3-external-message.md`), run `fr5_worker_never_runs_a_task_search` and the full `crates/horch-e2e/tests/p4_safety.rs` suite 3 times locally through `scripts/dev-sandbox.sh`, find any remaining cause of failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-e2e/tests/p4_safety.rs` (lines 1 to 260).

## Test covered (1 class `f?` row)

- `fr5_worker_never_runs_a_task_search` (`crates/horch-e2e/src/harness.rs:963` / `crates/horch-e2e/tests/p4_safety.rs:231`, Linux, class `f?`): `error: test failed, to rerun pass -p horch-e2e --test p4_safety`

## Steps

1. Build the workspace binaries and run `fr5_worker_never_runs_a_task_search` 3 times locally through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test p4_safety -- fr5_worker_never_runs_a_task_search --locked || exit 1; done`
2. Note that on CI `fr5_worker_never_runs_a_task_search` failed at `crates/horch-e2e/src/harness.rs:963` (`the build of the fakes failed: ... --profile out`, fixed by FIX-1, and on Linux `socat` was missing, fixed by FIX-2). Check whether `world` in `crates/horch-e2e/tests/p4_safety.rs` has any remaining flake or setup issue across 3 consecutive runs, and fix it in `crates/horch-e2e/tests/p4_safety.rs` if found. Never weaken an assert.
3. Run the Check.

## Out of scope

- Any file outside `crates/horch-e2e/tests/p4_safety.rs`.

## Done when

`fr5_worker_never_runs_a_task_search` and all tests in `crates/horch-e2e/tests/p4_safety.rs` pass 3 times in a row, and the Check passes.

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
