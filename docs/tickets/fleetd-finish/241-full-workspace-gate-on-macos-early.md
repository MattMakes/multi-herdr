# Early full gate on macOS: the 3 gate commands of the goal on the tip of the run branch, and each red test with its cause

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/full-gate-macos-early.md && grep -q '^## Result of each command' docs/specs/fleetd/gates/full-gate-macos-early.md && grep -q '^## Each red test' docs/specs/fleetd/gates/full-gate-macos-early.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/full-gate-macos-early.md`

## Why

Done criterion 3 of the run: the tip passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked` and `cargo test --workspace --locked`. Many fix tasks landed with a narrow Check. No task ran the 3 commands as a whole on the tip since the pre-gates (tip 895e3848). Find each red test now, before the merge of main.

## Steps

1. In your worktree (the tip of `fleet/fleetd-finish`; write the commit in the report): `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; then `scripts/dev-sandbox.sh cargo test --workspace --locked --no-fail-fast` with the output in a log file in your sandbox folder. Use `--test-threads=4` when the 1-minute load is above 32 (other fleets use this host; never touch their processes). The test run can take 2 hours: run it in the background with an `until` wait, and write 1 `horch ledger progress` line each 10 minutes (the count of test binaries done).
2. For each red test: run it alone 3 times. Then 1 row: test, file, message, red alone (n of 3), and the cause class: (a) product fault, (b) stale test of a retired behavior, (c) fails only in the full run (load, a shared thing, a leaked process: say which), (d) an entry of `crates/horch-e2e/expected-failures/`. Name the task that owns it if `ai_docs/plans/fleetd-finish/decisions.md` of the main repository or `git log` names one (tasks T-289 and T-369 are still open: their tests can be red by their open work; say so).
3. List the 10 slowest test binaries with their time.
4. Write `docs/specs/fleetd/gates/full-gate-macos-early.md` with `## Result of each command` (exit code, passed, failed, time, load), `## Each red test`, `## Slowest test binaries`, `## For the orchestrator` (a proposed ticket for each cause group). Label each claim verified or not verified. Fix nothing.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Never install the build or a tool.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. No test and no manual run touches the launchd label `horch-fleetd`.
4. Before DONE: no process of the unit is alive (`ps`), `launchctl list | grep horch` shows nothing of yours, the sandbox folder and your `target/` are removed.

## Done when

The Check passes and each red test has a row.
