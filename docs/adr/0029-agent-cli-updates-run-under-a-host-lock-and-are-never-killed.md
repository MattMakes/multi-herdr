# Agent CLI updates run under a host lock and are never killed

`horch update`, and the step `horch fleet` and `horch orchestration` run
before their first pane starts, update the agent CLIs (ARC-32 to ARC-35,
`crates/horch-core/src/harness/update.rs`). Each CLI has its own update
command, and each command finds its own install method (native, Homebrew,
npm), so horch only runs it. The panes themselves keep their auto-update
switched off. Several fleets can start on 1 host, and each one runs this
step (#205). So there is 1 lock per CLI under `<default state root>/update/`,
which lets 1 updater of a CLI run at a time on the host. A second horch that
finds the lock held skips that CLI. An updater that passes its time limit is
not killed: a package manager stopped between the removal of the old build
and the link of the new one leaves no CLI. It is left to finish, and the
fleet start stops waiting for it.

## Consequences

- The update folder does not follow `HORCH_STATE_DIR`: a scratch fleet
  updates the same installed CLIs.
- The fleet step skips a CLI whose last update succeeded less than
  `FRESH_FOR` (24 hours) ago.
- While a left-running updater runs (at most `LOCK_STALE`, 3600 s), every
  later horch skips that CLI. An updater that ended unseen counts as the
  CLI's last check.
- `DEFAULT_UPDATE_TIMEOUT` in `crates/horch-core/src/runtime/context.rs`
  is the time limit of 1 updater when `HORCH_UPDATE_TIMEOUT_MS` is unset:
  180 s (180000 ms). A Homebrew `codex update` alone can take 60 s.
- `MAX_TIMEOUT` (1800 s), the longest horch waits for 1 updater, stays under
  `LOCK_STALE`, enforced by a const assert. So a later horch never breaks
  the lock of a horch that still waits.
- A CLI can change under a running fleet: a detached updater, or the
  updater of a later fleet start, replaces it. It is never left half
  installed.
- `run_updater` runs 1 updater with stdin null, no forbidden variable, in
  its own process group, with its output in log files. Files, not pipes:
  an updater left running at the timeout must not die of a closed pipe
  when horch exits. It is not killed either (#205). The e2e tests in
  `crates/horch-e2e/tests/update.rs` check that each updater starts as the
  leader of its own process group.
- The updater's `.running` mark is written as soon as it starts, so a
  horch that dies while it waits still leaves the mark. When
  `start_updater` cannot write the mark, it stops the updater's process
  group at once: an unmarked updater that outlives horch would let a later
  horch run a second one beside it. It is stopped before it can change
  anything.
- A Codex pane can write the state root. So the files under
  `<default state root>/update/` are written without following a link, and
  `UpdateFiles::read_mark` reads a mark file only when it is a regular
  file, without blocking and bounded in size.
- Every updater waits out the release cooldown first
  (`crates/horch-core/src/harness/cooldown.rs`).
- `update_bin` picks the program that updates a CLI from the one horch
  launches. `cpx` is a proxy that launches `claude` and has no `update`
  command, so a `cpx` launch updates `claude` from `PATH`: when the claude
  persona launches through `cpx`, the update runs `claude update`, not
  `cpx update`.
- herdr is not in the default list: `herdr update` can stop the herdr
  server, and that closes every pane. `run_herdr_update` runs it only on
  the operator's request, attached to the operator's terminal, with no time
  limit, because herdr can ask the operator whether to stop its server. The
  exit status is herdr's.
