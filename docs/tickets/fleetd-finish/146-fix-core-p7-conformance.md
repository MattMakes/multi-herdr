<!-- horch-ticket: 1 -->
# Fix the red tests of `crates/horch-core/tests/p7_conformance.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test p7_conformance --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_dispatch --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/p7_conformance.rs, crates/horch-e2e/src/exec.rs`

## Goal

Make the 2 red tests of `crates/horch-core/tests/p7_conformance.rs` from `docs/specs/fleetd/gates/baseline-2026-11.md` pass without breaking `p0_dispatch` or weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-core`, `tests/p7_conformance.rs`).
- `crates/horch-core/tests/p7_conformance.rs` (lines 380 to 495).
- `crates/horch-e2e/src/exec.rs` (lines 195 to 235) and `git show 3737f0b855`.

## Failing tests and their first error line from the baseline

1. `hooks_fire_fails_when_no_stop_reaches_the_wrapper` — `the check passed`
2. `shared_write_fails_when_the_agent_ends_without_the_write` — `the check passed`

## Cause and what to do

- In `crates/horch-core/tests/p7_conformance.rs`, `shared_write_fails_when_the_agent_ends_without_the_write` (line 390) and `hooks_fire_fails_when_no_stop_reaches_the_wrapper` (line 453) call `m.check(&claude(), "exit_without_result", Check::SharedWrite)` and `m.check(&claude(), "exit_without_result", Check::HooksFire)`, which sets `HORCH_FAKE_SCENARIO="exec,conformance,exit_without_result"` and expects the fake CLI to exit 0 after printing progress lines without writing `outbox/result.json` or firing `Stop`.
- Commit `3737f0b855` (`fleetd-build 54`) changed line 221 of `crates/horch-e2e/src/exec.rs` from:
  `if scenario_has("exit_without_result") || (obeys_stop_gate && conformance)`
  to:
  `if (scenario_has("exit_without_result") && !check) || (obeys_stop_gate && conformance)`
  so that a fleet-wide `HORCH_FAKE_SCENARIO="exec,commit,exit_without_result"` in `p0_dispatch` (`fm6`) would still pass the automatic background conformance run (where `check` is `true` from the prompt header, and `"conformance"` is not in `HORCH_FAKE_SCENARIO`).
- However, gating `scenario_has("exit_without_result")` on `&& !check` also suppressed `exit_without_result` when a test explicitly tests a failing conformance check (such as `m.check(..., "exit_without_result", ...)` in `p7_conformance.rs`, where `scenario_has("conformance")` is true, or in `p7_docs_sync.rs` where a fake CLI wrapper appends `,exit_without_result`).
- Fix `crates/horch-e2e/src/exec.rs` (and `crates/horch-core/tests/p7_conformance.rs` if needed) so that:
  1. When `exit_without_result` is requested for a conformance check under test (`p7_conformance.rs`), the fake exits 0 before writing the result, and `hooks_fire_fails_when_no_stop_reaches_the_wrapper` and `shared_write_fails_when_the_agent_ends_without_the_write` both fail the conformance check as expected.
  2. `p0_dispatch` (`fm6`, where `HORCH_FAKE_SCENARIO="exec,commit,exit_without_result"` has `scenario_has("commit")`) still gets a passing background conformance check.
- **Other cause classes:**
  - The retired command `horch ledger run new` or `horch ledger task`: migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
  - `reading the operator token ... No such file`: use `h.serve_fleetd()`.
  - A behavior that Phase 6 removed on purpose: do not delete the test on your own decision; send `QUESTION:` to the orchestrator with the design rule.
  - A time-out or a result that changes between runs: run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert.

## Done when

Both red tests of `p7_conformance.rs` and all tests of `p0_dispatch.rs` pass 3 times in a row, and the Check passes.

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
