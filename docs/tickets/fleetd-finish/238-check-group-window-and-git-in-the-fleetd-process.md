# 2 stop gaps that task T-319 found: the window before the handler of a check group, and a git that runs in the fleetd process with no stop path

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test gate_jobs --test worktree_jobs_stop --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`

Source: the DONE note of task T-319 (findings F3 and F4) and its commits on the run branch (`git log --grep 'T-319'`). Read them first: the TERM-first stop rule, the registered groups of a job process (8 slots, task T-276), the notice of an orphan group.

## Fault F4 (fix it)

`run_check` in `fleet_runtime/application/jobs.rs` (about line 1367 to 1381) launches the check, then stores `CHECK_GROUP` and arms the handler `kill_group_on_termination`. A TERM of the job process between the launch and the store leaves the check group alive with no owner. The gate group (`GATE_GROUP`) and each job kind with sandboxed git of T-319 can have the same order: check each.

Goal: no window. The handler is armed before the launch, and the group is known to the handler from the first moment that the child exists (for example: block the signals around the spawn and the store with the mask of the thread, then restore; or spawn the child stopped in its own group, register, then let it go). A signal handler does only async-signal-safe work. Proof: a test with a hook between the launch and the register that sends TERM to the job process there; the child is gone in a bound. It fails before the fix. Run on macOS, and on Linux in a container (image `t250-linux:local` exists: `docker run --rm --name t<your task number>-lx --privileged --init`; never stop, restart, remove or prune a container or image of the operator such as `litellm-postgres`, `phoenix`, `buzz-prod-*`; never `docker ... prune`; remove by exact name only what you made; the image has git 2.39.5, so a test that ends in a rescue cannot pass in it).

## Fault F3 (report first, then fix if small)

A git that fleetd runs in its own process (`effects.rs` `integration_git` at line 365, used by the pass; and the 2 places of `crates/horch/src/cmd/fleetd.rs` near lines 833 and 1185) has no stop path: a hung git holds the pass, and with it each rule that needs a pass (leases, the stall ladder, the command channel answers?). Design rule A23: a pass never waits for a job.

Answer with code lines: which git commands run in the pass process, are they local reads only or can they touch the network or a lock, what time limit do they have, and what holds if 1 hangs. If each has a time limit and ends its child at the limit: say so, add the missing test, done. If not: give each a time limit with the kill of its group at the limit (the design has a bound for a local git command: name the line), or send `QUESTION:` with options when the right fix is to move the command into a job. `crates/horch/src/cmd/fleetd.rs` is a shared file: ask before you edit it.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A dev fleetd and a test never stop a process that they did not start. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Other fleets run on this host: never touch their processes. At most 4 test processes at the same time.
4. No flaky test, no `#[ignore]`, no weaker assert, no sleep as a fix, no larger time limit without a measured cause.
5. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives a safety rule, an invariant, a bound or a design id.
6. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes, F4 has a test that fails before the fix, and F3 has an answer with code lines and either a fix with a test or an open `QUESTION:`.
