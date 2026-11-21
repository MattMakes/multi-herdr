# horch done finishes inside the agent's 120 s command timeout

Claude Code kills an agent's command with SIGKILL at 120 s (P18).
`horch done` finishes inside that timeout whatever state the orchestrator
is in (#67). Before #67, `done` waited 120 s for an agent at status
`unknown`, an agent that is still starting, and Claude Code killed `done`.
`done` now lets all its pushes, the report and the ledger's wake lines,
take 20 s in total (`DONE_PUSH_BUDGET` in
`crates/horch-core/src/messaging/delivery.rs`). `done_cmd`
(`crates/horch/src/cmd/taskledger.rs`) uses that 1 budget for the wakes
and the report push.

## Consequences

- `ENTER_CALLS` keeps 1 s of the budget for the herdr Enter calls after
  typing. The push deadline ends a herdr call, and an Enter cut off leaves
  typed text in the pane that joins the next line typed there. So it
  covers 2 calls on a loaded machine, not a quick one.
- Typing needs its full settle, or a busy TUI splits the line.
  `send_line_with` keeps the longer of the settle and the second-Enter wait
  after the tail poll, plus `ENTER_CALLS`. With less budget left it types
  nothing, and the line waits in the mail.
- The acceptance limit for `done` is 30 s. The e2e test
  `crates/horch-e2e/tests/done_budget.rs` keeps an agent at status
  `unknown` in the orchestrator's pane for as long as it runs. The test
  makes `done` stop at a fault point and checks a bound that a command
  that spent the budget and stopped at its end stays under. The real 20 s
  budget takes longer, so the bound fails if the fault point is ignored.
- `horch tell` waits up to `TELL_GRACE` (30 s) for an agent to show in a
  freshly registered orchestrator's pane (U-70). A worker gives its agent
  `HORCH_TELL_GRACE_MS=0`, so the agent's `horch done` report does not
  wait and the pane closes at once. Before commit `2792508`,
  `horch smoke fleet` waited 34 s for it.
- The e2e harness sets `HORCH_TELL_GRACE_MS=0`, because a fake pane never
  shows an agent. `tests/tell_grace.rs` sets it empty, which is the default
  grace, against an orchestrator that is a plain shell (fake-herdr
  `shell`) and registered seconds ago.
