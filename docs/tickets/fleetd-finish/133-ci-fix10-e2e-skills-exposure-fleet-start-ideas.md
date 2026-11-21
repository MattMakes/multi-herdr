# CI FIX-10: fix unexplained failures in horch-e2e fleet_start.rs, p0_ideas.rs and skills_exposure.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet_start --test p0_ideas --test skills_exposure --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/fleet_start.rs`, `crates/horch-e2e/tests/p0_ideas.rs`, `crates/horch-e2e/tests/skills_exposure.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Fix units` row FIX-10) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 76, 91, 160-161).

## Goal

Run the 4 tests from `red-baseline.txt` in `crates/horch-e2e/tests/fleet_start.rs` (1 test), `crates/horch-e2e/tests/p0_ideas.rs` (1 test), and `crates/horch-e2e/tests/skills_exposure.rs` (2 tests) 3 times locally through `scripts/dev-sandbox.sh`, find the cause of any failure (such as unexpected `horch teammates --check` warnings on `stderr` in `skills_exposure.rs`), and fix it.

## Read first

- `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 76, 91, 160-161).
- `crates/horch-e2e/tests/fleet_start.rs`, `crates/horch-e2e/tests/p0_ideas.rs`, and `crates/horch-e2e/tests/skills_exposure.rs` (lines 504 to 660).

## Tests covered (4 rows of `red-baseline.txt`)

1. `crates/horch-e2e/tests/fleet_start.rs` (1 test):
   - `a_scratch_fleet_does_not_change_an_existing_default_ledger`: `horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
2. `crates/horch-e2e/tests/p0_ideas.rs` (1 test):
   - `fm8_two_ideas_sent_back_to_back_stay_2_ideas`: `horch idea add "Retry sync with backoff and log each retry": status: exit status: 1 stdout:`
3. `crates/horch-e2e/tests/skills_exposure.rs` (2 tests):
   - `operator_skills_e2e_check_warns_on_a_missing_name` (`crates/horch-e2e/tests/skills_exposure.rs:588`): `stdout:  stderr: warning: godot-csharp-engineer: the persona names the Task tool, which codex (fallback codex-sol) lacks`
   - `operator_skills_e2e_exposure_claude` (`crates/horch-e2e/tests/skills_exposure.rs:508`): `stdout:  stderr: warning: godot-csharp-engineer: the persona names the Task tool, which codex (fallback codex-sol) lacks`

## Steps

1. Build the workspace binaries and run each of the 4 tests 3 times through `scripts/dev-sandbox.sh`.
2. In `crates/horch-e2e/tests/skills_exposure.rs` (`operator_skills_e2e_exposure_claude` at line 508 and `operator_skills_e2e_check_warns_on_a_missing_name` at line 588), inspect why `h.run(&["teammates", "--check"])` failed or printed `warning: godot-csharp-engineer: the persona names the Task tool, which codex (fallback codex-sol) lacks` / missing operator skill warnings in the test fixture home, and fix `operator_world` or the assertion in `crates/horch-e2e/tests/skills_exposure.rs`.
3. In `crates/horch-e2e/tests/fleet_start.rs` and `crates/horch-e2e/tests/p0_ideas.rs`, verify that `a_scratch_fleet_does_not_change_an_existing_default_ledger` and `fm8_two_ideas_sent_back_to_back_stay_2_ideas` pass 3 times in a row without flake; fix any failure found inside `**Own:**`. Never weaken an assert.
4. Run the Check.

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `fleet_start.rs`, `p0_ideas.rs`, and `skills_exposure.rs` pass 3 times in a row, and the Check passes.

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

- `crates/horch-e2e/tests/fleet_start.rs`:
  - `a_scratch_fleet_warns_about_the_shared_ledger_and_does_not_create_it` — `timed out after 10s waiting for a live fleetd at /private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-p6-scratch-fleet-n2zX09/home/.horch/fleetd.sock`
- `crates/horch-e2e/tests/skills_exposure.rs`:
  - `git_env_cannot_reach_another_repository` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_changed_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_deleted_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_exposure_codex` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_ledger_record_lists_them` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `plugin_skills_e2e_changed_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `plugin_skills_e2e_ledger_record_lists_them` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_antigravity` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_claude` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_codex` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_opencode` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_pi` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_prime` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_marketplace_offline` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `worker_reads_the_skill_store_its_spawner_read` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### Additional notes on covered tests in `106` and `128`
- **Ticket `106` (`crates/horch-core/src/taskledger/stop.rs`):** `baseline-2026-11.md` lists `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_and_queues_nothing` and `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves_the_cancel_standing`, which commit `6260fc5c` (`T-893`) renamed to `trf_36_a_stop_ends_the_worker_by_its_pids_and_queues_nothing` and `trf_36_a_worker_that_cannot_be_stopped_returns_false`. Ticket `106` names the post-`T-893` test identifiers.
- **Ticket `128` (`crates/horch-core/tests/baseline_oracles.rs`):** `arc_09_argv_matches_baseline` is named in ticket `101-ci-fix2-linux-socat.md` (which only edits `.github/workflows/ci.yml`), but it also failed locally on macOS in `baseline-2026-11.md` with `oracle mismatch for claude/happy`. Ticket `128` already owns `crates/horch-core/tests/baseline_oracles.rs` and runs `cargo test -p horch-core --test baseline_oracles --locked`, so `128` will also run `arc_09_argv_matches_baseline`.
