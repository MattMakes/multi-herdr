# Worktree-claim landing is serialized under the claim registry lock

`Landing` in `crates/horch-core/src/vcs/landing.rs` handles worker worktree
claims and loss-resistant landing. It keeps 1 durable registry per
repository. Landing is serialized with that registry's lock, so concurrent
workers always rebase and fast-forward the base branch in a linear order
and no commit is lost.

## Consequences

- Once the base has moved, a failed cleanup (set upstream, remove worktree,
  delete branch) is reported as `Landed.cleanup_note`, not as an error. An
  error here once told the worker the land had failed.
- `git branch -d` checks merged-ness against the upstream when one exists.
  `Landing::clean_up` sets the landed branch's upstream to its base when
  the repository's current checkout is unrelated to the landed base, so the
  delete is not refused.
- `Landing::unregistered` lists changed linked worktrees which have no
  registry claim. Only worktrees under the repository's `.worktrees/`
  folder are fleet worktrees. The operator's own checkouts anywhere on disk
  are never reported, however dirty they are.
- `Landing::with_recorded_worktrees` takes the paths the task ledger
  records as worktrees and leaves them out of that scan. Ledger worktrees
  are not in the claim registry, so without this every worker's `done`
  lists the other workers' in-flight worktrees as unregistered.
- A linked worktree git cannot read (prunable, or its folder missing) is
  named in `UnregisteredScan.skipped`, never dropped in silence. A deleted
  folder used to fail the whole scan, and `horch done` then reported
  nothing at all.
- The test `arc_31_long_check_keeps_the_lock` holds the check until the test
  releases it. It ages the lock past its stale age by hand and waits for
  the heartbeat to refresh it. It uses no wall-clock margin: a loaded test
  run (a slow fsync in the refresh) broke the earlier version at a 0.4 s
  stale age.
