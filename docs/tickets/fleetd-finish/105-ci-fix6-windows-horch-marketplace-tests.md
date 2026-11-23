# CI FIX-6: gate the unix symlink test in horch-marketplace for Windows test build

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-marketplace --test marketplace --locked && cargo fmt --all --check`
**Own:** `crates/horch-marketplace/tests/marketplace.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Windows` and `## Fix units` row FIX-6).

## Goal

On Windows CI (`test-windows`), `crates/horch-marketplace/tests/marketplace.rs` fails to compile with 2 errors at lines 445 and 459 (`std::os::unix::fs::symlink`). Gate `mkt_06_rejects_symlink` with `#[cfg(unix)]` so the `marketplace` test target compiles on Windows.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Windows` and `## Fix units` row FIX-6.
- `crates/horch-marketplace/tests/marketplace.rs`: lines 441 to 470 (`mkt_06_rejects_symlink`).

## Compile errors covered (2 errors in `horch-marketplace` test `marketplace`)

- `mkt_06_rejects_symlink` (`crates/horch-marketplace/tests/marketplace.rs:445` and `:459`, class `c`): ``error[E0433]: cannot find `unix` in `os` `` (`std::os::unix::fs::symlink`).

## Steps

1. In `crates/horch-marketplace/tests/marketplace.rs:441`, add `#[cfg(unix)]` above `#[test] fn mkt_06_rejects_symlink()`.
2. Check the rest of `crates/horch-marketplace/tests/marketplace.rs` for any other `std::os::unix` reference.
3. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch-marketplace --test marketplace --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `crates/horch-marketplace/tests/marketplace.rs`.

## Done when

`mkt_06_rejects_symlink` is gated with `#[cfg(unix)]` and the Check passes.

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
