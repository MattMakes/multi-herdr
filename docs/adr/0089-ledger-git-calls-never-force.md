# The ledger's git calls never force and never lose work

Every git call of the task ledger (`taskledger::git`: run branch, worktrees,
land, cleanup, and `taskledger::worktree_ops`) names its directory with `-C`
and never depends on the current directory. No call ever forces: no
`--force`, no `-D`, no `reset`. Keeping a redundant branch or tree is
preferred over destroying work. Each call goes through
`horch_marketplace::git::GitRunner`, the 1 git runner of horch: it never
prompts, runs in the C locale, and runs without the caller's git repository
variables, because a `GIT_DIR` set by a git hook wins over `-C`.

## Consequences

- `land::delete_worktree` sets the worktree branch's upstream to the run
  branch and deletes it with `git branch -d`. `branch -d` checks that the
  branch is merged into its upstream (else into the checked-out branch), so
  git itself verifies the work is on the run branch.
- `worktree_ops::drop_worktree` deletes a branch only with `git branch -d`
  and ignores a refusal: `-d` has its own upstream check and may refuse
  conservatively. The folder is gone by then, and the redundant branch
  stays.
- `drop_worktree` decides whether a branch holds unique work by excluding the
  branch itself from every durable ref. A reflog or a linked HEAD is not
  another published copy. A local branch, a tag or a remote ref is a copy.
- The ledger records a branch, but ordinary git commands can detach or switch
  the live checkout. `worktree_ops::verified_head` never lets a tree be
  removed while its HEAD differs from the recorded branch: keeping the
  recorded branch would not preserve the actual checkout's commit.
  `drop_worktree` repeats this check after the ref and commit safety checks,
  directly before removal, so a checkout changed during those reads stays
  intact.
