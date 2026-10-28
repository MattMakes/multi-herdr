# 03: Inbox shows worker status and open questions; horch peek (P03)

**Issue:** [#59](https://github.com/MattMakes/multi-herdr/issues/59) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core --test messaging && cargo test -p horch-e2e --test messaging --test mail`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** `horch inbox` still shows only role, pane, closed-pane and channel marks. No `horch peek` exists, and nothing tracks open `QUESTION:`/`BLOCKED:` lines.
- **New code to build on:**
  - The task-ledger watchdog's observer already reads each worker pane's agent status, its state-change counter and its visible text, and parses the working timer. `horch peek` and the status column should use the same herdr pane reads.
  - `horch ledger status` has a "Live workers" section with ledger worker status, check-in age and progress age, but only for task-ledger runs, and with no herdr idle/working/blocked status. Decide whether `inbox` also shows a ledger worker's last watchdog verdict and its waiting state (#70). The recommendation is yes, 1 short column.
- **Open questions:** #70 defines and stores the "open question" record (a `QUESTION:`/`BLOCKED:` from a role with no later orchestrator `tell` to that role), in the messaging layer so that fleets without a ledger run get it too. This ticket shows that record. It does not define its own.
- The watchdog already wakes the orchestrator on a worker's first stall with a status question. Those lines appear in `inbox` as mail.

## What to build

The orchestrator sees which workers are idle, working or blocked, and which questions wait, without reading panes or building loops.

- `horch inbox` shows each role's agent status from herdr: idle, working, blocked, done, unknown, or pane closed. For a ledger worker, it adds the watchdog verdict and any waiting state.
- Under each role, `inbox` lists its open `QUESTION:` and `BLOCKED:` lines with their age, from #70's record.
- `horch peek <role> [--lines N]` prints the last N lines of that role's pane. It is read-only and never types.
- `horch wait` is not built: the orchestrator ends its turn and is woken by mail.

## Acceptance criteria

- [ ] End-to-end: with fake-herdr panes in idle, working and blocked states, `inbox` shows each status, and shows a closed pane as closed.
- [ ] A worker's `QUESTION:` appears as open with its age, also after `inbox` acknowledges it; it disappears after the orchestrator sends that role a `tell`.
- [ ] `--roles` (from #48) shows the same status and open questions.
- [ ] `horch peek <role> --lines 5` prints the pane's last 5 lines and types nothing; an unknown role exits non-zero with a clear message.

## Blocked by

- #48 (Pitfalls 02: inbox flags and output order)
- #70 (Pitfalls 22: the waiting state, which owns the open-question record)
