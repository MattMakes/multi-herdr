# Task-ledger worktrees are recorded and looked up in the ledger

A task-ledger worktree is recorded in the ledger's worktree table, not in
the legacy claim registry, and horch's worktree scans read the ledger.

## Consequences

- The unregistered-worktree scan of `horch worktree list` and `horch done`
  consults the ledger. Without it, every worker's `done` lists its peers'
  in-flight work as unregistered.
- A ticket file that is not committed is missing from the worker's
  worktree, also an inherited one. `horch ledger claim` then names the
  ticket by its absolute path in the main checkout.
