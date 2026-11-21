# CI FIX-8: migrate stale tests in horch-e2e done_budget.rs, fakes.rs, fleet18_brief_worktree.rs and fleet18_messaging.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test done_budget --test fakes --test fleet18_brief_worktree --test fleet18_messaging --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/done_budget.rs`, `crates/horch-e2e/tests/fakes.rs`, `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, `crates/horch-e2e/tests/fleet18_messaging.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-8) and `ai_docs/plans/fleetd-build/red-baseline.txt` (rows 54-69).

## Goal

Migrate the retired `horch ledger run new` / `horch ledger add` setup and fix the missing `schema::run_idea` (`FR-12`) and skill budget assertions in `crates/horch-e2e/tests/done_budget.rs`, `crates/horch-e2e/tests/fakes.rs`, `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, and `crates/horch-e2e/tests/fleet18_messaging.rs`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-8.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- The 4 test files in `**Own:**`.

## Tests covered (15 tests across 4 files)

1. `crates/horch-e2e/tests/done_budget.rs` (2 rows of `red-baseline.txt`):
   - `ledger_done_finishes_in_time_against_a_stuck_orchestrator` (`crates/horch-e2e/tests/done_budget.rs:214`): `status: exit status: 1 stdout:` (calls `["ledger", "run", "new", ...]` and `["ledger", "add", ...]`)
   - `ledger_done_killed_during_the_wake_push_has_recorded_the_report` (`crates/horch-e2e/tests/done_budget.rs:214`): `status: exit status: 1 stdout:` (calls `["ledger", "run", "new", ...]` and `["ledger", "add", ...]`)
2. `crates/horch-e2e/tests/fakes.rs` (4 rows of `red-baseline.txt`):
   - `phase4::ask_scenario_sends_one_question` (`crates/horch-e2e/tests/fakes.rs`): `horch ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `phase4::bound_scenario_writes_entry_json_at_entry` (`crates/horch-e2e/tests/fakes.rs`): `horch ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `phase4::noresult_scenario_writes_nothing` (`crates/horch-e2e/tests/fakes.rs`): `horch ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
   - `phase4::result_scenario_writes_result_json` (`crates/horch-e2e/tests/fakes.rs`): `horch ["ledger", "run", "new", "r1"]: horch: the task ledger <tmp> does not exist. fleetd creates it at its start: run horch fleetd check`
3. `crates/horch-e2e/tests/fleet18_brief_worktree.rs` (5 rows of `red-baseline.txt` + 1 class `f?` row of the CI report):
   - `g69_drop_keep_branch_refuses_detached_unique_commits` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:103`): `["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: the task ledger <tmp> does not exist.`
   - `g69_drop_refuses_changed_checked_out_branch` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:103`): `["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: the task ledger <tmp> does not exist.`
   - `g69_drop_refuses_detached_unique_commits` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:103`): `["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: the task ledger <tmp> does not exist.`
   - `g69_from_scratch_and_drop_preserve_unpublished_work` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:103`): `["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: the task ledger <tmp> does not exist.`
   - `g69_open_scratch_does_not_block_finish_or_done` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:103`): `["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: the task ledger <tmp> does not exist.`
   - `g64_skill_states_each_delegation_and_reporting_rule_within_budget` (`crates/horch-e2e/tests/fleet18_brief_worktree.rs:391`, class `f?`): `missing verified (with evidence) or not verified`
4. `crates/horch-e2e/tests/fleet18_messaging.rs` (3 class `f` rows of the CI report; note that after T-11 lands T-897, `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart` is removed by T-897, leaving the `ledger_worker()` helper at line 311 used by `g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap` and `g70_waiting_background_deadline_and_dead_process`):
   - `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart` (`crates/horch-e2e/tests/fleet18_messaging.rs:311`): `Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
   - `g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap` (`crates/horch-e2e/tests/fleet18_messaging.rs:311`): `Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`
   - `g70_waiting_background_deadline_and_dead_process` (`crates/horch-e2e/tests/fleet18_messaging.rs:311`): `Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add`

## Steps

1. In `crates/horch-e2e/tests/done_budget.rs`, `crates/horch-e2e/tests/fakes.rs` (`mod phase4`), and `crates/horch-e2e/tests/fleet18_brief_worktree.rs` (`ledger_world` at line 94), migrate each `horch ledger run new` and `horch ledger add` call to `horch_e2e::ledger_seed::{seed_run, seed_task}`.
2. In `crates/horch-e2e/tests/fleet18_brief_worktree.rs:371` (`g64_skill_states_each_delegation_and_reporting_rule_within_budget`), run it 3 times and compare the required strings against `skills/orchestrate/SKILL.md` (which Phase 6 updated when retiring `horch ledger run gate` and the old orchestrator skill wording); update `g64_skill_states_each_delegation_and_reporting_rule_within_budget` to match the Phase 6 `skills/orchestrate/SKILL.md` text without weakening the budget check.
3. In `crates/horch-e2e/tests/fleet18_messaging.rs:262-320` (`ledger_worker`), call `horch_core::taskledger::schema::run_idea(tx, &run.run_id, &horch_core::taskledger::util::now(), true)?` after `runs::create` before `tasks::add` so run `waits` has an idea in flight (`FR-12`).
4. Build the workspace binaries and run the Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test done_budget --test fakes --test fleet18_brief_worktree --test fleet18_messaging --locked && cargo fmt --all --check`

## Out of scope

- Any file outside `**Own:**`.

## Done when

All tests in `done_budget.rs`, `fakes.rs`, `fleet18_brief_worktree.rs`, and `fleet18_messaging.rs` pass, and the Check passes.

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


## Added 2026-11-20: more red tests of the local baseline in your Own files

These tests are red in `docs/specs/fleetd/gates/baseline-2026-11.md` and are part of this ticket. The ticket is done only when each test of each Own test file passes. Rule: a test of a behavior that Phase 6 retired by design gets its assert moved to the designed new effect; delete it only when no form of the rule exists, and name the test, the retired behavior and its rule in the commit message. No permanent expected-failures entry.

- `crates/horch-e2e/tests/done_budget.rs`:
  - `done_finishes_in_time_against_a_stuck_orchestrator` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `done_retried_after_a_kill_during_the_push_records_one_done` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `done_returns_in_time_when_herdr_hangs_on_pane_get` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/fleet18_brief_worktree.rs`:
  - `g56_facts_reach_the_launch_context_and_are_project_scoped` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g56_later_spawn_receives_recorded_fact` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g57_compacting_orchestrator_types_its_command_card` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `crates/horch-e2e/tests/fleet18_messaging.rs`:
  - `a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g60_codex_queue_confirmation_stays_pending_until_inbox` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g60_plain_done_and_tell_without_registration_arrive_after_late_registration` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_long_multiline_questions_and_blocked_survive_transport_ack_and_reply` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_question_reply_resets_stalls` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_sixty_minute_review_wait_and_normal_reap_after_completion` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_wait_expires_and_dead_agent_is_immediate` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## Added 2026-11-20: the unit T-897 lands at the same time

Task T-188 lands the branch of T-897 (worker question in a push attempt) while you work. It removes 1 test of `fleet18_messaging.rs` and changes `crates/horch-e2e/src/agent.rs` and the question ingest of fleetd. Before your last Check run: `git fetch` and `git rebase fleet/fleetd-finish`; if T-188 is not done then, wait for it (`horch ledger status`) and rebase after. Resolve a conflict with no change of an assert. Do not fix a test that T-897 removed.
