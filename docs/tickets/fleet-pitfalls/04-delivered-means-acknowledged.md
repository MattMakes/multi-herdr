# 04: Worker mail is delivered until acknowledged (P01)

**Issue:** [#60](https://github.com/MattMakes/multi-herdr/issues/60) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test mail --test messaging --test channel --test taskledger`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

**Still needed. The task ledger made it more urgent.**

- **What has not changed:**
  - A Claude relay "ok" still returns delivered.
  - The courier still stops after 6 attempts (about 17.5 minutes), or after 10 minutes with no orchestrator.
  - It still types only into an idle orchestrator.
  - `done` still prints "no orchestrator" and drops the report when nothing is registered.
- **New:** every tell now goes through 1 entry point that takes an explicit sender. Task-ledger wakes use it, sending as `horch`, for:
  - after `done` (tasks became claimable, or the run is complete);
  - after a reap;
  - newly startable tasks;
  - the watchdog's first-stall question.

  So wakes are durable mail, and inherit the 6-attempt cap and the idle-only push.
- **A wake with no registered orchestrator and no relay is lost.** It is not enqueued, and is recorded only as a "wake not delivered" event.
- `horch ledger done` ends by calling the old `done`, so the "no orchestrator" drop also hits ledger workers.
- The task-ledger end-to-end tests turn wakes off, so no test covers wake delivery through the courier.

## What to build

A message to the orchestrator, from a worker or from horch, is never lost, and the sender is never told it arrived when it did not.

- **Delivered means acknowledged.**
  - A relay "ok" from a Claude orchestrator only means the notification was written.
  - `horch tell orchestrator` reports a durable message as "queued, not yet acknowledged", and says it will be redelivered until `horch inbox` reads it.
  - The Codex relay keeps its stronger confirmation, but its mail still stays pending until it is acknowledged.
  - The wake event records "queued" or "delivered" truthfully.
- **The courier keeps going.**
  - After its current early retries, it retries on a slow fixed interval for as long as mail is pending.
  - It exits only when nothing is pending, or when the orchestrator pane and the relay have both been gone for its grace period.
  - It still types only into an idle or done orchestrator.
- **No silence on a missing registration.**
  - When no orchestrator is registered, mail from workers and from `horch` is still enqueued under the workspace (from the task-ledger run's workspace for wakes).
  - `horch done` and `horch ledger done` still queue their reports.
  - The courier delivers once an orchestrator registers.

## Acceptance criteria

- [ ] End-to-end: a `tell` through a Claude relay prints "queued, not yet acknowledged", and the mail stays pending until `inbox`.
- [ ] With a pinned clock, the courier still redelivers to an idle orchestrator 2 hours after enqueue, and stops right after `inbox` acknowledges.
- [ ] With no orchestrator registered, `tell`, `done`, `ledger done` and a task-ledger wake all leave pending mail. A later registration gets it.
- [ ] 1 task-ledger end-to-end test runs with wakes on, and sees the after-done wake arrive through the courier.
- [ ] The existing mail, messaging, channel and task-ledger tests pass, updated only where the delivered/queued wording changed.
- [ ] The command-flow docs describe the new schedule.

## Blocked by

- #47 (Pitfalls 01: Mailbox registry in the state folder)
- #67 (Pitfalls 19: the capped `done` report, which changes the same tell path)
