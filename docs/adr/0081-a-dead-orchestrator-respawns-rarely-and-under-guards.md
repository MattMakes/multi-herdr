# A dead orchestrator respawns rarely and under guards

`taskledger::orchestrator` respawns a dead fleet orchestrator, but rarely and
under guards (trace-fixes C7, D6). The orchestrator's pane launcher holds 1
lock per herdr workspace (`orchestrator::lock`) while its agent runs, so 2
orchestrators never run in 1 workspace. The launcher waits for its agent, so
the lock lives exactly as long as the agent. Each watchdog pass runs
`orchestrator::check`. A live lock holder is the agent PID of the run's
registered orchestrator, and `check` records it on the worker row. A recorded
PID that is gone, with no live lock holder, is a confirmed death.

## Consequences

- A confirmed death in an active run with no deliberate stop (`record_stop`)
  respawns the orchestrator: at most 1 per `RESPAWN_EVERY_S` (600 s) and
  `RESPAWNS_PER_RUN` (3) per run. The old row ends handed off to
  `HANDOFF_FILE` (`ai_docs/handoffs/orchestrator-whats-next.md`, the C4 path),
  and a new orchestrator row takes its place. The caller starts the agent
  with `first_instruction`.
- The new row is launch-pending (`spawning`) until a pass observes the lock
  holder, then it is `working`. A pending row with no lock holder did not
  start: a crash before the launch, or a failed launch. Each pass launches it
  again inside the same guards, and each launch counts 1 respawn (TRF-38).
- `orchestrator::launch_failed` still counts a respawn whose start failed, so
  a launch that always fails cannot loop. The row stays launch-pending until
  the last allowed respawn ends it `launch_failed`.
- A `spawning` orchestrator row waits for a watchdog pass to see the lock
  holder (TRF-39, ticket 44), so its old spawn time is not silence. The
  status board shows it as `launch pending`, never `STALE`. A worker row of
  the same age stays stale.
- `workers::hand_off` ends a live worker `handed_off` with its handoff file
  (trace-fixes C4). The attempt did not finish, so the row ends `incomplete`,
  as a reaped one does, with `end_reason` `handed_off`. `plan-workers` then
  names it for `--replaces`, so a successor picks up the task.
