# Review finding D243-F1 (Critical): 2 live holders of 1 folder lock after the 5 s guard timeout of a release

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked fsx:: && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fsx.rs`

Source: `docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md` on the run branch (finding D243-F1; if the file is not on the branch yet, read it in the worktree `.worktrees/T380-*` of the main repository) and the commits of task T-368 (`git log --grep 'T-368' -- crates/horch-core/src/fsx.rs`).

## Fault

`DirLockGuard::drop` waits at most 5 s (`BREAKER_STALE_AFTER`) for the breaker guard and then goes on without it. Order of events on Unix with a working guard: a breaker P holds the guard, judged the lock of A stale, and is paused for more than 5 s; A releases without the guard (renames its lock to a tomb); B takes the lock; P resumes and renames the lock of B to its tomb, rejects the owner, and before P puts it back C takes the lock; the put-back fails and P deletes the tomb: B and C both hold. This lock guards the land lock, the landing registry, the ledger open, the watchdog, the update and the compaction job folder. The comment "Limits:" names the case; a named limit does not keep the contract of a lock.

## Goal

With a working guard on Unix, no order of events gives 2 holders, also when a process that holds the guard is paused for any time. A release never removes a lock without the guard. A release that cannot get the guard still frees the lock for the next taker in a bounded time and never blocks its process for more than the 5 s of today.

## Design freedom, with these rules

1. The simple form of the review: when `wait_for_guard` gives `None`, `drop` returns and removes nothing. Then say what frees the lock: the holder process can live on (fleetd is long-lived), so the lock is not stale by a dead pid; it would block each taker until its age limit. If that is the result, it is not enough: add a way that makes the lock free at once for a taker that works under the guard, for example the release writes a mark "released" with its nonce into the lock folder (1 atomic create), and `is_stale` reads a lock with that mark as stale. The taker then breaks it under the guard by the normal path. Show that the mark cannot be written into the lock of another holder (the lock folder can change between the check and the write: write through a path that cannot name another lock, or check the nonce after the write and say what a wrong mark does).
2. The breaker path: can the put-back of a rejected tomb still fail (step 6 above) when each remover holds the guard? Prove that it cannot with a working guard; if it can, a breaker must not remove a lock that it did not judge: check the owner BEFORE the rename under the guard and keep the rename only for the owner that was judged.
3. Where no guard exists (Windows; a flock error), the limit stays: keep the "Limits:" doc exact for those 2 cases only.
4. Tests, each with a hook and no sleep as a proof: (a) the order of events above gives 1 holder (fails before the fix); (b) a release at the guard timeout frees the lock for the next taker in a bound and takes no more than the timeout; (c) the tests of T-368 stay green. Use a short timeout through a parameter of the test path, not a 5 s wait in each run. Run `fsx::` 30 times.
5. Each user of the lock keeps its behavior when a release leaves a marked lock: read the callers (`vcs/landing.rs`, `taskledger/finish/land.rs`, `taskledger/store.rs`, `taskledger/watchdog.rs`, `harness/update.rs`, `compaction/jobfile.rs`, `crates/horch/src/cmd/recipes.rs`) and name any that reads the lock folder by itself.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`, or the ledger of another fleet. Never install the build.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert.
4. Never run pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives the order of the steps of the lock, an invariant or a limit.
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes, test (a) fails before the fix, and the commit message gives the proof for rule 2.
