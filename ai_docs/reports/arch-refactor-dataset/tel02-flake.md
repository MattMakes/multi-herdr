# U34 tel02-flake report

## Cause
- Leak: `stop_collector(&h)` ran only at the end of `tel_02_fleet_writes_orchestrator_record`.
  A failing assert skipped it, so `horch telemetry --state-dir <tmp>` stayed alive.
- Flake: the test read the ledger once, with `unwrap()` on the read and the parse.
  The ledger is replaced atomically, so a read can see a missing or partial file.
- Product order is correct: `crates/horch/src/cmd/recipes.rs` inserts the orchestrator record before `pane_run`.

## Fix (only `crates/horch-e2e/tests/e2e.rs`)
- `stop_collector_in(&Path)` never panics. `stop_collector(&Harness)` stays as a wrapper.
- `CollectorGuard` stops the collector in `Drop`, on every exit path.
- The test now reads and parses the ledger inside `wait_for`, and retries on each poll.

## Results
- I could not reproduce the flake. Baseline: 30 of 30 passes, with `cargo test -p horch-e2e` looping in parallel.
- After the fix: 30 of 30 passes under the same load. Leaked collectors: 0.
- Forced failure (not committed): `pgrep -f 'horch telemetry --state-dir'` found 0 processes afterwards.
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: green.

## Gotchas
- `cargo build --workspace --bins` must run first in a fresh worktree. Otherwise every e2e test fails with "horch is not built".
- `crates/horch-e2e/src/lib.rs` is unchanged: the guard lives in the test file.
