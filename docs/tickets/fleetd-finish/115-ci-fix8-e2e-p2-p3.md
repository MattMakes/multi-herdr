# CI FIX-8: migrate stale tests in horch-e2e p2_leases.rs, p2_release.rs, p3_refusals.rs, p3_release.rs, p3_unlanded.rs and p3_worktrees.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p2_leases --test p2_release --test p3_refusals --test p3_release --test p3_unlanded --test p3_worktrees --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p2_leases.rs`, `crates/horch-e2e/tests/p2_release.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/tests/p3_release.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`, `crates/horch-e2e/tests/p3_worktrees.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 111-129, 140-154).

## Goal

Migrate the 34 stale e2e tests in `crates/horch-e2e/tests/p2_leases.rs` (7 tests), `crates/horch-e2e/tests/p2_release.rs` (1 test), `crates/horch-e2e/tests/p3_refusals.rs` (10 tests), `crates/horch-e2e/tests/p3_release.rs` (1 test), `crates/horch-e2e/tests/p3_unlanded.rs` (8 tests), and `crates/horch-e2e/tests/p3_worktrees.rs` (7 tests) from the retired `horch ledger run new` / `horch ledger add` CLI commands (and the Phase 5 `horch ledger progress` outbox / channel contract) to `horch_e2e::ledger_seed`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- The 6 test files in `**Own:**`.

## Tests covered (34 rows of `red-baseline.txt`)

1. `crates/horch-e2e/tests/p2_leases.rs` (7 tests):
   - `cancel_of_a_blocked_on_user_task_is_immediate`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `cancel_of_a_waiting_quota_task_is_immediate`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `fm14_no_second_attempt_starts_while_the_old_process_lives`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `rr1_stopped_worker_is_caught_within_1_lease`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `rr2_expired_lease_never_leaves_the_task_assigned`: `horch ledger run new p2: status: exit status: 1 stdout:`
   - `rr6_task_that_fails_each_attempt_escalates_after_max_attempts`: `horch ledger run new p2 --max-attempts 2: status: exit status: 1 stdout:`
2. `crates/horch-e2e/tests/p2_release.rs` (1 test):
   - `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease`: `horch ledger progress building: status: exit status: 1 stdout:`
3. `crates/horch-e2e/tests/p3_refusals.rs` (10 tests):
   - `cleanup_after_land_removes_a_landed_tree_through_the_remove_job`: `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
   - `cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed`: `horch ledger run new alpha --steps land,cleanup: status: exit status: 1 stdout:`
   - `dismiss_cleanup_rescues_then_removes_only_the_recorded_path`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `drop_of_a_merged_branch_deletes_it_and_keep_branch_keeps_it`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr13_drop_of_a_branch_with_a_stash_entry_is_refused_and_the_tree_stays`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr13_drop_of_a_dirty_tree_is_refused_and_the_tree_stays`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr13_drop_of_unpublished_unique_commits_is_refused_and_the_tree_stays`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `run_gate_folder_is_added_and_removed_through_the_jobs`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
4. `crates/horch-e2e/tests/p3_release.rs` (1 test):
   - `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update`: `horch ledger progress building: status: exit status: 1 stdout:`
5. `crates/horch-e2e/tests/p3_unlanded.rs` (8 tests):
   - `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `pres_04_l9_unlanded_work_is_listed_and_decided_once`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr14_requeue_of_a_parked_task_runs_in_the_same_tree`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr15_landed_work_of_a_done_task_is_not_listed`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr15_rescued_work_that_did_not_land_stays_listed_after_the_task_is_done`: `horch ledger run new alpha --steps land: status: exit status: 1 stdout:`
   - `rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0`: `horch ledger run new alpha --steps land --max-attempts 2: status: exit status: 1 stdout:`
   - `rr18_review_requeue_keeps_the_attempt_count`: `horch ledger run new alpha --steps code-sweep: status: exit status: 1 stdout:`
6. `crates/horch-e2e/tests/p3_worktrees.rs` (7 tests):
   - `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref`: `horch ledger run new fm26: status: exit status: 1 stdout:`
   - `fm27_only_landed_or_rescued_trees_are_removed`: `horch ledger run new fm27 --steps land: status: exit status: 1 stdout:`
   - `fm27_vanished_tree_opens_1_needs_you_item_within_60_s`: `horch ledger run new fm27: status: exit status: 1 stdout:`
   - `fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree`: `horch ledger run new old: status: exit status: 1 stdout:`
   - `fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass`: `horch ledger run new fr11: status: exit status: 1 stdout:`
   - `rr10_hand_made_worktree_is_adopted_within_1_list_period`: `horch ledger run new rr10: status: exit status: 1 stdout:`
   - `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed`: `horch ledger run new slow: status: exit status: 1 stdout:`

## Steps

1. In `crates/horch-e2e/tests/p2_leases.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`, and `crates/horch-e2e/tests/p3_worktrees.rs`, replace all `horch ledger run new` and `horch ledger add` calls with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add` when custom steps or `max_attempts` are set).
2. In `crates/horch-e2e/tests/p2_release.rs` and `crates/horch-e2e/tests/p3_release.rs`, inspect why `worker_at(&h, now, &["ledger", "progress", "building"])` exited 1 after the Phase 5/6 migration (for example missing operator token or `HORCH_TASK_DIR` / `serve_fleetd` channel in the release test fixture) and update the fixture so the release tests pass.
3. Never weaken an assert. Build the workspace binaries and run the Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p2_leases --test p2_release --test p3_refusals --test p3_release --test p3_unlanded --test p3_worktrees --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 34 tests across `p2_leases.rs`, `p2_release.rs`, `p3_refusals.rs`, `p3_release.rs`, `p3_unlanded.rs`, and `p3_worktrees.rs` pass, and the Check passes.

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

## Added 2026-11-20: the unit T-897 lands at the same time

Task T-188 lands the branch of T-897 (worker question in a push attempt) while you work. It removes 1 test of `fleet18_messaging.rs` and changes `crates/horch-e2e/src/agent.rs` and the question ingest of fleetd. Before your last Check run: `git fetch` and `git rebase fleet/fleetd-finish`; if T-188 is not done then, wait for it (`horch ledger status`) and rebase after. Resolve a conflict with no change of an assert. Do not fix a test that T-897 removed.
