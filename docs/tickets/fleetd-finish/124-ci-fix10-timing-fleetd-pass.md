# CI FIX-10: fix the launching start-timeout failure in horch-core tests/fleetd_pass.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/fleetd_pass.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Fix the unexplained class `f?` failure of `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out` in `crates/horch-core/tests/fleetd_pass.rs:613` and prove that it passes 3 times in a row through `scripts/dev-sandbox.sh`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/tests/fleetd_pass.rs` (lines 598 to 645).

## Test covered (1 named timing test of FIX-10)

- `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out` (`crates/horch-core/tests/fleetd_pass.rs:613`, macOS, class `f?`): `error: test failed, to rerun pass -p horch-core --test fleetd_pass`

## Steps

1. Run `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out` 3 times locally through `scripts/dev-sandbox.sh`:
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass -- launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out --locked || exit 1; done`
2. Inspect `crates/horch-core/tests/fleetd_pass.rs:598-645`:
   - In `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out`, `Fake::then(&w, "exit 0")` launches a short-lived process, `run(&mut pass, &mut w)` records `record.launched_at`, and `wait_for(|| pass_reaps(&mut pass, &mut w))` runs passes until the child exits and is reaped. Check how `w.launching_attempt()` sets `attempts.started_at` vs `record.launched_at` and how `pass_reaps` advances or reads the clock before `w.set_clock(record.launched_at.0 + 80)` at line 611 (causing the attempt at line 613 to already transition or fail the `assert!(wait_for(...))` under load on macOS CI).
3. Fix the cause in `crates/horch-core/tests/fleetd_pass.rs` without weakening any assert. A flaky test is a bug.
4. Run the Check (which runs `fleetd_pass` 3 times in a row and checks formatting).

## Out of scope

- Any file outside `crates/horch-core/tests/fleetd_pass.rs`.

## Done when

`launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out` and the full `fleetd_pass` test binary pass 3 times in a row, and the Check passes.

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
