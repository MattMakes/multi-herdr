# 10: The watchdog: no worker works silently

**What to build:** A horch watchdog process, 1 per run, that checks every
`working` worker every `check_every_s` and acts on what it finds, so a
worker that stopped is found within 30 minutes, not days later.

- It starts with the run (`horch fleet`), runs single-instance by lock, and
  writes its own PID, start time and `watchdog_checkin_at` on the run row.
- Every `horch` command checks `watchdog_checkin_at`. When it is older
  than 2 x `check_every_s` and a task is `running`, the command restarts the
  watchdog and prints a warning.
- Each check writes 1 `checks` row and applies the rules of spec section
  5.3 in order: process gone means `dead` at once; agent progress; proxy
  checkin from transcript growth, herdr's `state_change_seq` or a screen
  timer that ran the whole interval (ticket 08); suspect when the timer ran
  less than the clock, verified at once; else `no_progress`.
- The first `no_progress` wakes the orchestrator to ask the worker for an
  update. At `stall_rounds` it runs `ledger reap` (ticket 09).
- It wakes the orchestrator only when a worker's verdict changes.
- Every check also calls `retry_cleanup` (ticket 06).
- `ledger status` marks a `working` worker with no `checks` row in the last
  2 x `check_every_s` as `UNWATCHED`.

**Blocked by:** 04 (Workers record their PIDs), 08 (Read the agent's working timer), 09 (Reap a dead worker)

**Status:** ready-for-agent

**Persona:** primary `opus-hardening`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.7, 5.3, 10

- [ ] The checks run on an injected clock; tests use the repo's existing
      clock abstraction, not real 10-minute sleeps.
- [ ] Case: the timer reads 10m then 20m, 10 min apart, with no ledger call
      gives `proxy_checkin` and resets `stall_rounds`.
- [ ] Case: the timer reads 10m then 15m, 10 min apart, the transcript is
      unchanged and herdr says `idle`, gives `no_progress`.
- [ ] Case: the agent PID is gone gives `dead` and a reap on the same
      check.
- [ ] Case: 3 `no_progress` checks in a row give a reap; 2 then 1 progress
      check do not.
- [ ] A watchdog killed by test is restarted by the next `horch` command.
- [ ] 2 watchdogs for 1 run cannot both run.
