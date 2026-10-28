# 01: Mailbox registry lives in the state folder (P09)

**Issue:** [#47](https://github.com/MattMakes/multi-herdr/issues/47) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-core && cargo test -p horch-e2e --test messaging --test mail --test channel --test brief --test startup_error`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** v0.3.0 did not change the mailbox registry's location or its resolve and register paths. They still use the temp folder.
- Durable mail and channel sockets already live in the state folder. Only the registry does not.
- The per-role brief files in the registry now also carry the worker id and the task-ledger path. The task ledger's own database is a separate path, and is not affected.
- More tests hard-code the temp-folder registry path than before. Besides the end-to-end messaging, mail, channel, worktree, brief, e2e, skills-exposure, lifecycle and startup-error tests, unit tests in mode, compact, execution lifecycle, the core messaging tests and the Prime context tests use it.

## What to build

The mailbox registry (role ids, briefs, launch markers, harvest logs) moves from the temp folder into horch's state folder, so the OS cannot delete it from under a fleet that sits idle for days. A fleet started before the upgrade still resolves its roles, because reads fall back to the old temp location. Writes go only to the new location.

## Acceptance criteria

- [ ] `horch register` and `horch spawn` write the registry under the state folder; nothing new is written under the temp folder.
- [ ] With the temp folder emptied after registration, `horch inbox` still lists the roles and `horch tell` still reaches them.
- [ ] A registry that exists only in the old temp location is still found by `inbox`, `tell`, `done` and `horch ledger done`.
- [ ] Existing tests that hard-code the temp-folder path move to the new path with unchanged behavior.
- [ ] No test or code reads `ANTHROPIC_API_KEY`.

## Blocked by

None (can start immediately)
