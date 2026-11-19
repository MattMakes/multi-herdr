# Make the test lock_is_free_after_drop stable

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-platform && cargo test -p horch-core --test arch_scan && cargo fmt --all --check`
**Own:** `crates/horch-platform/tests/lock.rs`, `crates/horch-platform/src/lock.rs`, the lock files under `crates/horch-platform/src/os`

## Goal

The test `lock_is_free_after_drop` in `crates/horch-platform/tests/lock.rs` passes each time, and
the cause of its failure is known and written down.

## Context

The worker of T-86 saw this test fail 1 time, then pass 8 of 8 times. Its guess: another test in
the same binary starts a child process. Between fork and exec the child holds a copy of the open
lock file, so the lock is not free for a short time after the drop. This is a guess. Prove it or
find the real cause.

## Steps

1. Reproduce the failure first. Run the test binary many times in a loop with the default
   parallel threads, and under load if necessary. Write down the failure rate.
2. Find the cause. Read how the lock file is opened (close-on-exec flag) and how the lock is held
   on each OS.
3. Decide where the fault is:
   - If the fault is only in the test (a fork window from a parallel test), make the test
     tolerant in the correct way, for example a short bounded retry of the second take, with a
     comment that gives the cause. Do not hide a real fault with a long sleep.
   - If the product has the fault (a child process of fleetd can keep the lock after fleetd
     dies, longer than the fork window), do not change the product here. Send QUESTION with the
     evidence, then end your turn.
4. Run the loop of step 1 again. The failure rate must be 0 in at least 200 runs.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders, never the live ledger.
- Keep the Windows code small. Read it 2 times. Name each Windows risk in your report.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, the loop of step 4 has 0 failures, and the done summary gives the cause and
the failure rate before and after.
