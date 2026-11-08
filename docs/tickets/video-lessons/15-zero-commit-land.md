# 15: A task with 0 commits lands with no push, and its park never re-queues

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core taskledger::finish && cargo test -p horch-e2e --test i308_fleet_steward`
**Own:** `crates/horch-core/src/taskledger/finish/`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/store.rs`

## Goal

When a task has 0 commits since its claim and a clean worktree, `horch ledger finish` skips the push in land and pr, cleanup removes the worktree, and `horch ledger done` passes. A park of such a task never re-queues it.

## Evidence

- A media fleet in another project ran on 2026-11-07. The ssh agent had no key, so every push failed.
- A task with 0 commits still pushed the run branch in the land step. Every video task failed land, and `horch ledger done` refused.
- The workers parked the tasks as unlanded. The park re-queued them. The operator cancelled 15 tasks by hand.
- 16 empty worktrees of 2.5 GB each (50 GB) were left for triage.
- Prior fix, run solo-w68 T-5 (commit 0452be1c, merged in e10eedfd): a task with NO worktree and no repo change finishes as done. Read that change first (`git show 0452be1c -- crates/horch-core/src/taskledger/`). This ticket covers the case WITH a worktree that has 0 commits.

## Steps

1. Read the land, pr and cleanup steps under `crates/horch-core/src/taskledger/finish/`. Find where land pushes.
2. Detect "0 commits since the claim base and a clean worktree" once, in 1 function.
3. For that case: land and pr record `not_applicable` with the reason "no commits", and push nothing. Cleanup removes the worktree and its branch. done passes.
4. A worktree with uncommitted changes is not this case: keep the current behaviour.
5. Find where an unlanded park re-queues a task. A park of a task with 0 commits must not re-queue it. Prefer that such a task never parks: it finishes as done by step 3.
6. Add tests: a 0-commit task with a push remote that always fails finishes as done and pushes nothing; a task with 1 commit still lands and pushes; a dirty worktree still parks.
7. Run the check.

## Out of scope

- Do not change the steward, triage or --check none code.
- Do not change the push itself for tasks with commits.

## Done when

- The new tests pass. The check passes.
- The DONE report names the function of step 2 and the changed files.

## Report

Run `horch ledger done` with the files changed, the tests, and the root cause.
