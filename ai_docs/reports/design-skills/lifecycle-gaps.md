# D12 lifecycle-gaps: report

Unit: `lifecycle-gaps`. Branch: `ds/lifecycle-gaps`. Author: opus-32.

## Summary

- `horch done` succeeds when the pane is gone at any step after the summary is recorded.
- `horch done` gives Prime a final session discovery. The launch marker carries the Prime sessions dir.
- fake-herdr never gives out a pane id twice.
- Each gap has a test that fails without its fix. Each commit passed the full gate.

## Gap 1: `horch done` and a vanished pane

- Cause: `execution/lifecycle.rs` `done` ran `pane get` after `mark_done` with `?`. The dataset coordinator closes the pane of a candidate whose record is done. When it closed the pane before that `pane get`, `done` exited 1. D10 fixed only the final close.
- Fix: `mark_done` still runs first, and its error still fails `done` (nothing else is recorded yet). After it, a failed `pane get` means "already closed". `done` writes 1 line to stderr and goes on:
  - The workspace comes from `HORCH_WORKSPACE_ID`, else from `pane get`.
  - When the workspace is known, `record_session`, `unregister` and `settle` run. `settle` still helps, because the coordinator close also leaves a hole.
  - When the pane is gone and the workspace is unknown, those 3 steps are skipped. `done` returns success.
  - When the pane exists but has no workspace, `done` still fails ("could not resolve this pane's workspace").
  - The final close keeps the D10 rule: a failed close is success only when `pane get` shows the pane is gone.
- Test: `done_succeeds_when_the_pane_vanishes_before_any_step` (unit, `FakeWorkspace`). The pane vanishes before each of 7 steps: `mark_done`, report, `pane get`, `record_session`, `unregister`, `settle`, close. Each case runs with and without a request workspace (14 cases). Every case: the summary is recorded, `done` returns `Ok`, and the step count matches. The test fails on the old `done`.
- Trade-off: `done` cannot tell "pane gone" from "herdr unreachable" at `pane get`. Both now count as "closed" and exit 0. In both cases the ledger has the summary and no close is possible. The D10 close step already used `pane get` failure as "gone".

## Gap 2: Prime and the final discovery

- Cause: `launch::discover_now` called `discover_once` with `sessions_dir = None`. Prime finds its session only in the private `--session-dir` that `prepare` makes, so the `done` attempt never found it. When `horch done` ran before the first thread poll (0.5 s), the record had no session id.
- Fix (`harness/launch.rs` only): `start_discovery` writes the launch's `sessions_dir` into the launch marker. Before, the marker was empty. `discover_now` reads it back (`marker_sessions_dir`) and passes it to `discover_once`.
- Decision: the plan proposed a field on the execution or the brief. I used the launch marker:
  - The marker already exists for each launch, and only discovery reads it. `discover_now` already reads it for `since`.
  - The marker is per launch. A resume launch makes a new Prime dir, but a resume is never discovered.
  - No change to the brief, the ledger, `store.rs`, `brief.rs` or `messaging.rs`. No serialization change, no golden change.
  - Backward compatible: an empty marker from an older horch gives `None`, which is the old behavior.
- Tests:
  - `tel_session_recorded_when_prime_ends_fast` (e2e, `crates/horch-e2e/tests/lifecycle.rs`). It waits for fake-prime's session file, runs `horch done` at once, and expects the file path as `session_id`. With the fix removed it failed 4 of 5 runs (the thread sometimes wins). With the fix it passed 5 of 5 runs and in 2 full gates.
  - `launch_marker_carries_the_sessions_dir` (unit): round trip, an empty marker, and a missing marker.
- `fake-prime.rs` needed no change.

## Gap 3: fake-herdr pane ids

- Cause: `split_pane` took the first free `<workspace>:p<n>` among live panes. After a close, the next split got the closed id again. Real herdr ids are unique for each server.
- Fix: each workspace entry in the fake's state has `next_pane`. `workspace create` sets it to 2. `split_pane` uses it and adds 1. A state without the field (from an older fake) starts above its highest live id (`highest_pane`).
- Tests (unit tests in the bin, run by `cargo test --workspace`):
  - `pane_ids_are_never_reused_after_a_close`: split p2 and p3, close both, the next split is p4. The old code gives p2.
  - `a_state_without_a_counter_starts_above_the_live_ids`.
- Dependence check: no test in `crates/*/tests` or `crates/horch-e2e/src` names a split pane id. Only `w1:p1` (a root pane) appears. The gate is green with the change.

## Files changed

- `crates/horch-core/src/execution/lifecycle.rs` (Gap 1, unit test).
- `crates/horch-core/src/harness/launch.rs` (Gap 2, discovery only, unit test).
- `crates/horch-e2e/tests/lifecycle.rs` (Gap 2 e2e test).
- `crates/horch-e2e/src/bin/fake-herdr.rs` (Gap 3, unit tests).
- This report.

Not changed: `execution/store.rs`, `messaging/brief.rs`, `crates/horch/src/cmd/messaging.rs`, `fake-prime.rs`. No oracle or golden changed.

## Gotchas

- `cargo test -p horch-e2e` does not rebuild `horch`. Run `cargo build --workspace --bins` first.
- The Prime e2e test checks a race. Without the fix it can still pass when the discovery thread polls before `horch done`. It cannot fail falsely with the fix.

## Follow-ups (outside my scope)

- `done` cannot tell "pane gone" from "herdr down" at `pane get`. A reachability probe in `WorkspaceClient` could separate them. I did not add one, because a closed dataset workspace can make the probe fail too.
- The fsx breaker residual from D10 is still open.
