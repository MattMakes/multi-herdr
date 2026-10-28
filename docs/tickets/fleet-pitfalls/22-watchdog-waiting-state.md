# 22: The watchdog does not reap a worker that is waiting on reviewers or an answer (new in v0.3.0)

**Issue:** [#70](https://github.com/MattMakes/multi-herdr/issues/70) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core watchdog && cargo test -p horch-e2e --test taskledger --test messaging`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## Background

This is a new finding from checking the pitfall tickets against v0.3.0.

- The watchdog checks every worker in status `working` every 10 minutes. It declares a worker dead after 3 checks with no progress, about 30 minutes.
- No status is exempt.
- Today, the briefing tells a worker to end its turn in 2 cases:
  - while code-sweep reviewer tasks run (`finish` prints "waiting for reviewer tasks... End your turn");
  - after it sends a `QUESTION:`.

  An idle agent with no ledger call, no transcript growth and no running timer looks dead. So a worker waiting on slow reviews or on the operator is reaped and requeued, and its work is redone.
- The first stall wakes the orchestrator with a status question, and that is all.

This also blocks the "ask, then go on" and "background commands" rules in #63, which tell workers to end their turn.

## What to build

A worker that is waiting for something horch knows about is not counted as stalled.

- **Open questions.** Record each open question in the messaging layer, per workspace, so that fleets without a task-ledger run have them too. An open question is a `QUESTION:` or `BLOCKED:` that a role sent to the orchestrator, with no later orchestrator `tell` to that role, kept for the acked-mail retention (7 days). #59 shows these records in `horch inbox`, so define them once, here.
- **Waiting state.** A worker gets a waiting state with a reason and a deadline:
  - `finish` sets it when it returns waiting on reviewer tasks, until those tasks are done;
  - an open question from the worker sets it, until the orchestrator next tells that role;
  - `horch ledger progress "waiting: ..."` sets it for a background command, with a default deadline.
- **The watchdog** skips the stall count while the wait holds and its deadline has not passed. It still declares a gone process dead at once.
- When the wait ends (the reviews are in, or the reply is sent), the stall count starts fresh.
- `horch ledger status` shows the waiting workers, with their reason and age.

## Acceptance criteria

- [ ] End-to-end with a pinned clock: a worker waiting on reviewer tasks for 60 minutes is not reaped, and is reaped normally if it stays idle after the reviews are in.
- [ ] A worker that sent `QUESTION:` is not reaped while no reply has come, up to the deadline. After the orchestrator's reply, 3 idle checks reap it as today.
- [ ] A dead agent process is still dead at once, waiting or not.
- [ ] `ledger status` lists the waiting workers with their reason.
- [ ] The open-question record works in a fleet with no task-ledger run.

## Blocked by

None (can start immediately)
