<!-- horch-ticket: 1 -->
# Fix the red tests of `channel.rs`, `fleet18_delivery_probe.rs`, and `relay_first_delivery.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test channel --test fleet18_delivery_probe --test relay_first_delivery --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/channel.rs, crates/horch-e2e/tests/fleet18_delivery_probe.rs, crates/horch-e2e/tests/relay_first_delivery.rs`

## Goal

Make the 8 red tests across `crates/horch-e2e/tests/channel.rs` (4 tests, listed under `horch-platform` in `baseline-2026-11.md`), `crates/horch-e2e/tests/fleet18_delivery_probe.rs` (3 tests), and `crates/horch-e2e/tests/relay_first_delivery.rs` (1 test) pass without weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-platform` `tests/channel.rs`, and `horch-e2e` `tests/fleet18_delivery_probe.rs` and `tests/relay_first_delivery.rs`).
- `crates/horch-e2e/tests/channel.rs`, `crates/horch-e2e/tests/fleet18_delivery_probe.rs`, and `crates/horch-e2e/tests/relay_first_delivery.rs`.
- `crates/horch-e2e/tests/fleet18_messaging.rs` (`g60_missing_registration_still_queues_worker_mail`, a green test that sets `HORCH_LEDGER_DB` and calls `h.serve_fleetd()`).
- `crates/horch-e2e/tests/fleet_start.rs` (`a_fleet_starts_no_orchestrator_pane`).

## Failing tests and their first error line from the baseline

### `crates/horch-e2e/tests/channel.rs` (4 tests; listed under `horch-platform` in `baseline-2026-11.md`)

1. `arc_30_e2e_a_live_relay_that_fails_leaves_the_tell_for_the_courier` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
2. `arc_30_e2e_codex_relay_runs_codex_queue` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
3. `w2_e2e_done_reports_through_a_relay_without_a_registered_pane` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
4. `w2_e2e_refused_tell_without_a_pane_does_not_claim_no_orchestrator` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`

### `crates/horch-e2e/tests/fleet18_delivery_probe.rs` (3 tests)

5. `g61_fleet_confirms_only_after_orchestrator_runs_inbox` — `expected pending probe, got: horch 0.4.8`
6. `g61_fleet_probe_never_falls_back_to_separate_text_and_enter` — `probe must land in pane, got ""`
7. `g61_fleet_without_ack_warns_and_keeps_probe_pending` — `expected probe failure warning, got stdout=horch 0.4.8`

### `crates/horch-e2e/tests/relay_first_delivery.rs` (1 test)

8. `a_live_relay_gets_the_fleet_probe_and_the_idle_pane_gets_no_text` — `probe must not report failure: out=horch 0.4.8`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file` (`crates/horch-e2e/tests/channel.rs`, tests 1 to 4):** the test calls `SessionLedger::for_project(&h.project, &state_dir).unwrap().insert(&rec).unwrap()`, which now routes through `fleetd` and requires a fleet root with an operator token. Use the harness helper `h.serve_fleetd()` (with `HORCH_LEDGER_DB` set to `h.root.join("ledger.sqlite3")`) that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs` or `world` in `crates/horch-e2e/tests/taskledger.rs`) before calling `SessionLedger::insert` or running `horch done` / `horch tell`.
- **A behavior that Phase 6 removed on purpose (`fleet18_delivery_probe.rs` tests 5 to 7 and `relay_first_delivery.rs` test 8):** all 4 tests invoke `h.run(&["fleet", "--no-update"])` and expect `horch fleet` to launch or probe an orchestrator pane, whereas Phase 6 (`FM-16`, "No orchestrator seat"; see `a_fleet_starts_no_orchestrator_pane` in `crates/horch-e2e/tests/fleet_start.rs`) changed `horch fleet` to print the status banner (`horch 0.4.8` / `Add an idea: horch idea add "..."`) without launching or probing an orchestrator pane. Do **not** delete these tests on your own decision; send `QUESTION:` to the orchestrator with the design rule (`FM-16`) and a recommended option.
- **A time-out or a result that changes between runs:** the baseline ran at a load average of 16 to 26; run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert. Do not edit product code; product code is out of scope for this ticket.

## Done when

Each of the 4 tests in `channel.rs` passes 3 times in a row, the 4 tests in `fleet18_delivery_probe.rs` and `relay_first_delivery.rs` either pass 3 times in a row or have an orchestrator decision on a `QUESTION:` for `FM-16`, and the Check passes.

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
