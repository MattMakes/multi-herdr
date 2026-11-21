<!-- horch-ticket: 1 -->
# Fix the red tests of `crates/horch-e2e/tests/messaging.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test messaging --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/messaging.rs`

## Goal

Make the 11 red tests of `crates/horch-e2e/tests/messaging.rs` pass without weakening any assert. (Note: `docs/specs/fleetd/gates/baseline-2026-11.md` lists `tests/messaging.rs` under the `horch-core` heading because both crates have a `tests/messaging.rs` target name, but all 11 failing tests live in `crates/horch-e2e/tests/messaging.rs`.)

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (listed under `horch-core`, `tests/messaging.rs`).
- `crates/horch-e2e/tests/messaging.rs`.
- `crates/horch-e2e/tests/fleet18_messaging.rs` (`g60_missing_registration_still_queues_worker_mail`, a green test that uses `h.serve_fleetd()`).
- `crates/horch-e2e/tests/fleet_start.rs` (`a_fleet_starts_no_orchestrator_pane`).

## Failing tests and their first error line from the baseline

1. `an_old_registry_others_can_write_is_not_copied_and_inbox_says_why` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
2. `done_outside_a_git_work_tree_reports_no_scan_error` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
3. `g5_done_without_an_orchestrator` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
4. `g5_resume_after_a_closed_pane` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
5. `mail_u1_done_push_failure_records_queued_and_closes` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
6. `mail_u1_done_stays_open_when_enqueue_fails` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
7. `p09_registry_in_the_old_temp_location_still_resolves` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
8. `p09_registry_lives_in_the_state_folder` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
9. `t11_followup_done_warns_when_the_registration_stays` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
10. `t11_followup_orchestrator_registers_under_a_custom_state_dir` — `horch fleet ran a pane-launch command`
11. `w2_agent_exit_before_done_tells_the_orchestrator` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file`:** the test needs a fleet root with a token; use the harness helper `h.serve_fleetd()` (with `HORCH_LEDGER_DB` set to `h.root.join("ledger.sqlite3")`) that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs` or `world` in `crates/horch-e2e/tests/taskledger.rs`).
- **A behavior that Phase 6 removed on purpose:**
  - Tests 1 to 9 and 11 fail in the test helper `running_worker(&h)` -> `spawn(&h, &["codex-sol"])` because `horch spawn` was retired in Phase 6 (`FR-5`, ticket `47-c6-t908-retire-worker-run-cli.md`). Where the test itself is about `horch done`, `horch inbox`, registry resolution, or `horch worker` exit notification (and only used `spawn(&h, ...)` as fixture setup to start a worker pane and register its session), replace the `spawn(&h, ...)` fixture setup with `h.serve_fleetd()` + a registered worker session / `horch worker` launch. Where a test (`g5_resume_after_a_closed_pane`) tests a retired `horch resume` / `horch spawn` CLI path removed in Phase 6, do **not** delete the test on your own decision; send `QUESTION:` to the orchestrator with the design rule (`FR-5`).
  - Test 10 (`t11_followup_orchestrator_registers_under_a_custom_state_dir`) calls `horch fleet --no-update` and expects `horch fleet` to run an orchestrator pane-launch command, which Phase 6 removed on purpose under `FM-16` ("No orchestrator seat"; see `a_fleet_starts_no_orchestrator_pane` in `crates/horch-e2e/tests/fleet_start.rs`). Do **not** delete the test on your own decision; send `QUESTION:` to the orchestrator with the design rule (`FM-16`).
- **A time-out or a result that changes between runs:** the baseline ran at a load average of 16 to 26; run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert. Do not edit product code; product code is out of scope for this ticket.

## Done when

Each of the 11 tests passes 3 times in a row (or has an orchestrator decision on a `QUESTION:` for tests of behaviors removed in Phase 6), and the Check passes.

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
