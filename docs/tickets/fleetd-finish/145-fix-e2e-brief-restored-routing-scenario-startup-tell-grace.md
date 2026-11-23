<!-- horch-ticket: 1 -->
# Fix the red tests of `brief.rs`, `restored_pane.rs`, `routing_provenance.rs`, `scenario.rs`, `startup_error.rs`, and `tell_grace.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test brief --test restored_pane --test routing_provenance --test scenario --test startup_error --test tell_grace --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/brief.rs, crates/horch-e2e/tests/restored_pane.rs, crates/horch-e2e/tests/routing_provenance.rs, crates/horch-e2e/tests/scenario.rs, crates/horch-e2e/tests/startup_error.rs, crates/horch-e2e/tests/tell_grace.rs`

## Goal

Make the 9 red tests across `crates/horch-e2e/tests/brief.rs` (1), `crates/horch-e2e/tests/restored_pane.rs` (1), `crates/horch-e2e/tests/routing_provenance.rs` (1), `crates/horch-e2e/tests/scenario.rs` (2), `crates/horch-e2e/tests/startup_error.rs` (3), and `crates/horch-e2e/tests/tell_grace.rs` (1) from `docs/specs/fleetd/gates/baseline-2026-11.md` pass without weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-e2e`).
- `crates/horch-e2e/tests/brief.rs`, `crates/horch-e2e/tests/restored_pane.rs`, `crates/horch-e2e/tests/routing_provenance.rs`, `crates/horch-e2e/tests/scenario.rs`, `crates/horch-e2e/tests/startup_error.rs`, and `crates/horch-e2e/tests/tell_grace.rs`.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d`.
- `crates/horch-e2e/tests/fleet18_messaging.rs` (`g60_missing_registration_still_queues_worker_mail`, a green test that uses `h.serve_fleetd()`).

## Failing tests and their first error line from the baseline

### `crates/horch-e2e/tests/brief.rs` (1 test)

1. `arc_07_e2e_bin_overrides_reach_worker` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### `crates/horch-e2e/tests/restored_pane.rs` (1 test)

2. `a_restored_worker_pane_tells_as_its_role_peeks_its_inbox_and_finishes` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### `crates/horch-e2e/tests/routing_provenance.rs` (1 test)

3. `arc_14_provenance_on_spawn_substitute_resume` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### `crates/horch-e2e/tests/scenario.rs` (2 tests)

4. `the_hermetic_story` — `assertion 'left == right' failed`
5. `the_hermetic_story_with_an_ollama_that_starts_in_7_s` — `assertion 'left == right' failed`

### `crates/horch-e2e/tests/startup_error.rs` (3 tests)

6. `a_worker_that_registers_after_the_wait_still_works` — `assertion failed: out.status.success()`
7. `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` — ` stderr: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
8. `spawn_waits_for_a_worker_that_registers_and_prints_only_the_pane` — `stdout: `

### `crates/horch-e2e/tests/tell_grace.rs` (1 test)

9. `worker_done_reports_without_the_default_tell_grace` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file`:** the test needs a fleet root with a token; use the harness helper `h.serve_fleetd()` (with `HORCH_LEDGER_DB` set to `h.root.join("ledger.sqlite3")`) that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs` or `world` in `crates/horch-e2e/tests/taskledger.rs`).
- **A behavior that Phase 6 removed on purpose (`horch spawn` retired in Phase 6 under `FR-5`, ticket `47-c6-t908-retire-worker-run-cli.md`):**
  - All 9 tests fail because they invoke `horch spawn`.
  - Where `horch spawn` was only used as fixture setup to test a surviving feature (`a_restored_worker_pane_tells_as_its_role_peeks_its_inbox_and_finishes` in `restored_pane.rs`, `worker_done_reports_without_the_default_tell_grace` in `tell_grace.rs`, or `arc_07_e2e_bin_overrides_reach_worker` in `brief.rs`), migrate the fixture setup to `h.serve_fleetd()` + a registered worker session / `horch worker` launch.
  - Where the test asserts the CLI behavior of `horch spawn` itself (`startup_error.rs` tests 6 to 8, `routing_provenance.rs` test 3, and the `h.run(&["spawn", "opus", "--from-pane", "w1:p1"])` step at line 1161 of `scenario.rs` tests 4 and 5), do **not** delete the test or assert on your own decision; send `QUESTION:` to the orchestrator with the design rule (`FR-5`, Phase 6 ticket `47-c6-t908-retire-worker-run-cli.md`) and a recommended option.
- **A time-out or a result that changes between runs:** the baseline ran at a load average of 16 to 26; run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert. Do not edit product code; product code is out of scope for this ticket.

## Done when

Each of the 9 tests passes 3 times in a row (or has an orchestrator decision on a `QUESTION:` for tests of retired `horch spawn` behavior), and the Check passes.

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
