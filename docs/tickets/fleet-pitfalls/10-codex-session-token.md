# 10: Codex session found by a token unique to its launch (P10)

**Issue:** [#52](https://github.com/MattMakes/multi-herdr/issues/52) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core codex && cargo test -p horch-e2e --test lifecycle --test taskledger`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

**Still present, and it now does more harm.**

- Codex rollout discovery is unchanged. It still matches by working folder and launch time, newest first.
- The task-ledger supervisor does not discover sessions itself. It copies the session id and transcript the launch wrote to the session ledger, and it only fills empty columns. So a wrong first match is never corrected.
- **The watchdog uses transcript growth as progress.** Another worker's rollout can keep a stalled worker looking alive, or make a live one look stalled.
- **A replacement worker** is handed the dead worker's transcript path, which can be the wrong one.
- **Second fix option:** the agent's PID is now recorded at launch. Matching the rollout file that the agent process has open is a second, stronger way to tie a session to a launch. Either fix works; the launch token stays the recommended one, because it also works after the process exits.
- **Test to extend:** the task-ledger end-to-end test that checks a spawned worker records its PIDs, session and checkins.

## What to build

Codex workers spawned together in one folder never take each other's session ids.

- Each launch puts a token unique to its record into the first prompt.
- Session discovery matches that token in the rollout's first user message, in addition to the working folder and launch time.
- Discovery also used by `done` matches the same token.
- When herdr reports the pane's session id itself, that still wins.
- The fake Codex writes a per-launch session id and its prompt into its rollout, so the race can be tested.

## Acceptance criteria

- [ ] 2 fake Codex workers spawned in 1 folder in the same second each get their own session id in both the session ledger and the task ledger, whichever polls first.
- [ ] A rollout with the right folder and time but another launch's token is never taken.
- [ ] Each task-ledger worker's recorded transcript path is its own rollout.
- [ ] Compaction and resume of each worker use its own session.
- [ ] The existing single-worker discovery and lifecycle tests still pass.
- [ ] Any briefing golden change is in a named sanctioned block.

## Blocked by

None (can start immediately)
