# CI FIX-10: fix the 4 unexplained failures in horch-core tests/check_sandbox.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test check_sandbox --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/check_sandbox.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Run each of the 4 class `f?` failing tests in `crates/horch-core/tests/check_sandbox.rs` 3 times locally through `scripts/dev-sandbox.sh`, find why `outcome(&row).unwrap()` panicked at line 254 with `called Option::unwrap() on a None value`, and fix the cause.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/tests/check_sandbox.rs` (lines 180 to 490) and `crates/horch-core/tests/fleetd_job_result_file.rs` (how `jobs::run_job` writes a job result file to `<fleet_root>/run/jobs/<id>-<run>.json` and `jobs::ingest_results` ingests it into `jobs.result_json`).

## Tests covered (4 class `f?` rows)

1. `check_job_has_no_forbidden_env` (`crates/horch-core/tests/check_sandbox.rs:254`, Linux + macOS): `called Option::unwrap() on a None value`
2. `check_runs_in_the_worker_sandbox_not_with_fleetd_rights` (`crates/horch-core/tests/check_sandbox.rs:254`, Linux + macOS): `called Option::unwrap() on a None value`
3. `claude_worker_check_runs_in_the_platform_sandbox` (`crates/horch-core/tests/check_sandbox.rs:254`, Linux + macOS): `called Option::unwrap() on a None value`
4. `codex_worker_check_keeps_codex_sandbox` (`crates/horch-core/tests/check_sandbox.rs:254`, Linux + macOS): `called Option::unwrap() on a None value`

## Cause (verify in `crates/horch-core/tests/check_sandbox.rs:245-255`)

In `World::check` (`crates/horch-core/tests/check_sandbox.rs:251-254`), `jobs::run_job(&ctx, &mut self.conn, id, me, &checks)` writes the job result file to `<fleet_root>/run/jobs/<id>-1.json` (per RR-5 / SR-6, `run_job` no longer updates the `jobs` row in SQLite directly). Because `World::check` does not call `jobs::ingest_results` (or `jobs::record_facts`) before reading `let row = self.row(id); (row.state, outcome(&row).unwrap())` at line 254, `row.result_json` is still `None` and `outcome(&row).unwrap()` panics.

## Steps

1. Run `crates/horch-core/tests/check_sandbox.rs` through `scripts/dev-sandbox.sh` and confirm the panic at line 254.
2. In `crates/horch-core/tests/check_sandbox.rs` (`World::check` around line 251), after `jobs::run_job(&ctx, &mut self.conn, id, me, &checks).unwrap()`, ingest the written job result file into `self.conn` (using `job_result_file::read_all` and `jobs::ingest_results` in a transaction, matching `crates/horch-core/tests/fleetd_job_result_file.rs`) before reading `self.row(id)` and `outcome(&row).unwrap()`.
3. Run all tests in `crates/horch-core/tests/check_sandbox.rs` 3 times through `scripts/dev-sandbox.sh`. Never weaken an assert.
4. Run the Check.

## Out of scope

- Any file outside `crates/horch-core/tests/check_sandbox.rs`.

## Done when

All tests in `crates/horch-core/tests/check_sandbox.rs` pass 3 times in a row, and the Check passes.

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
