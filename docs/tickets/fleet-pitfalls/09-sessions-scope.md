# 09: sessions and context show only your workspace; multi-line summaries indented (P16, P21)

**Issue:** [#51](https://github.com/MattMakes/multi-herdr/issues/51) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch --test baseline_cli --test context_cli && cargo test -p horch-core execution`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** `horch sessions` still exists and reads the JSON session ledger. Task-ledger workers also get JSON records there, so it lists them too.
  - It filters only by dataset round.
  - Its renderer still prints a multi-line summary with no indent.
- `horch context` still picks every live record in the project. It uses the caller's workspace only to find compaction jobs.
- `horch ledger status` is the board for 1 task-ledger run. It does not replace `sessions` for fleets without a ledger run, for resume candidates or for session ids. The old JSON plumbing commands moved under `horch ledger legacy`; `sessions` did not move.
- The orchestrator briefing still says to read `horch sessions` before every spawn, while its new task-ledger section says to spawn per `horch ledger plan-workers`. Reconciling that text belongs to #64; this ticket only fixes the output.

## What to build

`horch sessions` and `horch context` show only what the caller's fleet is doing.

- By default, both show only records from the caller's workspace (from the workspace id horch passes to every pane, or the caller's pane).
- `--all-workspaces` shows every fleet in the project. A caller with no workspace sees everything, as today.
- `--live` shows only live records. `--role <role>` shows only that role.
- A live record whose pane herdr no longer lists is marked `(pane gone)`.
- In `horch sessions`, the lines after the first in a multi-line `DONE` summary or task are indented under their row.
- When the caller's workspace has an active task-ledger run, `sessions` ends with 1 line pointing to `horch ledger status`.

## Acceptance criteria

- [ ] With records from 2 workspaces in one project, a caller in workspace A sees only A's rows; `--all-workspaces` shows both.
- [ ] `--live` hides done rows. `--role sonnet-2` shows only that role.
- [ ] A "working" row whose pane is gone shows `(pane gone)`.
- [ ] A 3-line summary renders as 3 lines, the last 2 indented under the row.
- [ ] The existing session oracles still match when run with no workspace (the oracle file count is unchanged).
- [ ] JSON output gets the same filtering, plus a field for the closed pane.

## Blocked by

None (can start immediately)
