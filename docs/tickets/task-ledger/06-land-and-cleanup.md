# 06: Land through the ledger and verify the worktree is gone

**What to build:** A worker in worktree mode creates, lands and removes its
worktree only through the ledger, and the ledger knows when the folder is
really gone from disk.

- `horch ledger worktree new <name>` creates the worktree at
  `<repo>/.worktrees/T<seq>-<name>` and its `worktrees` row (`open`), tied
  to the worker's task.
- `horch ledger land` sets `landing`, rebases, runs the task's
  `land_check`, fast-forwards the base (the existing landing logic, driven
  from the ledger row), writes `landed_commit` and `landed`, then measures
  `bytes_on_disk`, removes the worktree and its branch, checks that the
  path no longer exists, and writes `cleaned`.
- A red gate puts the row back to `open` with `check_output_path` and a
  `land_red` event.
- A removal failure after a good land writes `cleanup_failed` with the
  reason. A function `retry_cleanup` retries every `cleanup_failed` row;
  ticket 10's watchdog calls it every check.
- Cleanup deletes only a `path` that is in the `worktrees` table. Nothing
  globs `.worktrees/`.

The old `horch worktree new/land/list` commands keep working for runs that
do not use the task ledger.

**Blocked by:** 03 (Queue tasks and claim them)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.9, 5.5

- [ ] A test lands a worktree with a 50 MB ignored build folder: the row
      ends `cleaned`, `bytes_on_disk` is at least 50 MB, and the folder is
      gone.
- [ ] A test makes removal fail (a file locked or the folder read-only):
      the base still has the commit, the row is `cleanup_failed`, and
      `retry_cleanup` cleans it once the cause is removed.
- [ ] A red `land_check` leaves the branch, the worktree and the base
      unchanged.
- [ ] 2 workers landing at the same time land in a linear order (the
      existing landing lock), both rows end `cleaned`.
- [ ] A folder under `.worktrees/` with no row is never removed.
