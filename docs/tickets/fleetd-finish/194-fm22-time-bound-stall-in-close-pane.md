# FM-22: the orphans stop 17 s after the start of fleetd in 2 of 63 runs under load (limit 10 s): fleetd waits 12 s in `close_pane`

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_orphans && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_orphans.rs`

## Fault

Found by task T-268 (opus-52) on 2026-11-20. The time assert at `crates/horch-e2e/tests/p1_orphans.rs:276` (test `fm22_orphan_process_and_pane_after_a_reap_are_stopped`) fails in 2 of 63 runs at a load average of 24 to 45: the orphans stopped 17 s after fleetd started; the limit is 10 s (FM-22 gives 5 s as the recovery time). The base branch has the same assert.

Evidence of the worker: the polite stop comes in pass 1 at about 1 s. The kill and the pane close come in pass 2 at about 6 s. In the failing run, pass 2 read the panes at 5.7 s, and then fleetd made no herdr call until 17.7 s. A sample of fleetd in a slow run shows 80% of 3 s in `Pass::stop`, `close_pane`, waiting on the output of the child process of the fake herdr pane close.

## Goal

No flaky test. Find the cause of the 12 s with evidence and say which is true:

- (a) Product fault: the pass of fleetd waits with no time limit on a herdr child process, so a slow herdr holds the pass and the heartbeat (design rule A32: nothing slow runs between 2 heartbeats; each herdr call has a time limit). Then send `QUESTION:` with the file and the evidence; the fix is in product code that is not in Own.
- (b) Test fault: the fake herdr is slow to start under load (a process start of seconds), and the product is correct. Then the test must prove FM-22 in a way that does not depend on the load of the host: for example by the count of passes or by the fleet clock between the reap and the stop, with the wall time only as a wide guard. Keep the rule: an orphan process and its pane are stopped by the pass after the reap, and nothing else is stopped.

## Steps

1. Reproduce: run the test in a loop under a build load of your own worktree (not a host-wide load tool) until it fails; keep the folder of the failed run. Record the times of each herdr call from the fake herdr log.
2. Decide (a) or (b) with the evidence. Fix (b) in the test, or report (a).
3. Run the file 20 times under the same load: 0 failures.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Do not make a time limit larger without a measured cause. Do not weaken an assert. No `#[ignore]`.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the file passes 20 times in a row under load, or the product fault is reported with evidence.
