# 08: Spawn and fleet warn about a live orchestrator and high host load (P08, P15)

**Issue:** [#50](https://github.com/MattMakes/multi-herdr/issues/50) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core --test preflight --test taskledger_queue && cargo test -p horch-e2e --test taskledger --test e2e`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** `spawn` and `fleet` print no live-orchestrator or load warning. The machine snapshot gained only a host name; it still has no load average.
- **There is now a second source of live orchestrators:**
  - The task ledger keeps runs per project, each with its orchestrator, workspace and status.
  - Projects are keyed by the main worktree root, so a run is seen from every worktree of the repo.
  - More than 1 active run per project is allowed when their slugs differ.
  - A run exists only after `horch ledger run new`. A plain `horch fleet` has only the old JSON session-ledger orchestrator record.
- **Check both sources.**
  - An active task-ledger run in this project whose workspace is not the caller's.
  - A live JSON-ledger orchestrator record whose workspace is not the caller's.

  Either one counts only if herdr still lists its workspace. Abandoned runs stay `active` in the ledger.
- **Tests to extend:** the task-ledger end-to-end and queue tests, plus the existing orchestrator-record tests.

## What to build

Before work collides or the host overloads, horch says so as a fact.

- **Live orchestrator.** It is live as defined above. `horch fleet` prints 1 warning naming its workspace (and its run slug for a ledger run). `horch spawn` prints the same as a `NOTE:` line. Neither one refuses.
- **Host load.**
  - The machine snapshot gains the 1-minute load average. On Windows it is absent and no note prints.
  - `horch spawn` prints a `NOTE:` line when the load is at least the CPU count, with both numbers.
  - The load can be pinned through the machine fixture for tests.

## Acceptance criteria

- [ ] With an active task-ledger run in another listed workspace, `fleet` warns and `spawn` prints `NOTE:` naming it; both still succeed.
- [ ] The same with only a JSON-ledger orchestrator record in another listed workspace.
- [ ] A run or record whose workspace herdr no longer lists produces no warning.
- [ ] The caller's own orchestrator or run produces no warning.
- [ ] With the machine fixture's load at or above its CPU count, `spawn` prints the load `NOTE:`; below it, nothing.
- [ ] The machine fixtures and their strict parsing accept the new field.

## Blocked by

None (can start immediately)
