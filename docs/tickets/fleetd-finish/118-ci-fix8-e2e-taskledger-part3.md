# CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 3 (lines 2870 to 3362)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger --locked -- queue_claim_and_status_through_the_cli a_spawned_worker_records_its_pids_session_and_checkins a_task_finishes_through_review_land_pr_and_cleanup a_critical_finding_fails_until_a_fix_is_reviewed_again stopping_one_of_three_workers_leaves_the_others_running a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid a_dead_fleetd_is_restarted_by_the_next_command the_no_watchdog_setting_turns_off_the_fleetd_check two_fleetds_started_at_once_leave_1 the_import_finds_work_that_never_landed trf_43_assign_is_gone the_whole_flow_on_the_fake_herdr a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits the_reviews_being_in_wakes_nobody_because_fleetd_carries_on legacy_plumbing_lives_under_ledger_legacy status_on_a_fresh_ledger_creates_it_and_says_no_runs && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows of `taskledger.rs`).

## Goal

Migrate the third range of tests in `crates/horch-e2e/tests/taskledger.rs` (from `a_command_means_the_run_it_names_before_the_run_of_its_worker` at line 2871 through `the_watchdog_ends_pre_claim_rows_whose_process_is_gone` at line 3293, including helpers `queue` at line 2852 and `queue_for` at line 3067) from the retired `horch ledger run new` / `horch ledger add` CLI commands to `horch_e2e::ledger_seed` (or delete tests of behaviors that Phase 6 removed on purpose).

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch-e2e/tests/taskledger.rs` (lines 2850 to 3362).

## Tests covered (12 tests in lines 2870 to 3362 of `crates/horch-e2e/tests/taskledger.rs`)

1. `a_command_means_the_run_it_names_before_the_run_of_its_worker` (line 2871): `horch ledger run new a: status: exit status: 1 stdout:`
2. `a_command_from_another_workspace_says_whose_run_it_uses` (line 2927): `horch ledger run new a: status: exit status: 1 stdout:`
3. `a_closed_run_frees_the_project_for_the_next_run` (line 2956): `horch ledger run new a: status: exit status: 1 stdout:`
4. `close_abandon_cancels_the_open_tasks` (line 2997): `horch ledger run new a: status: exit status: 1 stdout:`
5. `run_new_refuses_an_old_run_branch_behind_the_main_branch` (line 3012): `horch ledger run new a: status: exit status: 1 stdout:`
6. `ledger_watch_points_at_fleetd` (line 3027): `horch ledger run new a --check-every 10: status: exit status: 1 stdout:`
7. `spawn_run_names_the_run_the_worker_joins` (line 3088): `status: exit status: 1 stdout:`
8. `a_spawn_joins_the_run_of_its_own_workspace` (line 3122): `horch ledger run new a: status: exit status: 1 stdout:`
9. `trf_43_a_spawn_with_task_text_refuses_with_the_ledger_task_command` (line 3160): `horch ledger run new a: status: exit status: 1 stdout:`
10. `a_refused_spawn_ends_its_worker_row` (line 3241): `horch ledger run new a: status: exit status: 1 stdout:`
11. `a_spawn_of_an_unknown_teammate_leaves_no_live_row` (line 3273): `horch ledger run new a: status: exit status: 1 stdout:`
12. `the_watchdog_ends_pre_claim_rows_whose_process_is_gone` (line 3293): `horch ledger run new a: status: exit status: 1 stdout:`

## Steps

1. In `crates/horch-e2e/tests/taskledger.rs` (lines 2850 to 3362), migrate the run and task setup in helpers `queue` and `queue_for` and in each test from `horch ledger run new` / `horch ledger add` to `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add`).
2. If any test in this range proves a CLI behavior that Phase 6 removed on purpose (for example `run_new_refuses_an_old_run_branch_behind_the_main_branch` or `horch spawn` worker-claim commands retired by `FR-5` and `FR-12` in Phase 6 ticket 47), either test the underlying `runs::create` library rule or delete the test of the retired CLI command and write the reason and design rule (`FR-5`, `FR-12`) in the commit message. Never weaken an assert.
3. Build the workspace binaries and run the Check (excluding any test deleted under step 2).

## Out of scope

- Tests outside lines 2870 to 3362 of `crates/horch-e2e/tests/taskledger.rs`.

## Done when

All kept tests in lines 2870 to 3362 of `crates/horch-e2e/tests/taskledger.rs` pass, and the Check passes.

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


## Changed 2026-11-20T18:30Z: read this first

1. The Check of this ticket changed. The ledger holds the valid Check: it builds the bins and runs the 16 tests of part 1 (they must stay green). It does not name the tests of your part, because their names can change. Thus you must also run each test of your part 3 times and write the result (names, 3 of 3) in your DONE note. Cargo rule: only 1 filter goes before `--`; `--locked` goes before `--`.
2. Pattern: read the commit message of 9e1efb7d (part 1). It shows the seed helpers (`seed_run`, `seed_run_with`, `seed_task_with`), the push form of a finish test, and how a dropped assert is written. Do the same.
3. Rules for a test of a retired behavior: move the assert to the designed new effect. Delete a test only when no form of the rule exists, and name the test, the retired behavior and the design text in the commit message. For each dropped assert, name the design text that retires it; if you find none, send `QUESTION:`. Do not weaken an assert. No `#[ignore]`.
4. A test that fails each run because of a known product fault gets `expected::check` and 1 line in `crates/horch-e2e/expected-failures/taskledger.txt` that names the owner ticket (for example ticket 168, T-207: the tree of landed work is not removed). That file is in your Own. If no ticket owns the fault, send `QUESTION:` with the evidence.
5. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
6. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
