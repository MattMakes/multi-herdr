# CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 2 (lines 1979 to 2869)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger --locked -- queue_claim_and_status_through_the_cli a_spawned_worker_records_its_pids_session_and_checkins a_task_finishes_through_review_land_pr_and_cleanup a_critical_finding_fails_until_a_fix_is_reviewed_again stopping_one_of_three_workers_leaves_the_others_running a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid a_dead_fleetd_is_restarted_by_the_next_command the_no_watchdog_setting_turns_off_the_fleetd_check two_fleetds_started_at_once_leave_1 the_import_finds_work_that_never_landed trf_43_assign_is_gone the_whole_flow_on_the_fake_herdr a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits the_reviews_being_in_wakes_nobody_because_fleetd_carries_on legacy_plumbing_lives_under_ledger_legacy status_on_a_fresh_ledger_creates_it_and_says_no_runs && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows of `taskledger.rs`).

## Goal

Migrate the second range of tests in `crates/horch-e2e/tests/taskledger.rs` (from `a_failed_push_after_a_land_is_retried_by_the_next_finish` at line 1980 through `finish_finds_the_code_sweep_skill_in_the_plugins_of_the_home` at line 2805, including helpers `claimed_check` at line 2172, `sweep_world` at line 2338, and `complexity_step_with_the_analyser_in` at line 2746) from the retired `horch ledger run new` / `horch ledger add` CLI commands to `horch_e2e::ledger_seed`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch-e2e/tests/taskledger.rs` (lines 1900 to 2869).

## Tests covered (24 tests in lines 1979 to 2869 of `crates/horch-e2e/tests/taskledger.rs`)

1. `a_failed_push_after_a_land_is_retried_by_the_next_finish` (line 1980): `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
2. `a_replacement_commit_after_the_land_lands_and_is_pushed` (line 2012): `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
3. `a_commit_after_the_land_is_checked_and_reviewed_again` (line 2043): `horch ledger run new alpha --steps check,code-sweep,land,cleanup: status: exit status: 1 stdout:`
4. `a_landed_worktree_with_no_new_commit_passes_land_without_a_push` (line 2077): `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
5. `a_landed_worktree_passes_land_at_once_in_a_run_with_no_remote` (line 2092): `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
6. `a_check_past_its_timeout_is_killed_with_its_children` (line 2107): `horch ledger run new alpha --steps check: status: exit status: 1 stdout:`
7. `a_codex_workers_check_cannot_write_outside_its_worktree` (line 2202): `horch ledger run new alpha --steps check: status: exit status: 1 stdout:`
8. `a_codex_workers_sandboxed_check_past_its_timeout_leaves_no_process` (line 2240): `horch ledger run new alpha --steps check: status: exit status: 1 stdout:`
9. `a_land_checks_outside_the_lock_and_lands_after_a_faster_task` (line 2287): `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
10. `a_reviewer_that_cannot_run_hands_the_round_to_its_fallback` (line 2370): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
11. `a_queued_review_moves_to_its_fallback_persona` (line 2391): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
12. `a_cancelled_review_is_queued_again_and_the_sweep_then_passes` (line 2403): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
13. `only_the_orchestrator_waives_a_critical_finding` (line 2437): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
14. `a_new_tree_supersedes_the_open_reviews_of_the_old_round` (line 2485): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
15. `a_replacement_with_the_same_tree_reuses_the_finished_round` (line 2517): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
16. `a_missing_reviewer_prompt_queues_no_review` (line 2536): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
17. `a_rust_only_change_is_not_sent_to_the_complexity_analyser` (line 2550): `horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 stdout:`
18. `finish_finds_the_complexity_analyser_in_a_local_marketplace_of_the_store` (line 2571): `horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 stdout:`
19. `a_complexity_report_past_64_kib_is_read_whole` (line 2618): `horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 stdout:`
20. `an_outer_git_dir_reaches_no_child_of_finish` (line 2654): `horch ledger run new alpha --steps check,complexity-sweep,land,pr: status: exit status: 1 stdout:`
21. `finish_finds_the_code_sweep_skill_in_the_skill_store` (line 2715): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
22. `finish_finds_the_complexity_analyser_in_the_plugins_of_the_home` (line 2783): `horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 stdout:`
23. `finish_finds_the_complexity_analyser_in_the_skill_store` (line 2794): `horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 stdout:`
24. `finish_finds_the_code_sweep_skill_in_the_plugins_of_the_home` (line 2805): `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`

## Steps

1. In `crates/horch-e2e/tests/taskledger.rs` (lines 1979 to 2869, and helpers `claimed_with_commit` if not already migrated in part 1, `claimed_check`, `sweep_world`, and `complexity_step_with_the_analyser_in`), replace each `horch ledger run new` and `horch ledger add` CLI call with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add` with the requested finish steps).
2. Never weaken an assert. Build the workspace binaries and run the Check.

## Out of scope

- Tests outside lines 1979 to 2869 of `crates/horch-e2e/tests/taskledger.rs`.

## Done when

All 24 tests in lines 1979 to 2869 of `crates/horch-e2e/tests/taskledger.rs` pass, and the Check passes.

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

7. FIRST STEP of this task: land part 1. In your worktree run `git merge --ff-only keep/fleetd-finish-T90`. If that fails because the run branch moved: `git reset --hard keep/fleetd-finish-T90`, then `git rebase fleet/fleetd-finish`. The commit 9e1efb7d (or its rebased copy) must be in `git log`. Then run the Check 1 time before you start part 2.
