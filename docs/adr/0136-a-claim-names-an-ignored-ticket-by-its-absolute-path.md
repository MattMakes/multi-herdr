# A claim names an ignored ticket by its absolute path

A ticket queued from a git-ignored folder is absent from the worker's git
worktree. `taskledger::absolute_ticket_if_missing`
(`crates/horch/src/cmd/taskledger.rs`) then rewrites the claimed ticket
path to its absolute path in the project root.

## Consequences

- A tracked ticket keeps its relative path.
- A shared-checkout task works in the project root, so its relative path
  always resolves.
