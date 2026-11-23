# Review 2 findings D254-F1 (Critical), F2 and F3 (Important): the check and the kill of a group are 2 steps; an interrupted wait leaves the timed git; a restart loses the notice of a lock

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test worktree_jobs_stop --test gate_jobs --test fleetd_pass_p4b --test arch_scan && scripts/dev-sandbox.sh cargo test -p horch-marketplace --locked && scripts/dev-sandbox.sh cargo test -p horch-platform --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-marketplace/src/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-e2e/src/harness.rs`

Source: `docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md` on the run branch. Read the 3 findings in full, and the commits of task T-369 (`git log --grep 'T-369'`).

## D254-F1 (Critical)

`jobs.rs` (about lines 1010 to 1023), `effects.rs` (85 to 102) and the e2e harness (`harness.rs` about 757 to 763 and 818 to 827) check that the recorded leader runs with the recorded start, and then, as a second step, list the members of the group or send KILL to the group id. Between the 2 steps the group can end and the id can go to a group that fleetd did not start. Rule of the run: fleetd never stops a process that it did not start.

Fix, as the review says: for a stop of a group with a leader: (1) take a snapshot of each member with its start time; (2) check again that the recorded leader still leads the group with its recorded start; (3) signal only the processes of the snapshot, each by pid and start time (the way of `stop_same`), for TERM and for KILL. No `kill` of a group id after a separate check. The same in the harness. A child that is made after the snapshot can be missed: say what catches it (the next pass takes a new snapshot while the leader runs; with the leader gone, the notice of an orphan group of T-319). Say in 1 "Limit:" comment the window that stays (between the identity check of 1 pid and its signal) and why it cannot be closed without a process handle of the OS (pidfd on Linux: say if `horch-platform` could use it, do not add it now). Keep the bound of the job stop: the KILL of a job that ignores TERM still ends each member within the design grace; show it with the existing test of a fetch that ignores TERM. New tests with a hook between the check and the effect, as the review names them; each fails before the fix.

## D254-F2 (Important)

`crates/horch-marketplace/src/git.rs` (479 to 503, 511 to 525): the wait of the git with a time limit returns at `EINTR` through `?`; the child is not reaped, its group can live on and keep a `.lock`. Fix: retry the wait at `ErrorKind::Interrupted`; a guard after the spawn kills the own group of the child and reaps it on each early return, and is disarmed only after the wait. Test: a wait seam that returns `Interrupted` 1 time.

## D254-F3 (Important)

`worktree_custody/adapters/git.rs` (768 to 818) keeps the path of a `.lock` file that a killed git left only in memory (`LOCKS_LEFT`); the needs-you item is opened by `jobs::observe` in a later pass. A restart of fleetd between loses it. Fix: a row before the effect (A24): record the path in the ledger (an event or fact row, no schema step: the way of the TERM fact and the `GROUP_LEFT` notice of T-319) in the transaction that can see the error, or in the first write after it if the git ran outside a transaction; say which. `observe` reads the rows with 1 set read, opens the item 1 time, and resolves it when the file is gone. Test: a restart between the pass of the timeout and the pass of the observe still gives the item.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Linux proof of the stop tests: a container of the image `t-git241-linux:local` (`docker run --rm --name t<task number>-lx-<n> --privileged --init`; never stop, remove or prune a container or image of the operator such as `litellm-postgres`, `phoenix`, `buzz-prod-*`; never `docker ... prune`; remove by exact name only what you made).
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A pass never waits for a job and runs no SQL statement for each row. A signal handler does only async-signal-safe work. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert.
4. Never run pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each "Limit:" and SAFETY comment and each comment that gives a rule or a design id.
6. Before DONE: no process and no container of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes, each of the 3 findings has a test that fails before its fix, and the commit messages name D254-F1, D254-F2 and D254-F3.
