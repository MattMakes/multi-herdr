# The pass of fleetd calls herdr with no time limit: a herdr that does not answer holds the pass

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --locked --test fleetd_herdr_timeout && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p0_fleetd_hang --test p1_orphans && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/fleetd.rs`, `crates/horch/tests/fleetd_herdr_timeout.rs`

## Fault

Found by task T-275 (opus-55) on 2026-11-20; the orchestrator read the code and confirms it.

- `with_pass` in `crates/horch/src/cmd/fleetd.rs` (near line 1149) builds each herdr handle with `Herdr::with_bin(&ctx.bins.harness.herdr)`. `with_bin` sets `timeout: None` (`crates/horch-core/src/workspace/herdr.rs` near line 127).
- `Herdr::with_timeout` exists (`herdr.rs` near line 136): a herdr that has not answered after the limit is killed and the call returns an error.
- The design says each herdr call has a timeout (`docs/specs/fleetd/design.md` line 1495, row "herdr calls": "herdr CLI only, JSON envelope, own process group or job object, timeout"), and rule A32 says nothing slow runs between 2 heartbeats.
- Result: a herdr CLI that hangs holds the pass of fleetd until the hang guard of the service manager restarts fleetd (FM-12). Each pane read, pane close and agent state call of the pass is such a call.

## Goal

Each herdr call of the fleetd pass has a time limit. A call that reaches its limit is killed with its process group, the pass records the failure as a fact (the pane is "not read", never "gone": a time limit must not make fleetd reap a live attempt or close a pane), and the pass goes on and writes its heartbeat.

## Steps

1. Find each place where fleetd (the daemon, `horch fleetd check`, `horch ledger watch --once`, the launch helper) builds a `Herdr`: list them in the commit message with the limit that each one gets. Use 1 constant or setting; if the code already has a value for a herdr call limit, use it.
2. Write the failing test `crates/horch/tests/fleetd_herdr_timeout.rs`: a fake herdr that sleeps on 1 call; 1 pass ends in bounded time, writes its heartbeat, stops no attempt and closes no pane because of the timed-out read.
3. Fix. Read how the pass treats an error of a pane read today: if an error is read as "the pane is gone", that is a second fault: send `QUESTION:` with the evidence (the file is probably `pass.rs`, which other tasks hold).
4. Run the Check 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`. Never touch the launchd label `horch-fleetd`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`.
6. Never run `rm -rf /tmp/horch-e2e-*` or a like pattern. Never run pkill or killall.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes 3 times and the commit message lists each herdr handle with its limit.
