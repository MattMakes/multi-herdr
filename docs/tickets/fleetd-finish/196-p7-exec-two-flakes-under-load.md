# p7_exec: 2 tests fail under load: `horch result: no live attempt of this process`, and no Codex exec run in 120 s

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p7_exec && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p7_exec.rs`

## Fault

Found in the land check of task T-267 (opus-hardening-12) on 2026-11-20, macOS, load average 31. 2 of 8 tests of `crates/horch-e2e/tests/p7_exec.rs` failed; 3 later whole-file runs passed 8 of 8 at load 26 to 46. The change of T-267 does not reach these paths.

1. `exec_claude_task_runs_to_done`: the agent got `horch result: no live attempt of this process` (from `outbox_writer.rs`, the lookup of the wrapper in the parent chain of the process).
2. `new_attempt_gets_the_256000_window`: attempt A2 was live, but no Codex exec run was seen in 120 s.

## Goal

No flaky test. For each of the 2: find the cause with evidence and say product fault or test fault.

- Number 1 can be a product fault with a loss of work: an agent that calls `horch result` at a moment when fleetd or the ledger does not yet (or no longer) show its attempt as live gets a refusal, and its result is lost. Find the exact moment: which fact does the lookup read (ledger row, attempt file, parent chain), who writes it, and can the agent run before it is written. Task T-172 (ticket 157, "no agent code runs before fleetd has recorded the process of its attempt") changes this area: read its commits on the run branch first (`git log --grep T-172`); if T-172 has landed, test on a tip that holds it and say if the fault is gone, with a count of runs.
- Number 2: find what A2 waited for.

A product fault: send `QUESTION:` with the file and the evidence; do not edit product code without an answer. A test fault: fix the test; no larger time limit without a measured cause.

## Steps

1. Reproduce with a build load of your own worktree (no host-wide load tool): run the file in a loop until each of the 2 fails at least 1 time, or 40 runs pass. Keep the folder of each failed run.
2. Decide and fix or report.
3. Run the file 20 times under the same load: 0 failures.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*` or a like pattern. Remove only the exact folder of your own test. Never run pkill or killall.
7. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the file passes 20 times in a row under load, or each product fault is reported with evidence.
