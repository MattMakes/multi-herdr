# Worktree triage never loses work

Worktree triage (#308) applies the operator's decision for each parked
worktree (`taskledger::triage::apply`, section 3). Each decision runs on its
own, records 1 event and sets the row's `decision` and `decided_at`. The
rules of section 5 hold for every one. Git is never forced. Before a folder
is removed, `unsafe_to_remove`, or an archive or trash ref read back holding
the tip, proves that no commit and no uncommitted file is lost. A failed step
deletes nothing and leaves the row `cleanup_failed` with the reason. `LAND`
never moves the default branch.

## Consequences

- `ARCHIVE` keeps its work at the local ref `refs/horch/archive/<name>`.
  `DELETE` keeps it at `refs/horch/trash/<name>/<UTC yyyymmddThhmmssZ>` until
  `horch ledger gc` (`refs::gc`) deletes it, `TRASH_DAYS` (30) days on. The
  time is in the ref name, so a folder that never had a ledger row is dated
  too. Only local refs are used, so no remote is needed.
- `refs` points a ref at the tip and reads it back. A new ref is created only
  when none exists. An existing one moves only forward (its commit is in the
  tip's history), so no work it held is lost.
- `gc` deletes a trash ref with `update-ref`'s old value as a guard: a ref
  that moved is kept.
- After `DELETE` removed the folder, the worktree's branch is deleted only
  when the trash ref, read back, holds the branch's commit. The branch is
  then a 2nd name of work that the trash ref keeps.
- A checked-out branch must be the recorded branch. A clean detached HEAD is
  also safe when a durable branch, remote or tag already reaches it.
- The operator's reply is parsed by code (`triage::reply::parse`), never by a
  model. It is the markdown table that `template` prints, with `Decision`
  filled in. Each row is matched to a triage row by ID, its `Worktree` cell
  must still name that row, and every value is validated. Any bad line
  refuses the whole reply, so no decision runs from a typo.
- `triage::facts::conflicts` uses `git merge-tree --write-tree`, which touches
  no ref, index or folder. Exit 1 is a conflict: the output is the tree id on
  the first line, then 1 conflicting path per line.
- Version 6 of the schema stores the disk a parked worktree held (`bytes`,
  `bytes_measured_at`) when `horch worktree triage` last measured it.
  `ledger status` runs often, so it sums the stored values for a run's
  parked worktrees and never walks a folder.
