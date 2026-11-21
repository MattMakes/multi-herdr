# CI FIX-10: fix unexplained failures in horch-core fleetd_handlers_custody.rs, fleetd_pass_p3.rs, trial_merges.rs, w3_finish_steps.rs and w3_taskledger_core_status.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_handlers_custody --test fleetd_pass_p3 --test trial_merges --test w3_finish_steps --test w3_taskledger_core_status --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/fleetd_handlers_custody.rs`, `crates/horch-core/tests/fleetd_pass_p3.rs`, `crates/horch-core/tests/trial_merges.rs`, `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/w3_taskledger_core_status.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Run each of the 10 class `f?` tests in `crates/horch-core/tests/fleetd_handlers_custody.rs` (1 test), `crates/horch-core/tests/fleetd_pass_p3.rs` (3 tests), `crates/horch-core/tests/trial_merges.rs` (3 tests), `crates/horch-core/tests/w3_finish_steps.rs` (1 test), and `crates/horch-core/tests/w3_taskledger_core_status.rs` (2 tests) 3 times locally through `scripts/dev-sandbox.sh`, find the cause of each failure, and fix it.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- The 5 test files in `**Own:**`.

## Tests covered (10 class `f?` rows)

1. `crates/horch-core/tests/fleetd_handlers_custody.rs` (1 test):
   - `worktree_triage_queues_its_jobs` (`crates/horch-core/tests/fleetd_handlers_custody.rs:153`, Linux + macOS): `applied: {"applied":[],"failed":0,"job":2,"queued":["W1 _scratch/t1 ARCHIVE: queued in the triage_apply job 2; horch worktree tria`
2. `crates/horch-core/tests/fleetd_pass_p3.rs` (3 tests):
   - `pass_adopts_a_hand_made_worktree_within_1_list_period` (`crates/horch-core/tests/fleetd_pass_p3.rs:325`, Linux + macOS): `job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no`
   - `worktree_list_job_is_queued_once_per_period_per_repository` (`crates/horch-core/tests/fleetd_pass_p3.rs:325`, Linux + macOS): `job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no`
   - `worktree_list_job_writes_no_index_of_a_tree` (`crates/horch-core/tests/fleetd_pass_p3.rs:325`, Linux + macOS): `job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no`
3. `crates/horch-core/tests/trial_merges.rs` (3 tests):
   - `conflict_is_recorded_with_its_files` (`crates/horch-core/tests/trial_merges.rs:367`, Linux + macOS): `assert_eq failed: a conflict is a failed run`
   - `trial_merge_job_does_not_check_out` (`crates/horch-core/tests/trial_merges.rs:412`, Linux + macOS): `assert_eq failed`
   - `trial_merge_of_2_tips_runs_once` (`crates/horch-core/tests/trial_merges.rs:329`, Linux + macOS): `assert_eq failed`
4. `crates/horch-core/tests/w3_finish_steps.rs` (1 test):
   - `worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree` (`crates/horch-core/tests/w3_finish_steps.rs:1535`, Linux + macOS): `Err: T-2 has the attempt 430556fa-70e5-4647-b4e2-1aa0613234de/A1 that is not ended: 1 task has 1 live attempt (I1)`
5. `crates/horch-core/tests/w3_taskledger_core_status.rs` (2 tests):
   - `an_empty_run_renders_no_tasks_and_is_complete` (`crates/horch-core/tests/w3_taskledger_core_status.rs:163`, Linux + macOS): `assert_eq failed`
   - `render_prints_each_section` (`crates/horch-core/tests/w3_taskledger_core_status.rs:140`, Linux + macOS): `assert_eq failed`

## Steps

1. Run each of the 10 tests locally through `scripts/dev-sandbox.sh` and inspect the failure:
   - In `crates/horch-core/tests/fleetd_handlers_custody.rs`, check why `worktree_triage_queues_its_jobs` failed after `triage_apply` became a queued job (Phase 3/6) and update the test setup/assertion for the `triage_apply` job flow.
   - In `crates/horch-core/tests/fleetd_pass_p3.rs` and `crates/horch-core/tests/trial_merges.rs`, check why `jobs::run_job` in the test helper left the job in `State::Running`: after the RR-5 / SR-6 job-result-file migration, `jobs::run_job` writes `<fleet_root>/run/jobs/<id>-<run>.json` and a pass (or `jobs::ingest_results`) ingests that result file to move the `jobs` row to `State::Passed` / `State::Failed`. Update the test helpers (`Watching` in `fleetd_pass_p3.rs` and `World::run` in `trial_merges.rs`) to ingest the job result file (or run the pass that ingests it).
   - In `crates/horch-core/tests/w3_finish_steps.rs:1535` (`worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree`), check where the second worker claim on `T-2` violates invariant `I1` (`1 task has 1 live attempt`) before calling `finish::worktree_new_with`, and end the first attempt or test re-entry with the same worker/attempt.
   - In `crates/horch-core/tests/w3_taskledger_core_status.rs:140` and `:163`, inspect the rendered `status::render` output (such as the `Feature F-0001 (r1) open: not pushed.` line or `schema::run_idea` in `an_empty_run_renders_no_tasks_and_is_complete` / `render_prints_each_section`) and update the test fixture or expected render string.
2. Never weaken an assert. Run each test 3 times through `scripts/dev-sandbox.sh`.
3. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 10 tests pass 3 times in a row, and the Check passes.

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
