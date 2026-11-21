# 03: B1 baseline: execute fmt, clippy, and full workspace tests in the sandbox and record baseline-2026-11.md

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh test -f docs/specs/fleetd/gates/baseline-2026-11.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/baseline-2026-11.md`
**Do not touch:** `crates/`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 3 unit B1; section 4).

## Goal

Run the first full build and test execution of `fleet/fleetd-build` inside `scripts/dev-sandbox.sh`, and commit `docs/specs/fleetd/gates/baseline-2026-11.md` with every failing test, whether it is on the expected-failures list, the wall time of each command, and the peak size of `target/`.

## Requirements

- Execute `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked`, and `scripts/dev-sandbox.sh cargo test --workspace --locked` on `fleet/fleetd-build`.
- Record every failing test name, its crate and test binary, whether it is listed in `crates/horch-e2e/expected-failures/*.txt`, its wall time, and the peak disk usage of `target/`.
- Do not fix any code or test in this unit.

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

1. Run `cargo fmt --all --check` in the worktree and record its exit code, output, and wall time.
   - Check: record the result even if it exits non-zero.
2. Run `scripts/dev-sandbox.sh cargo build --workspace --bins --locked` and then `cargo clippy --workspace --all-targets --locked`. Record the exit code, any warnings or errors, and the wall time.
   - Check: `target/debug/horch` exists in the worktree.
3. Run `scripts/dev-sandbox.sh cargo test --workspace --locked --no-fail-fast` and capture the full output and wall time. Measure `du -sh target` at the end of the test run.
   - Check: every test binary has run to completion.
4. Compare each failing test against `crates/horch-e2e/expected-failures/*.txt`. Write `docs/specs/fleetd/gates/baseline-2026-11.md` with:
   - the commit SHA of `fleet/fleetd-build` tested;
   - a summary table of the 3 commands (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked`, `scripts/dev-sandbox.sh cargo test --workspace --locked`), their exit codes, and their wall times;
   - the peak size of `target/` from `du -sh target`;
   - a table of all failing tests with columns `Test`, `Binary`, `In expected-failures`, `Expected-failures file:line`, and `Failure summary`;
   - a table of any expected-failure entry whose test passed.
5. Run the ticket Check.

## Out of scope

- Fixing any formatting, clippy, unit test, or e2e test failure.

## Done when

`docs/specs/fleetd/gates/baseline-2026-11.md` is committed with the complete results, wall times, and `target/` peak size, and the Check passes.
