# 12: Switch the briefings to add, spawn, claim

**What to build:** Orchestrators and workers use the task ledger as their
main channel, and the prompts say so.

- The orchestrator briefing and the `orchestrate` skill: queue every ticket
  with `horch ledger add` (persona, fallback, parallel, blockers, files,
  check), run `horch ledger plan-workers`, spawn that many workers with no
  task, then end the turn and wait to be woken. Use `horch ledger status`
  to decide the run is done. Quota is a warning to the operator only.
  Stop a worker only with `horch ledger stop`. Use `horch tell` for
  questions and answers only. Read `horch ledger unfinished` at the start
  of a run.
- The worker briefing: on start run `horch ledger claim` and read the
  ticket file it prints; on `no_task` send the printed line and end the
  turn; report with `ledger progress`; land with `ledger land`; finish
  with `ledger done`. A replacement reads the previous transcript in
  chunks first.
- `horch assign` prints that assignment is now `horch ledger add` plus a
  claim, and exits non-zero.
- `docs/fleet-workflow.md`, `docs/command-flow.md` and `README.md` describe
  the new flow.

**Blocked by:** 07 (Worker progress and done), 09 (Reap a dead worker), 10 (The watchdog), 11 (Import the old ledgers)

**Status:** ready-for-agent

**Persona:** primary `opus`, fallback `backend-developer`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 1, 5, 9

- [ ] The golden prompt tests are updated, and their diffs show only the
      new flow.
- [ ] Both briefings stay in Simplified Technical English.
- [ ] No briefing tells a worker to wait, poll or sleep.
- [ ] The ban test of ticket 05 passes on the new briefings.
