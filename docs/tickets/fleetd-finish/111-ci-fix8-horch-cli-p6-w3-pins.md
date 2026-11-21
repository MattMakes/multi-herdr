# CI FIX-8: fix stale tests in p6_retired_run_commands.rs, p6_retired_worker_commands.rs and w3_ledger_cli_pins.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --test p6_retired_run_commands --test p6_retired_worker_commands --test w3_ledger_cli_pins --locked && cargo fmt --all --check`
**Own:** `crates/horch/tests/p6_retired_run_commands.rs`, `crates/horch/tests/p6_retired_worker_commands.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 37-50).

## Goal

Fix the 13 failing tests in `crates/horch/tests/p6_retired_run_commands.rs` (1 test), `crates/horch/tests/p6_retired_worker_commands.rs` (2 tests), and `crates/horch/tests/w3_ledger_cli_pins.rs` (10 tests) by updating the Phase 6 retired-command pins and migrating `w3_ledger_cli_pins.rs` from `horch ledger run new` / `horch ledger add` to direct ledger store seeding (or deleting tests of behaviors that Phase 6 removed on purpose).

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch/tests/p6_retired_run_commands.rs`, `crates/horch/tests/p6_retired_worker_commands.rs`, and `crates/horch/tests/w3_ledger_cli_pins.rs`.

## Tests covered (13 tests: 12 class `f` + 1 class `f?`)

1. `crates/horch/tests/p6_retired_run_commands.rs` (1 class `f` test):
   - `no_code_path_creates_a_task_without_an_idea` (`crates/horch/tests/p6_retired_run_commands.rs:414`): `a code path writes the idea of a run for its task: ["horch/src/cmd/worker.rs", "horch-e2e/src/ledger_seed.rs"]`
2. `crates/horch/tests/p6_retired_worker_commands.rs` (2 tests: 1 class `f` + 1 class `f?`):
   - `kind_catalog_is_the_table_of_the_design_and_the_kept_kinds` (`crates/horch/tests/p6_retired_worker_commands.rs:375`, class `f`): `assert_eq failed: Kind::ALL`
   - `fleetd_review_spawn_does_not_use_the_spawn_kind` (`crates/horch/tests/p6_retired_worker_commands.rs:594`, class `f?` on macOS): `the spawn wrote the task files of its attempt`
3. `crates/horch/tests/w3_ledger_cli_pins.rs` (10 class `f` tests, also in `red-baseline.txt`):
   - `a_command_with_no_fleetd_row_starts_no_watchdog` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea`
   - `add_map_prints_each_task_and_the_path_of_the_map_it_read` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `add_refuses_a_check_timeout_of_zero_after_it_queued_the_task` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `add_takes_the_title_check_and_files_from_the_ticket_unless_a_flag_names_them` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea`
   - `run_new_creates_the_branch_and_reports_it` (`crates/horch/tests/w3_ledger_cli_pins.rs:158`): `assert_eq failed: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `the_board_commands_print_their_pinned_text` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1", "--steps", "check,land"]: horch ledger run new is retired. Add an idea: horch ide`
   - `the_unfinished_import_and_refusal_arms_print_their_pinned_text` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
   - `watch_once_json_lists_each_checked_worker` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10", "--stall-rounds", "1000"]: horch ledger run new is retired`
   - `watch_once_reports_in_text_and_json` (`crates/horch/tests/w3_ledger_cli_pins.rs:138`): `assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea`

## Steps

1. In `crates/horch/tests/p6_retired_run_commands.rs:414` (`no_code_path_creates_a_task_without_an_idea`), inspect the allowlist of test/fixture paths that call `schema::run_idea`: `crates/horch-e2e/src/ledger_seed.rs` is the Phase 6 e2e test fixture helper (`git show 7532a60d`), and check why `horch/src/cmd/worker.rs` matched. Update the test's fixture exclusion list for `crates/horch-e2e/src/ledger_seed.rs` (and any test-only helper in `worker.rs`), without weakening the production code check that no production path creates a task without an idea.
2. In `crates/horch/tests/p6_retired_worker_commands.rs`:
   - At line 375 (`kind_catalog_is_the_table_of_the_design_and_the_kept_kinds`), check why `Kind::ALL` differed (for example, after T-899 / Phase 6 request kind updates) and align the table pin with the design and kept request kinds.
   - At line 548 (`fleetd_review_spawn_does_not_use_the_spawn_kind`), run it 3 times through `scripts/dev-sandbox.sh`, find why `horch spawn opus-reviewer --run r1 --exact` did not write the task files of the attempt on macOS (for example missing herdr fake or missing ticket/worktree/operator token in `World::down()`), and fix the fixture in `p6_retired_worker_commands.rs`.
3. In `crates/horch/tests/w3_ledger_cli_pins.rs`:
   - Migrate `a_command_with_no_fleetd_row_starts_no_watchdog`, `ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger`, `the_board_commands_print_their_pinned_text`, `the_unfinished_import_and_refusal_arms_print_their_pinned_text`, `watch_once_json_lists_each_checked_worker`, and `watch_once_reports_in_text_and_json` to seed their run and tasks through `Store::open` (`runs::create`, `schema::run_idea`, `tasks::add`) and provide the operator token file where the board command connects to `TestFleetd`.
   - For `run_new_creates_the_branch_and_reports_it`, `add_map_prints_each_task_and_the_path_of_the_map_it_read`, `add_refuses_a_check_timeout_of_zero_after_it_queued_the_task`, and `add_takes_the_title_check_and_files_from_the_ticket_unless_a_flag_names_them`, which test the CLI subcommands `horch ledger run new`, `horch ledger add`, and `horch ledger add-map` that Phase 6 retired on purpose (and whose retirement refusal text is tested in `p6_retired_run_commands.rs`), delete those 4 tests and write the reason and design rule (`FR-12`, Phase 6 ticket 47) in the commit message. Never weaken an assert.
4. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch --test p6_retired_run_commands --test p6_retired_worker_commands --test w3_ledger_cli_pins --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `p6_retired_run_commands.rs`, `p6_retired_worker_commands.rs`, and `w3_ledger_cli_pins.rs` pass (and `fleetd_review_spawn_does_not_use_the_spawn_kind` passes 3 times in a row), and the Check passes.

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
