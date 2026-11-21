# CI FIX-8: migrate stale unit tests in horch cmd/recipes.rs, cmd/rescuecmd.rs and cmd/sessionscmd.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::recipes::tests --locked && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::rescuecmd::tests --locked && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::sessionscmd::tests --locked && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/rescuecmd.rs`, `crates/horch/src/cmd/sessionscmd.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (row 9).

## Goal

Fix the 4 class `f` unit tests in `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/rescuecmd.rs`, and `crates/horch/src/cmd/sessionscmd.rs` that fail because Phase 6 retired `horch ledger task`, required an idea in flight (`FR-12`), and routed rescue commands through the operator token check.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch/src/cmd/recipes.rs` (around line 1152), `crates/horch/src/cmd/rescuecmd.rs` (around lines 290 and 329), and `crates/horch/src/cmd/sessionscmd.rs` (around line 438).

## Tests covered (4 class `f` rows)

1. `crates/horch/src/cmd/recipes.rs` (1 test, also in `red-baseline.txt`):
   - `cmd::recipes::tests::trf_43_the_steward_task_is_queued_and_its_spawn_has_no_task_text` (`crates/horch/src/cmd/recipes.rs:1152`):
     - CI first error line: `Err: horch ledger task is retired. Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <feature>]`
     - `red-baseline.txt` first error line: `called Result::unwrap() on an Err value: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
2. `crates/horch/src/cmd/rescuecmd.rs` (2 tests):
   - `cmd::rescuecmd::tests::drop_deletes_the_ref_and_verifies_it` (`crates/horch/src/cmd/rescuecmd.rs:290`): `Err: reading the operator token`
   - `cmd::rescuecmd::tests::worker_caller_is_refused` (`crates/horch/src/cmd/rescuecmd.rs:329`): `Err: reading the operator token`
3. `crates/horch/src/cmd/sessionscmd.rs` (1 test):
   - `cmd::sessionscmd::tests::trf_44_sessions_shows_a_ledger_worker_with_its_claimed_task` (`crates/horch/src/cmd/sessionscmd.rs:438`): `Err: run r has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`

## Steps

1. In `crates/horch/src/cmd/recipes.rs:1152` (`trf_43_the_steward_task_is_queued_and_its_spawn_has_no_task_text`), check whether the worktree-steward recipe path under test was retired or moved in Phase 6 (`horch ledger task is retired`). If the test checks a behavior that continues through `Store` / `idea add --direct`, update the fixture to create the ledger and queue the steward task through the Phase 6 path; if the test proves a behavior that Phase 6 removed on purpose, delete the test and write the reason and design rule (`FR-12` / Phase 6 retirement of `horch ledger task`) in the commit message. Never weaken an assert.
2. In `crates/horch/src/cmd/rescuecmd.rs:290` (`drop_deletes_the_ref_and_verifies_it`) and `:329` (`worker_caller_is_refused`), seed the test environment with a valid `0o600` operator token file at `<fleet_root>/run/operator.token` (and request spool or test channel server as used by other `cmd` unit tests) so the operator token check succeeds before testing the rescue drop and worker-caller refusal.
3. In `crates/horch/src/cmd/sessionscmd.rs:438` (`trf_44_sessions_shows_a_ledger_worker_with_its_claimed_task`), call `horch_core::taskledger::schema::run_idea(tx, &run.run_id, &horch_core::taskledger::util::now(), true)?` when seeding run `r` in the test fixture.
4. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::recipes::tests --locked && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::rescuecmd::tests --locked && scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::sessionscmd::tests --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

The unit tests in `cmd::recipes::tests`, `cmd::rescuecmd::tests`, and `cmd::sessionscmd::tests` pass, and the Check passes.

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
