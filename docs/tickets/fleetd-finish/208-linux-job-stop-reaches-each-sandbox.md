# Linux: each stop of a job ends its sandbox (worktree jobs, and a KILL of the job group with a gate)

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test gate_jobs && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test worktree_jobs_stop && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`, `crates/horch-core/tests/worktree_jobs_stop.rs`

Source: 2 notes of task T-309 (worker opus-hardening-18, 2026-11-21). Read the commit of T-309 on the run branch first: the Linux sandbox is now `bwrap --unshare-pid` with `setsid` as the first program inside; a KILL of the group of bwrap ends each process of the sandbox; a TERM alone does not, because the init of a PID namespace ignores TERM from outside.

## Faults

1. Worktree jobs (`worktree_custody/adapters/executors.rs`, `git.rs`): git runs through `cmd.output()` in the process group of the job, with no own group and no handler. `jobs::stop` at the deadline sends TERM to each group member. The namespace init ignores TERM. The job process dies, so no KILL follows (the KILL goes only to a job that is still alive). The git sandbox lives on until git ends. FM-38 and the rule "each job has a time limit and fleetd ends it" need the sandbox to end.
2. Gate jobs: the gate leads its own group (`GATE_GROUP`). A KILL of the job process group (`jobs::stop` with kill, with no TERM first that the handler of the job host can act on) does not reach the gate group, so the gate and its sandbox stay alive.

T-309 gave no run proof of these 2 paths (code reading only). Prove each one first.

## Goal

After each stop path of a job (the deadline, the stop of fleetd, the cancel of a task, an abandoned run), no process of the job is alive on the host on Linux and on macOS, in bounded time.

## Steps

1. Failing tests first, proved in a Linux container the way T-309 did it (its DONE note and its commit message give the commands; the image must be built and removed by exact name; `docker run --rm`; never `docker ... prune`; never touch the containers `litellm-postgres`, `phoenix`, `buzz-prod-*`): (a) a worktree job whose git hangs (a fake git that sleeps and writes a heartbeat file) is stopped at its deadline: the heartbeat stops and a search of the host `/proc` for the marker finds no process; (b) a gate job gets a KILL of its job group with no TERM first: the same 2 asserts. If (b) cannot happen in the product (name each caller of the kill form and why the handler always runs first), write that proof in the commit message in place of a fix.
2. Fix 1 by the pattern of `run_gate`: each sandboxed git call of a worktree job runs in its own group that is registered with `kill_group_on_termination` (task T-276 gave the handler 8 slots: check that the count is enough and that a slot is freed after the call).
3. Fix 2 if step 1 shows it: the stop with kill also kills each registered group of the job (the job records its groups where fleetd can read them), or the kill form is never used without the TERM step. Choose the smaller change and say why.
4. Search for each other `Command` of the product that starts a platform sandbox on Linux (T-309 names triage apply of the CLI and the marketplace git: they have no stop path). For each: say if a hang of the child can hold a lock, a job slot or fleetd; if yes send `QUESTION:`.
5. macOS: the same tests must be green there too (Seatbelt has no PID namespace: the group kill does the work).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. A dev fleetd never stops a process that it did not start.
6. Write 1 `horch ledger progress` line each 5 minutes while a container run is in progress. A container run that gives no output for 10 minutes hangs: read `docker top <name>`, record it, stop the container by its exact name.
7. Task T-286 changes other files of `worktree_custody/`: `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
8. Before DONE: no process and no container that the unit started is alive, the image that the unit built is removed, and the sandbox folder is removed.

## Done when

The Check passes on macOS, tests (a) and (b) are green 3 times in the Linux container, and the commit message lists each stop path with the signal and the group.
