# 48: Fix the 8 tests that fail on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --test fleet18_observability --test i308_triage --test taskledger && cargo test -p horch --test w3_ledger_cli_pins`
**Own:** `crates/horch-e2e/tests/fleet18_observability.rs`, `crates/horch-e2e/tests/i308_triage.rs`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 27.
Prior work: report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/27.md`, section "Verified".
Waits for: 27.

## Goal

The final gate (ticket 33) runs every test. On `fleet/trace-fixes` at `2ce6cbeb` and at `ed3c51a8`, these 8 tests fail:

- `fleet18_observability::g52_concurrent_workers_get_their_own_session_and_transcript` (spawn refused: 0 startable tasks)
- `i308_triage::run_close_refuses_while_a_parked_worktree_has_no_decision` (ticket 13 was told to fix it; it still fails)
- `taskledger::a_closed_run_frees_the_project_for_the_next_run`
- `taskledger::a_spawn_joins_the_run_of_its_own_workspace`
- `taskledger::run_new_refuses_an_old_run_branch_behind_the_main_branch`
- `taskledger::spawn_run_names_the_run_the_worker_joins`
- `taskledger::the_watchdog_of_a_closed_run_exits`
- `w3_ledger_cli_pins::the_board_commands_print_their_pinned_text`

Find the root cause of each failure and fix the cause, not the symptom.

## Steps

1. Run the 8 tests on the run-branch tip. Record each failure message in the report.
2. Find which landed ticket changed each behaviour: `git log --oneline fleet/trace-fixes -- <file>` and the test output. Likely causes: ticket 19 (a task-less spawn refuses with no startable task), ticket 11 (`ledger status` exits 1 for an unsettled run), ticket 13 (run gate before close), ticket 20 (run join rule), ticket 27 (no task text).
3. Decide per test: the test pins old behaviour that the spec changed (update the test to the new rule and keep what it proves), or the change has a bug (fix the code). A code fix outside Own needs approval: send the orchestrator 1 QUESTION with the file and the line.
4. Never delete a test to make it pass. Never mark one `#[ignore]`.
5. Run the Check. Then run `cargo test -p horch-e2e` and `cargo test -p horch` in full, and name every other failure in the report.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes, and the report names the root cause of each of the 8 failures.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/48.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
