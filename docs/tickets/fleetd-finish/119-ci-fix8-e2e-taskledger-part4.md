# CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 4 (lines 3363 to end)

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows of `taskledger.rs`).

## Goal

Migrate the final range of tests in `crates/horch-e2e/tests/taskledger.rs` (from `a_live_worker_that_does_not_claim_is_reported_once` at line 3364 through `a_killed_detached_finish_is_reported_by_the_next_finish` at line 3865, including helpers `claim_and_reap`, `claim_and_commit`, and `pane_world`) from the retired `horch ledger run new` / `horch ledger add` CLI commands to `horch_e2e::ledger_seed`, and verify that the entire `taskledger` test binary passes.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch-e2e/tests/taskledger.rs` (lines 3363 to end).

## Tests covered (11 tests in lines 3363 to end of `crates/horch-e2e/tests/taskledger.rs`)

1. `a_live_worker_that_does_not_claim_is_reported_once` (line 3364): `horch ledger run new a --heartbeat-stale 100000: status: exit status: 1 stdout:`
2. `replacing_a_live_worker_names_the_stop_command` (line 3410): `horch ledger run new a: status: exit status: 1 stdout:`
3. `a_task_reaped_max_attempts_times_fails_and_its_replan_requeues_it` (line 3435): `horch ledger run new a --max-attempts 2: status: exit status: 1 stdout:`
4. `requeue_refuses_a_task_that_is_neither_held_nor_cancelled` (line 3506): `horch ledger run new a: status: exit status: 1 stdout:`
5. `a_cancel_names_the_tasks_that_wait_for_it` (line 3522): `horch ledger run new a: status: exit status: 1 stdout:`
6. `an_unlanded_done_names_its_dependents_and_a_stuck_run_wakes_once` (line 3558): `horch ledger run new a: status: exit status: 1 stdout:`
7. `a_stuck_run_wakes_from_the_watchdog_when_no_task_finishes` (line 3594): `horch ledger run new a: status: exit status: 1 stdout:`
8. `a_cancelled_task_does_not_land_or_turn_done` (line 3647): `horch ledger run new r2 --steps land,cleanup: status: exit status: 1 stdout:`
9. `a_cancel_during_the_land_lands_nothing` (line 3693): `horch ledger run new r2 --steps land,cleanup: status: exit status: 1 stdout:`
10. `a_detached_finish_returns_at_once_and_mails_the_result` (line 3822, via `pane_world` at line 3764): `horch ledger run new alpha --steps check: status: exit status: 1 stdout:`
11. `a_killed_detached_finish_is_reported_by_the_next_finish` (line 3865, via `pane_world` at line 3764): `horch ledger run new alpha --steps check: status: exit status: 1 stdout:`

## Steps

1. In `crates/horch-e2e/tests/taskledger.rs` (lines 3363 to end, including `claim_and_reap`, `claim_and_commit`, and `pane_world`), replace each `horch ledger run new` and `horch ledger add` CLI call with `horch_e2e::ledger_seed::{seed_run, seed_task}` (or `Store::open` with `runs::create`, `schema::run_idea`, and `tasks::add`).
2. If any test in this range tests a CLI behavior that Phase 6 removed on purpose (for example `replacing_a_live_worker_names_the_stop_command` on retired `horch spawn`), delete that test and write the reason and design rule (`FR-5`, Phase 6 ticket 47) in the commit message. Never weaken an assert.
3. Build the workspace binaries and run the full `taskledger` test suite:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `crates/horch-e2e/tests/taskledger.rs`.

## Done when

Every test in `crates/horch-e2e/tests/taskledger.rs` passes, and the Check passes.

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


## Changed 2026-11-20T18:30Z: read this first

1. The Check of this ticket changed. The ledger holds the valid Check: it builds the bins and runs the whole file `taskledger.rs`. Each test of the file must pass. Run the whole file 3 times and write the result (names, 3 of 3) in your DONE note. Cargo rule: only 1 filter goes before `--`; `--locked` goes before `--`.
2. Pattern: read the commit message of 9e1efb7d (part 1). It shows the seed helpers (`seed_run`, `seed_run_with`, `seed_task_with`), the push form of a finish test, and how a dropped assert is written. Do the same.
3. Rules for a test of a retired behavior: move the assert to the designed new effect. Delete a test only when no form of the rule exists, and name the test, the retired behavior and the design text in the commit message. For each dropped assert, name the design text that retires it; if you find none, send `QUESTION:`. Do not weaken an assert. No `#[ignore]`.
4. A test that fails each run because of a known product fault gets `expected::check` and 1 line in `crates/horch-e2e/expected-failures/taskledger.txt` that names the owner ticket (for example ticket 168, T-207: the tree of landed work is not removed). That file is in your Own. If no ticket owns the fault, send `QUESTION:` with the evidence.
5. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
6. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
