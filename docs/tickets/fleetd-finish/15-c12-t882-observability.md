# 15: C12 T-882: resume under push dispatch and migrate 4 retired horch ledger run new tests to ledger_seed

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability && cargo fmt --all --check`
**Own:** `crates/horch-e2e/expected-failures/fleet18_observability.txt`, `crates/horch-e2e/src/ledger_seed.rs`, `crates/horch-e2e/tests/fleet18_observability.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_cli.rs`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (section 1 items L7, L13; section 3 unit C12) and `ai_docs/plans/fleetd-finish/audit/unlanded-branches.md` (section "T-882 - closing cleanup and observability migration").

## Goal

Fix `horch spawn --resume` under push dispatch (`g52_resume_uses_own_session`), migrate the 4 tests in `crates/horch-e2e/tests/fleet18_observability.rs` that call the retired `horch ledger run new` command onto `crates/horch-e2e/src/ledger_seed.rs`, remove all 5 entries from `crates/horch-e2e/expected-failures/fleet18_observability.txt`, and run each of the 5 tests 3 times.

## Requirements

- **L7 / L13 (T-882)**:
  1. `g52_resume_uses_own_session` (`crates/horch-e2e/tests/fleet18_observability.rs:500`): under push dispatch, when a killed agent ends its worker row and the operator runs `horch spawn --resume <role>`, bind or resume the task with its prior session (or update the push-dispatch resume path and test setup so `horch spawn --resume` resumes the worker's own session without failing with `no ready task for persona codex-terra`).
  2. Migrate these 4 tests in `crates/horch-e2e/tests/fleet18_observability.rs` from the retired `horch ledger run new` CLI call to `crates/horch-e2e/src/ledger_seed.rs`:
     - `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` (line 106)
     - `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` (line 338, using the push dispatch flow)
     - `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` (line 727)
     - `g53_ledger_handoff_compacts_resumes_and_records_progress_once` (line 835)
  3. Remove all 5 entries from `crates/horch-e2e/expected-failures/fleet18_observability.txt` so that the file contains only comments.
  4. Run each of the 5 removed expected-failure tests 3 times (rule 7).

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

## Steps

1. Inspect `crates/horch-e2e/expected-failures/fleet18_observability.txt`, `crates/horch-e2e/src/ledger_seed.rs`, and the 5 tests in `crates/horch-e2e/tests/fleet18_observability.rs` (`:106`, `:338`, `:500`, `:727`, `:835`). Also inspect `git show 7532a60d` for how earlier observability tests moved to `ledger_seed`.
   - Check: identify each `run new` and `add-task` call in the 4 Phase 6 tests and the resume flow in `g52_resume_uses_own_session`.
2. Migrate `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run`, `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block`, `g53_ledger_handoff_compacts_resumes_and_records_progress_once`, and `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` in `crates/horch-e2e/tests/fleet18_observability.rs` to seed runs and tasks through `crates/horch-e2e/src/ledger_seed.rs`. Add any needed helper in `ledger_seed.rs`.
   - Check: `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability -- g51_sessions_points_to_ledger_status_only_for_the_callers_active_run g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block g53_ledger_handoff_compacts_resumes_and_records_progress_once ledger_stop_stops_a_spawned_worker_by_the_ingested_pid`
3. Fix the resume flow for `g52_resume_uses_own_session` in `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` / `handlers_cli.rs` and `crates/horch-e2e/tests/fleet18_observability.rs:500` so that `horch spawn --resume` under push dispatch binds the task and resumes the worker's own session.
   - Check: `scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability -- g52_resume_uses_own_session`
4. Remove all 5 entries from `crates/horch-e2e/expected-failures/fleet18_observability.txt`. Run the 5 tests 3 times (rule 7), then run the entire `fleet18_observability` binary:
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability -- g52_resume_uses_own_session g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block g51_sessions_points_to_ledger_status_only_for_the_callers_active_run g53_ledger_handoff_compacts_resumes_and_records_progress_once ledger_stop_stops_a_spawned_worker_by_the_ingested_pid || exit 1; done`
   `scripts/dev-sandbox.sh cargo test -p horch-e2e --test fleet18_observability`
   - Check: `grep -v "^#" crates/horch-e2e/expected-failures/fleet18_observability.txt` prints nothing, and every test in `fleet18_observability` passes.
5. Run `cargo fmt --all --check`.

## Out of scope

- Writing Phase 4, 5, 6, or 7 gate files (units E2, E3, E4, F6).

## Done when

`crates/horch-e2e/expected-failures/fleet18_observability.txt` has no active entry, all 5 tests pass 3 times in a row, and the Check passes.


## Added 2026-11-20: more red tests of the local baseline in your Own files

These tests are red in `docs/specs/fleetd/gates/baseline-2026-11.md` and are part of this ticket. The ticket is done only when each test of each Own test file passes. Rule: a test of a behavior that Phase 6 retired by design gets its assert moved to the designed new effect; delete it only when no form of the rule exists, and name the test, the retired behavior and its rule in the commit message. No permanent expected-failures entry.

- `g49_spawn_doctor_and_inbox_warn_once_and_name_installed_binary` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g50_legacy_orchestrator_and_host_load_are_advisory` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g51_pane_gone_context_filters_and_completed_summaries` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g51_sessions_filter_workspace_live_role_and_indent_summary` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g52_concurrent_workers_get_their_own_session_and_transcript` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g52_done_discovery_uses_the_same_launch_token` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_below_threshold_handoff_does_not_schedule` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_first_handoff_without_prior_warning_does_not_schedule` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_progress_warns_once_and_missing_handoff_is_refused` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## Added 2026-11-20: build of the whole workspace before the land

This run changed about 60 test files after the task branch was made. Before you send READY-TO-LAND: run `scripts/dev-sandbox.sh cargo test --workspace --locked --no-run` 1 time. If a test file outside your Own does not build because of your merge (for example a function that your branch removes), tell the orchestrator the file; it is then added to your Own. Do not land a unit that breaks the build of another test file.
