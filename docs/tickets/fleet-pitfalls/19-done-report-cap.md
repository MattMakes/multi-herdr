# 19: horch done never outlives the agent's command timeout (P18)

**Issue:** [#67](https://github.com/MattMakes/multi-herdr/issues/67) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test messaging --test tell_grace --test taskledger --test lifecycle`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## Background

This was deferred to the task-ledger spec (#28), which closed without it. v0.3.0 did not fix it, and the ledger path can be slower.

Today, `horch done` (and `horch ledger done`, which ends by calling it in a pane) reports through the normal tell path. That path is:

- up to 12 s for the channel relay;
- then up to 120 s of readiness wait while the orchestrator's agent status is unknown;
- then up to 15 s waiting for the message tail.

That totals about 150 s, which is longer than Claude Code's 120 s command timeout. The agent kills `horch done` with SIGKILL (exit 137). The record then stays not done, and the pane stays open.

`horch ledger done` can make up to 2 more tells before that report: "reviews are in", and the after-done wake. There is no end-to-end test of `done` from inside a pane against a slow orchestrator.

## What to build

`horch done` finishes well inside 120 s, every time, whatever state the orchestrator is in.

- It prints a 1-line "closing: reporting to the orchestrator" first, so the worker sees progress at once.
- The report and any wake lines are enqueued as durable mail first (as today). The push to the orchestrator is capped at a short total budget, about 20 s for all tells in 1 `done`. When the budget runs out, the push stops and the courier delivers.
- The ledger and session records are marked done before the best-effort push, so a killed push never leaves the work not done.
- The pane closes as today.

## Acceptance criteria

- [ ] End-to-end: with a fake-herdr orchestrator pane stuck at agent status `unknown`, `horch done` from a worker pane exits 0 in under 30 s. The report is pending mail, and the record is done.
- [ ] The same for `horch ledger done` with a finished task: both wake lines are pending mail, and the total time is under 30 s.
- [ ] The first output line is the closing line.
- [ ] A `done` retried after a kill during the push records no second done (existing idempotence holds).
- [ ] With an idle orchestrator, the report is still typed at once (no regression of the current tell-grace test).

## Blocked by

None (can start immediately)
