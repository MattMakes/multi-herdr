# 05: Stop 1 worker by its recorded PIDs, and ban greedy kills

**What to build:** `horch ledger stop <worker> "<reason>"` is the only way
to stop a worker. It reads the PIDs, start times and pane id from the
`workers` row, never from its arguments. It sends TERM to the agent PID,
waits up to 10 s, sends KILL if the same process still runs, does the same
for the supervisor PID, then closes the exact pane in herdr. Each step
writes 1 `stops` row with its result (`sent`, `already_gone`,
`not_same_process`, `refused_other_host`, `pane_closed`, `error`).
A row whose `host` is not this machine is refused.

A test makes greedy kills impossible to add by accident: it fails if
horch's Rust source, scripts, justfile, teammate briefings or bundled
skills contain `pkill`, `killall`, `taskkill /IM`, `kill` with a pattern or
`$(pgrep ...)`, or a kill decided by matching text. Test code that
exercises fakes is listed in 1 explicit allowlist with a reason per entry.
The worker and orchestrator briefings say to stop a worker only with
`horch ledger stop`.

**Blocked by:** 01 (Exact process identity on Windows), 04 (Workers record their PIDs)

**Status:** ready-for-agent

**Persona:** primary `opus-hardening`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.8, 7

- [ ] `stop` takes only a worker id; there is no flag that takes a PID or a
      pattern.
- [ ] A test with a recorded PID that has exited and been reused by another
      test process: `stop` sends nothing to it and records
      `not_same_process`.
- [ ] A test with an agent that ignores TERM: KILL follows after the grace
      period, and both `stops` rows exist.
- [ ] `stop` on a worker that already ended records `already_gone` and
      still closes its pane if the pane exists.
- [ ] The ban test fails on a planted `pkill -f worker` in a briefing and
      passes on the clean tree.
- [ ] Stopping 1 of 3 live fake workers leaves the other 2 running.
