# A launch whose host call fails waits for the start timeout before the retry

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test push_dispatch && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p0_landing && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/launch.rs`, `crates/horch-core/tests/push_dispatch.rs`

Source: the DONE note of task T-254 (worker opus-58, 2026-11-21): "the Err branch of launch() in launch.rs (host.launch fails) still sets only launch_failed. That attempt waits for the start timeout. The features test of p0_landing depends on it."

## Fault

Task T-254 made a failed spawn that the launch helper reports (`worker_launch_failed`) end the push attempt at once (`push_launch::end_failed_spawn`). A second path is left: when `host.launch` itself returns an error inside `launch()`, the worker gets `launch_failed` but the attempt stays in launching until the start timeout (90 s of the fleet clock). The same known failure has 2 behaviors.

## Goal

Decide by the design and by evidence, then make both paths agree:

1. Read `launch.rs`, `end_failed_spawn` and the commit of T-254. Read the design: the rules of a launch that fails (search `docs/specs/fleetd/design.md` for "launch", RR-11, FM-22 and the retry and backoff rules).
2. Find why "the features test of p0_landing depends on it": name the test and the assert. If the test depends on the wait only by accident, the Err branch ends the attempt at once as T-254 does, and the test is corrected to the designed effect. If the wait is a designed behavior (for example a backoff that protects the host from a launch loop), keep it, write the design line in a code comment of 1 line, and add the test that pins it.
3. Write the failing test first in `crates/horch-core/tests/push_dispatch.rs`: `host.launch` fails; the attempt ends with a failure outcome in the same pass; the next launch follows the retry rule and its backoff; a launch that fails each time cannot loop without a bound (name the bound).
4. Run `p0_landing` 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet.
6. `launch.rs` and `push_dispatch.rs` are shared files (task T-300 changes `launch.rs`): change only what this ticket needs; `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the new test is green 3 times, and the commit message names the design line that decides the behavior.
