# CI FIX-10: fix unexplained failures in horch-e2e tests/p7_docs_sync.rs and tests/p7_release.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test p7_docs_sync --test p7_release --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p7_docs_sync.rs`, `crates/horch-e2e/tests/p7_release.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

After FIX-1 (ticket 100), run `operator_boundary_passes_for_each_adapter` in `crates/horch-e2e/tests/p7_docs_sync.rs` and `next_attempt_of_its_task_runs_in_exec_mode` in `crates/horch-e2e/tests/p7_release.rs` 3 times locally through `scripts/dev-sandbox.sh`, find any remaining cause of failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-e2e/tests/p7_docs_sync.rs` (around line 751) and `crates/horch-e2e/tests/p7_release.rs` (around line 677).

## Tests covered (2 class `f?` rows)

1. `operator_boundary_passes_for_each_adapter` (`crates/horch-e2e/src/harness.rs:963` / `crates/horch-e2e/tests/p7_docs_sync.rs:751`, macOS, class `f?`): `error: test failed, to rerun pass -p horch-e2e --test p7_launcher`
2. `next_attempt_of_its_task_runs_in_exec_mode` (`crates/horch-e2e/src/harness.rs:963` / `crates/horch-e2e/tests/p7_release.rs:677`, Linux, class `f?`): `Running tests/relay_first_delivery.rs (target/debug/build/horch-e2e/b48eacb3e51948ff/out/relay_first_delivery-b48eacb3e51948ff)`

## Steps

1. Build the workspace binaries and run `p7_docs_sync` and `p7_release` 3 times locally through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked`
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test p7_docs_sync --test p7_release --locked || exit 1; done`
2. Note that on CI both tests failed at `crates/horch-e2e/src/harness.rs:963` (`the build of the fakes failed: ... --profile out`, fixed by FIX-1). If either test fails or flakes across 3 runs in the sandbox, find the cause in `crates/horch-e2e/tests/p7_docs_sync.rs` or `crates/horch-e2e/tests/p7_release.rs` and fix it without weakening any assert.
3. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

`operator_boundary_passes_for_each_adapter`, `next_attempt_of_its_task_runs_in_exec_mode`, and the full `p7_docs_sync` and `p7_release` test binaries pass 3 times in a row, and the Check passes.

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


## Added 2026-11-20: more red tests of the local baseline in your Own files

These tests are red in `docs/specs/fleetd/gates/baseline-2026-11.md` and are part of this ticket. The ticket is done only when each test of each Own test file passes. Rule: a test of a behavior that Phase 6 retired by design gets its assert moved to the designed new effect; delete it only when no form of the rule exists, and name the test, the retired behavior and its rule in the commit message. No permanent expected-failures entry.

- `new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea` — `timed out after 20s waiting for an unhealthy row of claude and an idea` (Note: shares root cause with ticket `146` in `crates/horch-e2e/src/exec.rs:221`; block `132` on `146` or run `146` first.)
- `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes` — `timed out after 20s waiting for an unhealthy row of claude and an idea`
- `unhealthy_profile_gets_no_dispatch_and_status_names_the_hold` — `timed out after 20s waiting for an unhealthy row of claude and an idea`
