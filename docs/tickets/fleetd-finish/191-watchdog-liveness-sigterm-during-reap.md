# watchdog_liveness: after a SIGTERM to fleetd during a reap, the fleetd row does not name the new fleetd in 120 s

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test watchdog_liveness && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/watchdog_liveness.rs`

## Fault

Found by task T-172 (opus-hardening-11) on 2026-11-20. The test `a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd` fails: `timed out after 120 s waiting for the fleetd row to name the new fleetd`. It failed 1 of 1 on base f9e16cda and 2 of 2 with the change of T-172. The cause is not known. CI run 3 did not list it (it can be new after head 001e73c2: tasks T-171, T-195 and T-172 changed the start of fleetd and the launch).

## Goal

Find the cause and say: product fault or test fault, with evidence. Note of the orchestrator: task T-171 made fleetd refuse a start by a fleet process, and 3 other e2e tests broke by that (they started fleetd from the agent process; the fix was a wrapper process, see `git log --grep T-171` and the commits of T-89 and T-232). Check this first.

- A test fault: fix the test; keep each assert of the rule (the reap is left to the next fleetd; nothing is reaped twice; nothing is lost).
- A product fault (the next fleetd does not start, or does not take the reap): send `QUESTION:` with the evidence and the file. Do not edit product code without an answer.

## Steps

1. Run the test 3 times on the tip. Keep the folder of 1 failed run and read the fleetd logs and the ledger.
2. Fix or report. Run the whole file 5 times: 0 failures.

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

The Check passes and the file passes 5 times in a row, or the product fault is reported with evidence.
