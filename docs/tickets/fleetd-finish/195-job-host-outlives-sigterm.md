# A job host `horch job run N` is alive 25 s after SIGTERM in e2e teardown under load (a leaked process)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test job_host_stop && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_jobs && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/job_host_stop.rs`, `crates/horch/src/cmd/jobcmd.rs`, `crates/horch-e2e/src/harness.rs`

## Fault

Found by task T-234 (qa-engineer-55) on 2026-11-20. In whole-file runs of `crates/horch-e2e/tests/taskledger.rs` at a load average of 24 to 85, 4 of 6 runs fail in the teardown of the harness (`crates/horch-e2e/src/harness.rs` near line 699): `the test leaked 1 process(es) that outlived teardown by 25 s`; the process is `horch job run <N>`. It hits a different test each run (6 tests in all). The tests themselves pass.

This matters for the product: a process that fleetd started and that does not end on SIGTERM is a leftover process (the goal of this run forbids one), and the stop of a job group has a time bound in the design (the job host, the stop grace, and the kill after it).

## Goal

Find why a job host outlives SIGTERM by 25 s, and fix the owner:

- (a) The job host does not act on SIGTERM while it waits (for its child, for a lock, for a write of its result file), or it waits for a child that ignores the signal and it never kills the group. That is a product fault: the job host must end, and must end its process group, in the stop grace.
- (b) The harness sends the signal to the wrong process or group, or measures before the group can end. That is a harness fault.

First find the file of the job host (`horch job run`): the Own list names `crates/horch/src/cmd/jobcmd.rs` as a guess. If it is another file, send `NOTE:` with the file; it is then in your scope if no running task holds it. Read ticket `43-harness-teardown-under-load.md` and its commits (`git log --grep 'T-114'`) first: that task changed the teardown.

## Steps

1. Reproduce with a build load of your own worktree (no host-wide load tool): run `taskledger.rs` whole-file until the teardown fails; keep the folder; take a `sample` of the leaked process before the harness kills it (add a debug hook in the harness if needed, and remove it after).
2. Write the failing test `crates/horch-core/tests/job_host_stop.rs` for the cause that you found (for example: a job host whose child ignores SIGTERM ends with its group in the stop grace).
3. Fix. Run `taskledger.rs` whole-file 6 times under the same load: 0 teardown leaks.
4. Do not make `reap_grace` larger to hide the leak.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`.
6. Never run `rm -rf /tmp/horch-e2e-*` or a like pattern. Remove only the exact folder of your own test.
7. Never run pkill or killall. Stop only a process that your own test started, by its pid.
8. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
9. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes and `taskledger.rs` shows 0 teardown leaks in 6 whole-file runs under load.
