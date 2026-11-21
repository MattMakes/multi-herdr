# A spawn_only controller is identified by its pane and process

`horch spawn` accepts task text only as the 1 exception to TRF-43: a
`spawn_only` controller sends a brief to a role whose pane has
`horch_access: report`. A report pane's socket takes only progress, tell
and done, so it cannot run `horch ledger claim`, and the brief is its task.
Every other task text is refused (`spawn::check_task_text`). The controller
identity comes only from `spawn_only_check`: the caller's registered pane,
its horch-written brief and that teammate's globs, never a flag or an
environment variable.

## Consequences

- `spawn::Caller` is the role registered on the current pane, in the
  pane's own workspace, and the teammate its brief resolved. `HORCH_ROLE`
  is not read, because any worker can set it. No pane, no registered role
  or no readable brief means no caller (the orchestrator, an operator
  shell).
- `spawn_only_check` refuses a name that matches no `spawn_only` glob,
  `--resume`, and a roster folder other than the one the caller's brief
  names. It returns the controller for the new worker's brief: the
  caller's role when it has `spawn_only`, else none.
- A brief also needs the caller to run inside the agent process horch
  recorded for the controller's ledger row (`runs_in`). `HERDR_PANE_ID`,
  which any worker can set, is not proof of identity (TRF-43).
- Claude Code's macOS sandbox blocks the full process-info query for its
  agent process and for the caller's ancestors. `spawn::Procs` falls back
  to sandbox-safe queries (`procid::parent_sandboxed`,
  `procid::is_same_sandboxed`) that still prove the agent's exact start
  time and walk the parent chain. Before that fallback the walk in
  `runs_in` stopped at 42 and refused valid briefs.
