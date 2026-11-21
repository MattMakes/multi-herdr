# CI FIX-8: migrate stale tests in horch-e2e i308_fleet_steward.rs, i308_triage.rs, skill_intake_spawn.rs, watchdog_liveness.rs and worktree.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test i308_fleet_steward --test i308_triage --test skill_intake_spawn --test watchdog_liveness --test worktree --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/i308_fleet_steward.rs`, `crates/horch-e2e/tests/i308_triage.rs`, `crates/horch-e2e/tests/skill_intake_spawn.rs`, `crates/horch-e2e/tests/watchdog_liveness.rs`, `crates/horch-e2e/tests/worktree.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 77-88, 155-159, 229-230).

## Goal

Migrate the 18 stale e2e tests in `crates/horch-e2e/tests/i308_fleet_steward.rs` (7 tests), `crates/horch-e2e/tests/i308_triage.rs` (4 tests), `crates/horch-e2e/tests/skill_intake_spawn.rs` (5 tests), `crates/horch-e2e/tests/watchdog_liveness.rs` (1 test), and `crates/horch-e2e/tests/worktree.rs` (1 test) from the retired `horch ledger run new` / `horch ledger add` / `horch ledger task` commands to `horch_e2e::ledger_seed` (or delete tests of behaviors that Phase 6 removed on purpose).

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- The 5 test files in `**Own:**`.

## Tests covered (18 tests across 5 files)

1. `crates/horch-e2e/tests/i308_fleet_steward.rs` (7 rows of `red-baseline.txt`):
   - `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `a_worker_can_drop_a_clean_worktree_from_an_explicit_other_run`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `an_exhausted_claude_pool_starts_no_codex_steward`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `the_no_change_steward_task_finishes_without_a_worktree`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `triage_delete_refuses_a_detached_unique_commit`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `triage_lists_rows_from_a_ledger_it_cannot_write`: `horch ledger run new alpha: status: exit status: 1 stdout:`
2. `crates/horch-e2e/tests/i308_triage.rs` (4 rows of `red-baseline.txt`; note that `finish_queues_a_task_that_inherits_the_worktree_and_wakes_the_orchestrator` was already removed in `947c2f2f`):
   - `a_bad_reply_names_its_lines_and_runs_nothing`: `horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `archive_restore_and_gc_through_the_cli`: `horch worktree triage apply <tmp> status: exit status: 1 stdout:`
   - `run_close_abandon_cancels_then_refuses_until_the_worktree_is_decided`: `horch ledger run new alpha: status: exit status: 1 stdout:`
   - `run_close_refuses_while_a_parked_worktree_has_no_decision`: `horch ledger run new alpha: status: exit status: 1 stdout:`
3. `crates/horch-e2e/tests/skill_intake_spawn.rs` (5 rows of `red-baseline.txt`):
   - `a_controller_in_a_run_spawns_a_role_that_joins_the_run`: `status: exit status: 1 stdout:`
   - `a_report_role_with_a_row_and_no_task_runs_to_done`: `status: exit status: 1 stdout:`
   - `trf_43_a_controller_outside_every_run_cannot_send_a_brief`: `status: exit status: 1 stdout:`
   - `trf_43_a_spoofed_pane_id_cannot_send_a_brief`: `status: exit status: 1 stdout:`
   - `trf_43_a_worker_without_spawn_only_cannot_send_a_brief`: `status: exit status: 1 stdout:`
4. `crates/horch-e2e/tests/watchdog_liveness.rs` (1 row of `red-baseline.txt`):
   - `a_run_made_outside_herdr_warns_and_the_first_pane_plan_records_its_workspace`: `horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
5. `crates/horch-e2e/tests/worktree.rs` (1 row of `red-baseline.txt` and class `f?` in the CI report):
   - `arc_31_e2e_a_ledger_worktree_is_not_unregistered` (`crates/horch-e2e/tests/worktree.rs:255`):
     - CI first error line: `error: test failed, to rerun pass -p horch-e2e --test worktree`
     - `red-baseline.txt` first error line: `horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`

## Steps

1. In `crates/horch-e2e/tests/i308_fleet_steward.rs`, `crates/horch-e2e/tests/i308_triage.rs`, `crates/horch-e2e/tests/skill_intake_spawn.rs`, `crates/horch-e2e/tests/watchdog_liveness.rs`, and `crates/horch-e2e/tests/worktree.rs`, replace each `horch ledger run new`, `horch ledger add`, and `horch ledger task` setup call with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` + `runs::create` + `schema::run_idea` + `tasks::add`).
2. If any test in these 5 files proves a behavior that Phase 6 removed on purpose (for example `horch ledger run new` outside herdr or `horch spawn` / `horch ledger task` retired commands), delete that test and write the reason and the design rule (`FR-5`, `FR-12`, Phase 6 ticket 47) in the commit message. Never weaken an assert.
3. For `arc_31_e2e_a_ledger_worktree_is_not_unregistered` in `crates/horch-e2e/tests/worktree.rs`, run it 3 times after migrating its `run new` and `add` setup to `ledger_seed`.
4. Build the workspace binaries and run the Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test i308_fleet_steward --test i308_triage --test skill_intake_spawn --test watchdog_liveness --test worktree --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `i308_fleet_steward.rs`, `i308_triage.rs`, `skill_intake_spawn.rs`, `watchdog_liveness.rs`, and `worktree.rs` pass, and the Check passes.

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

- `crates/horch-e2e/tests/i308_fleet_steward.rs`:
  - `a_fleet_with_no_parked_worktree_spawns_no_steward` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/i308_triage.rs`:
  - `finish_queues_a_task_that_inherits_the_worktree` — `horch [new, --goal, g, --finish, check,land] failed: horch: 'horch ledger run new' is retired. Add work with 'horch idea add "<text>"'; fleetd plans and starts the run.`
- `crates/horch-e2e/tests/watchdog_liveness.rs`:
  - `a_busy_pi_worker_with_no_timer_is_kept_until_stall_after_then_stopped_at_rung_3` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `a_detached_fleetd_does_not_inherit_the_starting_panes_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `a_fleetd_started_from_another_workspace_wakes_the_runs_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `a_wake_from_another_workspace_goes_to_the_runs_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/worktree.rs`:
  - `arc_31_e2e_done_refuses_then_unlanded_reports` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `arc_31_e2e_list_and_done_survive_a_deleted_worktree_folder` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
