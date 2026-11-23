# CI FIX-8: migrate stale CLI tests in context_cli.rs, ledger_cli.rs and trf_11_add_map_cli.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --test context_cli --test ledger_cli --test trf_11_add_map_cli --locked && cargo fmt --all --check`
**Own:** `crates/horch/tests/context_cli.rs`, `crates/horch/tests/ledger_cli.rs`, `crates/horch/tests/trf_11_add_map_cli.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 11-12, 32-35).

## Goal

Fix the 8 class `f` CLI integration tests in `crates/horch/tests/context_cli.rs`, `crates/horch/tests/ledger_cli.rs`, and `crates/horch/tests/trf_11_add_map_cli.rs` that fail because `horch ledger run new` and `horch ledger add-map` (without `--dry-run`) were retired in Phase 6 and `horch note` requires an operator token or attempt context.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` (`seed_run` and `seed_task`).
- `crates/horch/tests/context_cli.rs`, `crates/horch/tests/ledger_cli.rs`, and `crates/horch/tests/trf_11_add_map_cli.rs`.

## Tests covered (8 class `f` rows)

1. `crates/horch/tests/context_cli.rs` (3 tests):
   - `ctx_01_cli_table_from_fixture_home` (`crates/horch/tests/context_cli.rs:129`): `horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc`
   - `ctx_21_compact_at_sets_base` (`crates/horch/tests/context_cli.rs:129`): `horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc`
   - `ctx_21_substituted_launch_takes_the_fallback_compact_at` (`crates/horch/tests/context_cli.rs:129`): `horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc`
2. `crates/horch/tests/ledger_cli.rs` (2 tests, also in `red-baseline.txt`):
   - `check_none_flag_on_add_stores_no_check` (`crates/horch/tests/ledger_cli.rs:63`):
     - CI first error line: `["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
     - `red-baseline.txt` first error line: `["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `check_none_in_ticket_on_add_without_flag_stores_no_check` (`crates/horch/tests/ledger_cli.rs:63`):
     - CI first error line: `["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
     - `red-baseline.txt` first error line: `["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
3. `crates/horch/tests/trf_11_add_map_cli.rs` (3 tests, also in `red-baseline.txt`):
   - `trf_11_dry_run_names_each_problem_and_writes_nothing` (`crates/horch/tests/trf_11_add_map_cli.rs:110`):
     - CI first error line: `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
     - `red-baseline.txt` first error line: `assertion left == right failed: ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `trf_13_add_map_refuses_a_ticket_already_queued_unless_again` (`crates/horch/tests/trf_11_add_map_cli.rs:110`):
     - CI first error line: `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
     - `red-baseline.txt` first error line: `assertion left == right failed: ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `trf_14_add_map_stores_kind_review_and_check_none` (`crates/horch/tests/trf_11_add_map_cli.rs:110`):
     - CI first error line: `assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>"`
     - `red-baseline.txt` first error line: `assertion left == right failed: ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`

## Steps

1. In `crates/horch/tests/context_cli.rs:129`, inspect how `World` runs `horch note` (or seeds session records) and provide the operator token file (`<fleet_root>/run/operator.token` with `0o600` permissions) or seed the session record directly through `ExecutionStore` so `ctx_01_cli_table_from_fixture_home`, `ctx_21_compact_at_sets_base`, and `ctx_21_substituted_launch_takes_the_fallback_compact_at` pass.
2. In `crates/horch/tests/ledger_cli.rs` and `crates/horch/tests/trf_11_add_map_cli.rs`:
   - For `trf_11_dry_run_names_each_problem_and_writes_nothing`, remove the `w.ok(&["ledger", "run", "new", "r1"])` call (or seed the run in the ledger store via `runs::create` + `schema::run_idea`), because `horch ledger add-map --dry-run` is kept in Phase 6 (`docs/specs/ticket-format.md` section 3).
   - For tests that call the retired `horch ledger add` or `horch ledger add-map` (without `--dry-run`), migrate them to `horch idea add --direct <ticket>` (or `horch_core::taskledger::map::add_map` / `tasks::add` if testing library storage of `Kind`, `Review`, and `Check: none`); if a test only tests a CLI subcommand that Phase 6 retired on purpose (`horch ledger add` `--check none` flag or `horch ledger add-map --again`), delete the test and write the reason and design rule (`FR-12`, Phase 6 ticket 47) in the commit message. Never weaken an assert.
3. Run the Check:
   `scripts/dev-sandbox.sh cargo test -p horch --test context_cli --test ledger_cli --test trf_11_add_map_cli --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `context_cli.rs`, `ledger_cli.rs`, and `trf_11_add_map_cli.rs` pass, and the Check passes.

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
