<!-- horch-ticket: 1 -->
# Fix the red tests of `crates/horch-e2e/tests/e2e.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test e2e --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/e2e.rs`

## Goal

Make the 12 red tests of `crates/horch-e2e/tests/e2e.rs` from `docs/specs/fleetd/gates/baseline-2026-11.md` pass without weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-e2e`, `tests/e2e.rs`).
- `crates/horch-e2e/tests/e2e.rs`.
- `crates/horch-e2e/tests/fleet_start.rs` (`a_fleet_starts_no_orchestrator_pane`, showing how `horch fleet` behaves after Phase 6 `FM-16`).
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.

## Failing tests and their first error line from the baseline

1. `bal_04_exit_code_3` — `assertion 'left == right' failed: expected exit code 3 for exhausted account, got 1 (stderr: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
2. `bal_04_gate_before_side_effects` — `assertion 'left == right' failed`
3. `bal_05_resume_uses_via` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
4. `bal_05_substituted_record_fields` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
5. `bal_06_route_matches_gate` — `assertion 'left == right' failed`
6. `bal_08_fleet_auto_choice_all_exhausted` — `expected exit code 3 when all candidates exhausted, got 0 (stderr: )`
7. `bal_08_fleet_auto_choice_all_ok` — `called 'Option::unwrap()' on a 'None' value`
8. `bal_08_fleet_auto_choice_claude_exhausted_codex_ok` — `called 'Option::unwrap()' on a 'None' value`
9. `bal_08_fleet_auto_choice_claude_unknown_codex_ok` — `called 'Option::unwrap()' on a 'None' value`
10. `tel_02_fleet_records_the_orchestrators_plugin_skills` — `waiting for the orchestrator's claude`
11. `tel_02_fleet_writes_orchestrator_record` — `assertion 'left == right' failed`
12. `tel_02_resume_refuses_orchestrator` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file`:** the test needs a fleet root with a token; use the harness helper `h.serve_fleetd()` (with `HORCH_LEDGER_DB` set to `h.root.join("ledger.sqlite3")`) that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs`).
- **A behavior that Phase 6 removed on purpose:**
  - Tests 1 to 5 and 12 (`bal_04_exit_code_3`, `bal_04_gate_before_side_effects`, `bal_05_resume_uses_via`, `bal_05_substituted_record_fields`, `bal_06_route_matches_gate`, `tel_02_resume_refuses_orchestrator`) call `horch spawn`, which Phase 6 retired on purpose under `FR-5` (ticket `47-c6-t908-retire-worker-run-cli.md`).
  - Tests 6 to 11 (`bal_08_fleet_auto_choice_*`, `tel_02_fleet_records_the_orchestrators_plugin_skills`, `tel_02_fleet_writes_orchestrator_record`) test `horch fleet` launching an orchestrator pane and writing an orchestrator session record, which Phase 6 removed on purpose under `FM-16` ("No orchestrator seat"; see `a_fleet_starts_no_orchestrator_pane` in `crates/horch-e2e/tests/fleet_start.rs`).
  - Do **not** delete these tests on your own decision; send `QUESTION:` to the orchestrator with the design rules (`FR-5` and `FM-16`) and a recommended option (for example whether to migrate the routing/substitution checks to `horch route` / `fleetd` dispatch and retire the `horch spawn` / orchestrator-pane-launch tests).
- **A time-out or a result that changes between runs:** the baseline ran at a load average of 16 to 26; run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert. Do not edit product code; product code is out of scope for this ticket.

## Done when

Each of the 12 tests passes 3 times in a row (or has an orchestrator decision on a `QUESTION:` for tests of behaviors removed in Phase 6), and the Check passes.

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
