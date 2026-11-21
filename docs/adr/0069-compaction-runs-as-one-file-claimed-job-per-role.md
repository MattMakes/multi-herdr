# Compaction runs as 1 file-claimed job per role

A compaction job for a role is claimed by files under
`<state_root>/compact/` (`crates/horch-core/src/compaction/jobfile.rs`).
`create_job` creates `job.json` with `create_new`, so 2 jobs for 1 role
cannot both run: the second gets `ErrorKind::AlreadyExists`. The job types
1 line into the harness pane, and `compaction::job` judges the outcome
from what the pane shows.

## Consequences

- `start_locked` works under a `fsx::DirLock` in `<state_root>/compact/`.
  It checks liveness again. When the job is Lost or None, it removes
  `job.json`, `heartbeat`, `spawned` and `lost` (it keeps `job.log`), runs
  `spawn` and writes `spawned`. When the job is live, it returns
  `Started::AlreadyRunning` and spawns nothing. So 2 callers cannot remove
  each other's new job.
- `claim_lost` creates the `lost` file with `create_new`. Only the 1
  caller that created it gets the `job.json` content, and only that caller
  records the `compact-failed` event. Other readers see `AlreadyExists`
  and record nothing. A `job.json` that does not parse gives a record with
  step `unknown`.
- `compaction::policy::compact_line` gives the line typed into the pane:
  `/compact <instructions>` where the harness takes instructions,
  `/compact` where it does not, None where it has no command. Instructions
  with a line break are refused (None), because a second line would be
  typed as a separate prompt.
- `compaction::job::new_failure` returns the first harness failure line
  that the pane shows more often now than before the send. It counts
  occurrences rather than comparing positions: a TUI redraws its screen
  and old lines leave the scrollback, so neither position nor presence
  tells an old line from a new one.
- `compaction::policy::INSTRUCTION_IDS` gives each `RowState`, in its
  declaration order, a stable briefing instruction id (TRF-08), such as
  `ctx-compact-lost` and `ctx-no-session`. The briefing keys its
  instruction for a state by this id, so an id never changes once
  published. `ok` has no instruction.
