# 11: Import the old ledgers and list work that never landed

**What to build:** The operator imports every old JSON session ledger and
worktree registry on the machine once, then asks an orchestrator what past
work never landed.

- `horch ledger import [--dry-run]` copies each old record unchanged into
  `import_raw` (stage 1), then maps it to typed rows with `origin =
  'import'` (stage 2), as in spec section 11.2, including the state table.
  A record that cannot be mapped keeps its raw row with `map_error`. A
  second import skips records it already has. The source files are not
  changed.
- The import looks at disk for every imported worktree (does the path
  exist, commits ahead of base, dirty files, bytes) and writes
  `import_findings`: `worktree_unlanded`, `worktree_orphan`,
  `done_not_landed`, `ended_without_done`, `dirty_worktree`, `disk_heavy`.
- `horch ledger unfinished [--project <path>] [--json]` lists open
  findings by run, newest first. Native runs show here too: a cancelled
  task with an open worktree, or a reaped task that never finished.
- `horch ledger reopen <finding> --file <ticket> --primary <persona>`
  queues a native task whose claim prints the old branch, worktree and
  transcript path.
- `horch ledger dismiss <finding> "<reason>" [--cleanup]` closes it;
  `--cleanup` removes that 1 registered path only.

**Blocked by:** 03 (Queue tasks and claim them), 06 (Land through the ledger)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` section 11

- [ ] Fixtures: an old ledger with records from before typed status, with
      missing `session_id`, `pane_id` and `plan`; all import, none fail.
- [ ] A fixture repo with 1 landed, 1 unlanded with 2 commits ahead, 1
      dirty and 1 orphan worktree gives exactly those findings.
- [ ] `--dry-run` writes nothing and prints what it would import.
- [ ] Running the import twice gives the same row counts.
- [ ] `dismiss --cleanup` on an orphan refuses: an orphan has no
      registered path.
