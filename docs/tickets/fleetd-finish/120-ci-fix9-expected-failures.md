# CI FIX-9: verify the 7 expected-failures rows and audit expected_failures_list_gates_the_run

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test expected_failures --test taskledger --test fleet18_observability --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/expected-failures/fleet18_observability.txt`, `crates/horch-e2e/expected-failures/taskledger.txt`, `crates/horch-e2e/tests/expected_failures.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-9).

## Goal

On CI, 7 tests in `crates/horch-e2e/expected-failures/taskledger.txt` (2 rows) and `crates/horch-e2e/expected-failures/fleet18_observability.txt` (5 rows) failed in class `(a)` because the e2e build failed first (`--profile out`) so their panic reason did not match the listed reason. After FIX-1 (ticket 100) and the landing chain tickets 13 (T-13), 14 (T-14), and 15 (T-15), verify that both `taskledger.txt` and `fleet18_observability.txt` have no active entries, run each of the 7 tests 3 times (Rule 7), and verify `expected_failures_list_gates_the_run`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` (class `a` rows) and `## Fix units` row FIX-9.
- `crates/horch-e2e/expected-failures/README.md`, `crates/horch-e2e/expected-failures/taskledger.txt`, `crates/horch-e2e/expected-failures/fleet18_observability.txt`, and `crates/horch-e2e/tests/expected_failures.rs`.

## Tests covered (7 class `a` rows of the CI report)

1. `crates/horch-e2e/expected-failures/taskledger.txt` (2 rows):
   - `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid` (`crates/horch-e2e/src/expected.rs:108`): `` `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid` is on the list of expected failures with the reason `its old process lived in the pass` ``
   - `stopping_one_of_three_workers_leaves_the_others_running` (`crates/horch-e2e/src/expected.rs:108`): `` `stopping_one_of_three_workers_leaves_the_others_running` is on the list of expected failures with the reason `timed out waiting for the stopped agent is gone` ``
2. `crates/horch-e2e/expected-failures/fleet18_observability.txt` (5 rows):
   - `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` (`crates/horch-e2e/src/expected.rs:108`): `` `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` is on the list of expected failures with the reason `horch ledger run new is retired` ``
   - `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` (`crates/horch-e2e/src/expected.rs:108`): `` `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` is on the list of expected failures with the reason `horch ledger run new is retired` ``
   - `g52_resume_uses_own_session` (`crates/horch-e2e/src/expected.rs:108`): `` `g52_resume_uses_own_session` is on the list of expected failures with the reason `horch spawn refused: no ready task for persona codex-terra` ``
   - `g53_ledger_handoff_compacts_resumes_and_records_progress_once` (`crates/horch-e2e/src/expected.rs:108`): `` `g53_ledger_handoff_compacts_resumes_and_records_progress_once` is on the list of expected failures with the reason `horch ledger run new is retired` ``
   - `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` (`crates/horch-e2e/src/expected.rs:108`): `` `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` is on the list of expected failures with the reason `horch ledger run new is retired` ``

## Steps

1. Inspect `crates/horch-e2e/expected-failures/taskledger.txt` and `crates/horch-e2e/expected-failures/fleet18_observability.txt` after T-13, T-14, and T-15 have landed. Confirm that neither file has any remaining active entry (`grep -v '^#' crates/horch-e2e/expected-failures/*.txt | grep -v '^$'`). If any of the 7 entries is still listed while the test now passes, remove the entry from its list file.
2. Run each of the 7 tests 3 times through `scripts/dev-sandbox.sh` (Rule 7):
   - `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger -- a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid stopping_one_of_three_workers_leaves_the_others_running --locked || exit 1; done`
   - `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability -- g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block g51_sessions_points_to_ledger_status_only_for_the_callers_active_run g52_resume_uses_own_session g53_ledger_handoff_compacts_resumes_and_records_progress_once ledger_stop_stops_a_spawned_worker_by_the_ingested_pid --locked || exit 1; done`
3. Run the Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test expected_failures --test taskledger --test fleet18_observability --locked && cargo fmt --all --check`

## Out of scope

- Modifying product code outside `**Own:**`.

## Done when

All 7 tests pass 3 times in a row, `expected_failures_list_gates_the_run` passes, and the Check passes.

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
