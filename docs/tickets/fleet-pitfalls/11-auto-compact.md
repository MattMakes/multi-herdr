# 11: Compact automatically when an over-threshold worker writes its handoff (P12)

**Issue:** [#53](https://github.com/MattMakes/multi-herdr/issues/53) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core --test compaction && cargo test -p horch compact && cargo test -p horch --test context_cli`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** The compaction code is unchanged.
- **New gap:** `horch note` still runs the context check (warn once over the threshold). But ledger workers are now told to report with `horch ledger progress`, and `horch ledger progress` and `horch ledger note` never run that check. So a ledger worker may never see the context warning, and this ticket's trigger would never fire for it.
- **Watchdog:** no direct interaction. Compaction runs in place, so the agent does not exit and the task is not requeued. But a compaction is not recorded as ledger progress.

## What to build

A worker over its context threshold is warned, and is compacted without waiting for the orchestrator to see `COMPACT-READY`.

- `horch ledger progress` and `horch ledger note` run the same context check as `horch note`. The check still needs the worker's session-ledger record, which the brief still sets.
- When `horch note`, `horch ledger note` or `horch ledger progress` records a handoff note (its text names the role's handoff path) and the record is over its threshold with an ask or warning on record, horch schedules compaction itself.
  - It uses the same path as `horch compact <role>`: the same freshness and route checks, and the same detached job that waits for the worker to be idle.
- horch tells the orchestrator in 1 line that compaction started for that role.
- A finished compaction is recorded as progress in the task ledger, so the watchdog does not count it as a stall.
- A worker below the threshold, or with no ask on record, is not compacted.
- The `COMPACT-READY` line stays valid. A second request while a job is running is a no-op.
- This adds the first end-to-end test of compaction.

## Acceptance criteria

- [ ] End-to-end: a worker over the threshold with a warning on record runs `horch ledger progress "handoff: <path>"` and ends its turn. The fake agent then gets the compact command and the resume line. The session ledger records the compaction, and the task ledger records progress. The orchestrator gets 1 line.
- [ ] `horch ledger progress` over the threshold prints the context warning once, like `horch note`.
- [ ] The same note below the threshold starts nothing.
- [ ] A stale or missing handoff file is refused, as `horch compact` refuses it, and the worker sees why.
- [ ] A later `horch compact <role>` from the orchestrator while the job runs does not start a second job.

## Blocked by

None (can start immediately)
