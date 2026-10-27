# 04: Workers record their PIDs, session and heartbeat

**What to build:** Every worker the orchestrator spawns has a `workers` row
from the first moment, and the row always says which processes it is and
when they were last heard from.

- `horch spawn` writes the row (`spawning`, persona, role, harness, model,
  effort, host, workspace, pane) and passes `HORCH_WORKER_ID` to the pane.
  A spawn with no task text gives the worker the fixed task
  "Run horch ledger claim."
- `horch worker` writes `supervisor_pid` and `supervisor_started`
  (`starting`). It starts the agent as a child it holds (not a call that
  hides the PID), and writes `agent_pid` and `agent_started` at once.
- While the agent runs, a heartbeat thread writes `last_checkin_at = now`
  every `heartbeat_every_s`.
- As soon as the session id is known (minted or discovered), it writes
  `session_id` and `transcript_path`.
- When the agent exits, the worker writes `exit_code`, `ended_at` and the
  status (`done` after `ledger done`, else `incomplete` if it held a task,
  else `no_task` or `launch_failed`), then closes its own pane by its exact
  pane id and writes a `stops` row with `requested_by = self`.
- `horch fleet` writes the orchestrator's own row (`kind = orchestrator`)
  and sets it as the run's `orchestrator_worker_id`. (Ticket 03 creates
  the run row.)

The JSON session ledger keeps being written as today; this ticket adds the
SQLite rows beside it.

**Blocked by:** 02 (The task ledger store)

**Status:** ready-for-agent

**Persona:** primary `opus`, fallback `backend-developer`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.6, 5.2 steps 1 and 2, 5.3

- [ ] A test with a fake agent: the row has both PIDs, both start times
      match `procid::start_time`, and `last_checkin_at` advances at least
      twice while the agent runs.
- [ ] The heartbeat stops within 1 interval after the agent exits.
- [ ] `session_id` is written for a minted harness before the agent's first
      turn, and for a discovered harness within the discovery window.
- [ ] `transcript_path` names an existing file for Claude and Codex fakes.
- [ ] The `claim` fallback that creates a missing `workers` row (ticket 03)
      is removed; `claim` without a row is an error.
- [ ] The pane close at exit uses the public pane id from the row; the
      startup-failure path no longer passes herdr's internal id to
      `pane close`.
- [ ] No `ANTHROPIC_API_KEY` reaches the agent (existing `FORBIDDEN_ENV`
      tests still pass).
