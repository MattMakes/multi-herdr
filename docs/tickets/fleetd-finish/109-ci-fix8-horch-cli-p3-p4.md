# CI FIX-8: migrate stale CLI tests in p3_worktrees_cli.rs, p4_question_file.rs and p4_result_cli.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --test p3_worktrees_cli --test p4_question_file --test p4_result_cli --locked && cargo fmt --all --check`
**Own:** `crates/horch/tests/p3_worktrees_cli.rs`, `crates/horch/tests/p4_question_file.rs`, `crates/horch/tests/p4_result_cli.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 13-31).

## Goal

Migrate the test setup in `crates/horch/tests/p3_worktrees_cli.rs` (9 tests), `crates/horch/tests/p4_question_file.rs` (4 tests), and `crates/horch/tests/p4_result_cli.rs` (6 tests) from the retired `horch ledger run new` / `horch ledger add` CLI commands to direct ledger seeding (`runs::create`, `schema::run_idea`, `tasks::add`).

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch/tests/p3_worktrees_cli.rs`, `crates/horch/tests/p4_question_file.rs`, and `crates/horch/tests/p4_result_cli.rs`.

## Tests covered (19 tests: 18 class `f` + 1 class `f?`, all 19 in `red-baseline.txt`)

1. `crates/horch/tests/p3_worktrees_cli.rs` (9 tests):
   - `ledger_status_shows_the_total_size_of_all_trees` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `restore_of_an_archive_name_keeps_today_s_behavior` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `restore_runs_no_hook_of_the_project` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `worktrees_json_has_each_field` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `worktrees_lists_each_tree_with_state_task_owner_age_and_size` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `worktrees_shows_the_rescue_job_that_the_reconciler_queued_for_an_orphan` (`crates/horch/tests/p3_worktrees_cli.rs:178`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
2. `crates/horch/tests/p4_question_file.rs` (4 tests: 3 class `f` + 1 class `f?` from the same `World::new()` at line 81):
   - `a_long_question_file_stays_under_64_kib` (`crates/horch/tests/p4_question_file.rs:186`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `question_of_a_claim_attempt_writes_no_file` (`crates/horch/tests/p4_question_file.rs:186`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `question_of_a_push_attempt_writes_1_question_file` (`crates/horch/tests/p4_question_file.rs:186`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"` (listed as `f?` in the CI report due to interleaved cargo output)
   - `two_questions_write_2_files` (`crates/horch/tests/p4_question_file.rs:186`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
3. `crates/horch/tests/p4_result_cli.rs` (6 tests):
   - `check_exits_2_with_neither_file_and_0_with_one` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `oversized_result_is_refused` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `result_json_with_another_status_is_refused` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `result_outside_a_push_attempt_is_refused` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `result_writes_a_valid_file_once` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `second_result_of_1_attempt_is_refused` (`crates/horch/tests/p4_result_cli.rs:194`): `assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`

## Steps

1. In `crates/horch/tests/p3_worktrees_cli.rs`, `crates/horch/tests/p4_question_file.rs`, and `crates/horch/tests/p4_result_cli.rs`, replace the `World::new` setup calls to `["ledger", "run", "new", "r1"]` and `["ledger", "add", ...]` with direct ledger store seeding (`Store::open`, `runs::create`, `schema::run_idea`, `tasks::add`, and creating branch `fleet/r1` from `main` before starting `TestFleetd`, matching `crates/horch-e2e/src/ledger_seed.rs`).
2. Never weaken an assert. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch --test p3_worktrees_cli --test p4_question_file --test p4_result_cli --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All 19 tests across `p3_worktrees_cli.rs`, `p4_question_file.rs`, and `p4_result_cli.rs` pass, and the Check passes.

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
