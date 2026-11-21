# The test file p3_worktrees takes 12 minutes: find where the time goes and make it short without a weaker test

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p3_worktrees && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p3_worktrees.rs`

## Fault

`crates/horch-e2e/tests/p3_worktrees.rs` (8 tests) takes 10 to 13 minutes (measured by tasks of this run: 725 s for 8 of 8 on a loaded host). It is in the Check of each task that edits worktree custody, and a land step runs the Check 2 times: the land of task T-286 took 49 minutes. It is also 1 of the longest files of the CI test jobs (macOS 93 min, Linux 74 min in CI run 4).

## Goal

The file proves the same rules in a small part of the time. Target: under 3 minutes on this host at a low load. Each test keeps each assert.

## Steps

1. Measure first: run the file 1 time with `--test-threads=1` and `-- --nocapture`, and record for each test its time and where it waits (the harness prints its waits; add timing output to the test log, not to the product). Record the 1-minute load.
2. Find the cause of each long wait. Candidates: a wait for 1 real period of the product (the worktree list period of 60 s: task T-278 set `worktree_list_period_s = 5` in the fleet config of `p7_exec.rs`, and `p0_rescue.rs` does the same); a wait on wall time where the fleet clock can be moved by the test; a fixture that builds a large repository or runs a slow job; tests that run 1 at a time because they share a thing that they do not need to share.
3. Fix by evidence. Allowed: a shorter period in the fleet config of the test world when the test does not prove the length of that period; a move of the fleet clock in place of a real wait; a smaller fixture. Not allowed: a weaker assert, a removed test, a larger time limit, a sleep. A test that proves a time rule (for example RR-10 "within 1 list period": task T-335 changes that test now; rebase and keep its change) must still fail when the product is 1 period late: prove it 1 time with a scratch edit of the product that you revert, and say so in the commit message.
4. Write the before and after time of each test in the commit message. Run the file 3 times.
5. In the DONE note: name each other e2e test file that has the same cause (search for the same wait), with its time in CI run 4 if `docs/specs/fleetd/gates/ci-run-4.md` gives it. Do not change those files.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build or a tool. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No larger time limit without a measured cause. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
6. Before each finish: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan` and `cargo clippy --workspace --all-targets --locked -- -D warnings` add no new failure (tasks T-327 and T-347 fix the ones of the base).
7. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit: restore each comment that gives a security rule, an invariant, a bound or a design id.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes 3 times, the file takes under 3 minutes (or the commit message says with numbers why not), and each assert is as before.
