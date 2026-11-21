# CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 1 (lines 304 to 1980)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger -- queue_claim_and_status_through_the_cli add_map_queues_a_ticket_map_in_one_step a_spawned_worker_records_its_pids_session_and_checkins a_task_finishes_through_review_land_pr_and_cleanup a_critical_finding_fails_until_a_fix_is_reviewed_again a_failed_reviews_in_tell_wakes_the_orchestrator_once a_failed_reviews_in_push_is_redelivered_until_the_author_runs_finish stopping_one_of_three_workers_leaves_the_others_running a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid a_dead_fleetd_is_restarted_by_the_next_command the_no_watchdog_setting_turns_off_the_fleetd_check two_fleetds_started_at_once_leave_1 the_import_finds_work_that_never_landed trf_43_assign_is_gone the_whole_flow_on_the_fake_herdr a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows of `taskledger.rs`).

## Goal

Migrate the first range of tests in `crates/horch-e2e/tests/taskledger.rs` (from `queue_claim_and_status_through_the_cli` at line 304 through `a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits` at line 1827, plus shared helpers `claimed_run` at line 1336 and `claimed_with_commit` at line 1904) from the retired `horch ledger run new` / `horch ledger add` / `horch ledger add-map` CLI commands to `horch_e2e::ledger_seed`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch-e2e/tests/taskledger.rs` (lines 1 to 1978, after T-13 and T-14 have landed).

## Tests covered (16 tests in lines 304 to 1978 of `crates/horch-e2e/tests/taskledger.rs`)

1. `queue_claim_and_status_through_the_cli` (line 304): `horch ledger run new alpha: status: exit status: 1 stdout:`
2. `add_map_queues_a_ticket_map_in_one_step` (line 395): `horch ledger run new beta: status: exit status: 1 stdout:`
3. `a_spawned_worker_records_its_pids_session_and_checkins` (line 451): `horch ledger run new gamma --heartbeat-every 1: status: exit status: 1 stdout:`
4. `a_task_finishes_through_review_land_pr_and_cleanup` (line 668): `horch ledger run new alpha --verify full: status: exit status: 1 stdout:`
5. `a_critical_finding_fails_until_a_fix_is_reviewed_again` (line 800): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
6. `a_failed_reviews_in_tell_wakes_the_orchestrator_once` (line 888): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
7. `a_failed_reviews_in_push_is_redelivered_until_the_author_runs_finish` (line 950): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
8. `stopping_one_of_three_workers_leaves_the_others_running` (line 1074, updated in T-13): `horch ledger run new delta --heartbeat-every 1: status: exit status: 1 stdout:`
9. `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid` (line 1145, updated in T-13): `horch ledger run new eps --heartbeat-every 1: status: exit status: 1 stdout:`
10. `a_dead_fleetd_is_restarted_by_the_next_command` (line 1360, via `claimed_run` at line 1336): `horch ledger run new zeta --check-every 1: status: exit status: 1 stdout:`
11. `the_no_watchdog_setting_turns_off_the_fleetd_check` (line 1383): `horch ledger run new zeta: status: exit status: 1 stdout:`
12. `two_fleetds_started_at_once_leave_1` (line 1401, via `claimed_run` at line 1336): `horch ledger run new zeta --check-every 1: status: exit status: 1 stdout:`
13. `the_import_finds_work_that_never_landed` (line 1428, and line 1577 `run new again`): `EOF while parsing a value at line 1 column 0: status: exit status: 1 stdout:`
14. `trf_43_assign_is_gone` (line 1601): `horch ledger run new eta: status: exit status: 1 stdout:`
15. `the_whole_flow_on_the_fake_herdr` (line 1637, updated in T-14): `horch ledger run new alpha --heartbeat-every 1 --verify full: status: exit status: 1 stdout:`
16. `a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits` (line 1827): `horch ledger run new alpha --steps check,land: status: exit status: 1 stdout:`

## Steps

1. In `crates/horch-e2e/tests/taskledger.rs` (lines 304 to 1978, including helper `claimed_run` at line 1336 and helper `claimed_with_commit` at line 1904), replace each `horch ledger run new` and `horch ledger add` CLI call with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add` when custom `steps`, `verify`, or `thresholds` are needed).
2. For `add_map_queues_a_ticket_map_in_one_step` (line 395), which tests `horch ledger add-map` without `--dry-run` (retired in Phase 6), migrate it to `horch ledger add-map --dry-run` (or if it tests the retired queueing CLI arm that Phase 6 removed on purpose, delete the test and write the reason and design rule `FR-12` in the commit message). Never weaken an assert.
3. Build the workspace binaries and run the Check.

## Out of scope

- Tests in `crates/horch-e2e/tests/taskledger.rs` from line 1979 onward (covered by tickets 117, 118, and 119, which run in sequence after this ticket).

## Done when

All tests in lines 304 to 1978 of `crates/horch-e2e/tests/taskledger.rs` pass, and the Check passes.

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

- `legacy_plumbing_lives_under_ledger_legacy` — `horch [ledger, legacy, init, --goal, legacy check] failed: error: unrecognized subcommand 'legacy'`
- `status_on_a_fresh_ledger_creates_it_and_says_no_runs` — `status failed: horch: reading the operator token at ~/.horch/operator.token: No such file or directory (os error 2)`
