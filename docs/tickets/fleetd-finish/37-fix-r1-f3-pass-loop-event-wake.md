# 37: Fix R1-F3: wake the fleetd pass loop on file events, request arrival, and job completion (RR-4)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --bin horch cmd::fleetd && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_pass && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/tests/p1_pass.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (finding R1-F3) and `docs/specs/fleetd/design.md` (rule RR-4).

## Goal

`horch fleetd`'s main loop wakes promptly to run a reconciliation pass when a watched file changes (such as `result.json` in an attempt outbox, a request file in `<fleet root>/run/requests`, or a job result file in `<fleet root>/run/jobs`), when a request arrives on the command channel, or when a launched child/job completes, with `pass_period_s` as the fixed-period fallback deadline.

## Requirements

- **R1-F3 (Important — RR-4)**:
  - **Finding text and file:line**:
    `crates/horch/src/cmd/fleetd.rs:1162` (`fn run_loop`) runs a pass, then sleeps until `pass_period_s` (`sleep_unless_term` at `:569` and `:1181`); only `TERM` shortens the sleep. No file watcher, request arrival, or job completion wakes this loop. A concrete case is a five-second period where `result.json` arrives immediately after a pass. Even though RR-4 requires reconciliation on file events as well as on a fixed period, the result waits almost five seconds. The same delay applies to an operator request file and an ended job. The test `rr4_tasks_complete_with_all_file_watch_events_dropped` proves periodic recovery from missed events, but it does not prove that a delivered event wakes a pass.
  - **Design rule**:
    - **RR-4** (`docs/specs/fleetd/design.md:157`): "The supervisor shall reconcile desired and actual state on a fixed period as well as on file events, so a missed event is caught by the next pass."

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

## Steps

1. Write the failing test first in `crates/horch-e2e/tests/p1_pass.rs` (and a unit test on `run_loop` in `crates/horch/src/cmd/fleetd.rs`):
   - In `crates/horch-e2e/tests/p1_pass.rs`, add test function `rr4_file_request_and_job_events_wake_pass_before_period`:
     - Start `horch fleetd` with a long `pass_period_s` in `fleet.toml` (for example `pass_period_s = 30`).
     - Wait for the initial pass heartbeat, then trigger:
       1. A request spool file written to `<fleet root>/run/requests/` (or a request arriving while fleetd is between periodic passes).
       2. A worker outbox event (`result.json` written in an active attempt's outbox folder).
       3. A running job completion (writing `<fleet root>/run/jobs/<id>-<run>.json` and exiting).
     - Assert that each event wakes fleetd and starts a pass that ingests the event well before the 30-second `pass_period_s` elapses (for example within 3 seconds).
   - Check: build workspace binaries (`scripts/dev-sandbox.sh cargo build --workspace --bins`) and run `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p1_pass -- rr4_file_request_and_job_events_wake_pass_before_period` to see it fail by waiting for the 30-second period.
2. In `crates/horch/src/cmd/fleetd.rs`:
   - Give `run_loop` (at line 1166) and `sleep_unless_term` (at line 569) a wake condition / predicate (`woke: &mut dyn FnMut() -> bool` or a shared wake handle) in addition to `TERM` and the `period` fallback deadline.
   - Feed the wake check in `daemon` (`crates/horch/src/cmd/fleetd.rs:630-640`) from:
     - An OS `horch_platform::watch::Watcher` watching `<fleet root>/run/requests`, `<fleet root>/run/jobs`, `<fleet root>/run/attempts`, and the active attempt outbox / worktree folders (when `!ctx.settings.faults.has(DROP_WATCH_EVENTS)`).
     - Channel request activity or ledger/spool updates and active child/job completion checks during the wait interval.
   - Ensure that when `DROP_WATCH_EVENTS` fault injection is set, watcher-triggered wakes are suppressed so `rr4_tasks_complete_with_all_file_watch_events_dropped` still tests pure periodic recovery.
3. Rebuild the workspace binaries (`scripts/dev-sandbox.sh cargo build --workspace --bins`) and run the ticket Check.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Changing the platform watcher implementations in `crates/horch-platform/`.

## Done when

`rr4_file_request_and_job_events_wake_pass_before_period` in `crates/horch-e2e/tests/p1_pass.rs` passes, `rr4_tasks_complete_with_all_file_watch_events_dropped` continues to pass, and the ticket Check passes.
