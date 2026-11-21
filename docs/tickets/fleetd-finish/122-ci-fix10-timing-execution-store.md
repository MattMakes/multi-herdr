# CI FIX-10: fix the concurrent dirlock failure in horch-core tests/execution_store.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test execution_store --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/store.rs`, `crates/horch-core/tests/execution_store.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Fix the unexplained class `f?` failure of `store_dirlock_excludes_concurrent_writers` in `crates/horch-core/tests/execution_store.rs:343` and prove that it passes 3 times in a row through `scripts/dev-sandbox.sh`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/tests/execution_store.rs` (lines 322 to 355) and `crates/horch-core/src/execution/store.rs` (dirlock acquisition and timeout/backoff).

## Test covered (1 named timing test of FIX-10)

- `store_dirlock_excludes_concurrent_writers` (`crates/horch-core/tests/execution_store.rs:343`, Linux, class `f?`): `Err: Any { .. }`

## Steps

1. Run `store_dirlock_excludes_concurrent_writers` 3 times locally through `scripts/dev-sandbox.sh`:
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test execution_store -- store_dirlock_excludes_concurrent_writers --locked || exit 1; done`
2. Inspect `crates/horch-core/tests/execution_store.rs:322-355` and `crates/horch-core/src/execution/store.rs` to find why one of the 2 threads panicked during 50 concurrent `store.insert(...)` iterations on Linux CI (for example dirlock spin timeout, stale lock check, or atomic rename/fsync contention under parallel test load).
3. Fix the cause in `crates/horch-core/tests/execution_store.rs` or `crates/horch-core/src/execution/store.rs`. Never weaken an assert. A flaky test is a bug.
4. Run the Check (which runs `execution_store` 3 times in a row and checks formatting).

## Out of scope

- Any file outside `**Own:**`.

## Done when

`store_dirlock_excludes_concurrent_writers` and the full `execution_store` test binary pass 3 times in a row, and the Check passes.

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
