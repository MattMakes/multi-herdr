# p1_orphans: the operator-boundary probe of the fake agent leaves a request file of mode 0644 that a fleetd pass can read (2 tests fail by a race)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_orphans && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_orphans.rs`, `crates/horch-e2e/src/probe.rs`

## Fault

Found by task T-172 (opus-hardening-11) on 2026-11-20.

- `fm22_another_start_time_stops_nothing` (`p1_orphans.rs:333`) and `fm22_orphan_process_and_pane_after_a_reap_are_stopped` (`p1_orphans.rs:275`) fail: `open_items` is `[request_file_rejected:.../run/requests/horch-probe-<pid>.json]`, expected `[]`. The first failed 1 of 1 on base f9e16cda and 2 of 2 with T-172; the second 0 of 1 and 1 of 2.
- Cause (from the worker): the operator_boundary probe of the fake agent (`crates/horch-e2e/src/probe.rs` near line 458, act `write_requests`, sandbox none) writes `horch-probe-<pid>.json` with mode 0644 into `fleet/run/requests` and removes it after. When a pass of fleetd reads the folder between the write and the remove, it opens a needs-you item "mode 0644 is wider than 0600". The product is correct (SR-5).

## Goal

The 2 tests do not depend on the time of a pass. Decide which is true and fix that: (a) in these 2 tests the fake agent must not run the probe at all (the tests are about orphans, not about the boundary); or (b) the probe is wanted, and then the item is an effect that the test must expect in a deterministic way. Do not make the product accept the file. Do not make the assert ignore each needs-you item: an unexpected item must still fail the test.

## Steps

1. See the failure: run the file 5 times; record how often each test fails.
2. Fix. Run the file 10 times: 0 failures.
3. Check each other caller of the probe act `write_requests`: say in the commit message if another test has the same race.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test: find the cause. Do not weaken an assert. No `#[ignore]`. No new expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the file passes 10 times in a row.
