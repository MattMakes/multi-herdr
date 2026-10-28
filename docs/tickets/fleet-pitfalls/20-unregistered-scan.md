# 20: The unregistered-worktree scan skips missing paths and other people's worktrees (P27, P30)

**Issue:** [#68](https://github.com/MattMakes/multi-herdr/issues/68) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `sonnet-feature`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core landing && cargo test -p horch-e2e --test worktree`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## Background

This was deferred to the task-ledger spec (#28), which closed without it. v0.3.0 did not fix it, and P30 is now wider.

`horch done` and `horch worktree list` scan every linked worktree of the repo for unlanded work.

- **P27:** One worktree whose folder was deleted makes git exit 128. The whole scan then fails: `worktree list` dies, and `done` silently reports nothing.
- **P30:** The scan reports the operator's own worktrees anywhere on disk as `UNREGISTERED`.
- **New in v0.3.0:** task-ledger worktrees are recorded only in the ledger's worktrees table, not in the old claim registry. So a worker's `done` also lists other workers' in-flight ledger worktrees as `UNREGISTERED`.

The task ledger's import already limits itself to paths under `.worktrees/`.

## What to build

The scan reports only worktrees a fleet worker made and left unlanded.

- Skip worktrees that git marks prunable, and worktrees whose folder is missing. Print 1 note naming each skipped path.
- Scan only worktrees under the repo's `.worktrees/` folder.
- Skip any path the task ledger records as a worktree.

## Acceptance criteria

- [ ] With 1 worktree whose folder was deleted, `horch worktree list` succeeds and notes the skipped path, and `horch done` still reports the other unregistered worktrees.
- [ ] A dirty worktree outside `.worktrees/` is never reported.
- [ ] A dirty worktree recorded in the task ledger is not reported as unregistered.
- [ ] A dirty, unclaimed worktree under `.worktrees/` is still reported (existing behavior).

## Blocked by

None (can start immediately)
