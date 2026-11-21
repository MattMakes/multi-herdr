<!-- horch-ticket: 1 -->
# Fix the red tests of `p5_read_only_worker.rs` and `p5_release.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p5_read_only_worker --test p5_release --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p5_read_only_worker.rs, crates/horch-e2e/tests/p5_release.rs`

## Goal

Make the 6 red tests across `crates/horch-e2e/tests/p5_read_only_worker.rs` (3 tests) and `crates/horch-e2e/tests/p5_release.rs` (3 tests) from `docs/specs/fleetd/gates/baseline-2026-11.md` pass without weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-e2e`, `tests/p5_read_only_worker.rs` and `tests/p5_release.rs`).
- `crates/horch-e2e/tests/p5_read_only_worker.rs` and `crates/horch-e2e/tests/p5_release.rs`.
- `crates/horch-e2e/tests/p4_verifier.rs` (struct `World` field drop order: `fleetds: Fleetds` is declared before `h: Harness` so child `fleetd` processes stop before `Harness::drop` checks for leaked processes).

## Failing tests and their first error line from the baseline

### `crates/horch-e2e/tests/p5_read_only_worker.rs` (3 tests)

1. `fm17_codex_worker_needs_no_ledger_allow_rule` — `progress in the Codex sandbox: stderr=horch: unable to open database file: Error code 14: Unable to open database file`
2. `fm17_codex_worker_writes_its_result_in_the_task_folder` — `result in the Codex sandbox: stderr=horch: unable to open database file: Error code 14: Unable to open database file`
3. `worker_with_a_read_only_ledger_reports_progress_and_done_with_no_error` — `progress with a read-only ledger: stderr=horch: unable to open database file: Error code 14: Unable to open database file`

### `crates/horch-e2e/tests/p5_release.rs` (3 tests)

4. `old_claim_worker_runs_progress_and_done_after_the_update_with_1_effect_each` — `the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them:`
5. `old_push_worker_runs_progress_and_done_after_the_update_with_1_effect_each` — `the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them:`
6. `update_stops_no_run` — `the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them:`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file`:** the test needs a fleet root with a token; use the harness helper `h.serve_fleetd()` that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs`).
- **A behavior that Phase 6 removed on purpose:** do not delete the test on your own decision; send `QUESTION:` to the orchestrator with the design rule.
- **A time-out or a result that changes between runs / teardown process leak (`p5_release.rs` tests 4 to 6 and `p5_read_only_worker.rs` tests 1 to 3):**
  - The baseline ran at a load average of 16 to 26; run each test 3 times, find the cause, fix it; a flaky test is a bug.
  - In `crates/horch-e2e/tests/p5_release.rs`, inspect `struct Flight` (lines 71 to 77): `h: Harness` is declared **before** `fleetds: Fleetds`, `claim: OldWorker`, `push: OldWorker`, and `old_fleetd: OldFleetd`. Rust drops struct fields in declaration order, so `Harness::drop` runs first and checks for leaked processes while `fleetds`, `claim`, `push`, and `old_fleetd` are still alive (or only stops `old_fleetd` if `stop_old_fleetd` was called, while `fleetds` and the worker children are still running). Declare `fleetds`, `claim`, `push`, and `old_fleetd` **before** `h: Harness` (as `World` in `crates/horch-e2e/tests/p4_verifier.rs` does) and ensure `Drop` stops all child processes before `h` drops.
  - In `crates/horch-e2e/tests/p5_read_only_worker.rs`, inspect `World::new` and `start_fleetd`: `w.start_fleetd()` spawns `horch fleetd --foreground` asynchronously without waiting for `fleetd` to open the WAL database and bind its socket (unlike `w.fleetds.serve(&w.h, "fleetd")` in `p4_verifier.rs`), or `horch ledger progress` / `horch result` is invoked while SQLite WAL `-shm`/`-wal` files cannot be created in the read-only folder. Trace why `Error code 14: Unable to open database file` occurs in `World`, fix the test setup in `p5_read_only_worker.rs` (or if you find a product fault outside `Own`, report it to the orchestrator via `QUESTION:` before touching any file outside `Own`).
- Never weaken an assert. Product code is not in `Own`.

## Done when

Each of the 6 tests in `p5_read_only_worker.rs` and `p5_release.rs` passes 3 times in a row, and the Check passes.

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
