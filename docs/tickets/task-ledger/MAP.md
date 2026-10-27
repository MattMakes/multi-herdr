# Map: the task ledger
Spec: [docs/specs/task-ledger.md](../../specs/task-ledger.md)

## Tickets
| # | Title | Blocked by | Wave | Areas | Persona (fallback) | Parallel |
|---|---|---|---|---|---|---|
| [01](01-procid-windows-identity.md) | Exact process identity on Windows | - | 1 | procid | backend-developer (opus) | yes |
| [02](02-ledger-store.md) | The task ledger store and `horch ledger status` | - | 1 | ledger store, ledger CLI, legacy ledger CLI | backend-developer (opus) | yes |
| [08](08-screen-timer-reader.md) | Read the agent's working timer from its pane | - | 1 | workspace observe | sonnet-feature (backend-developer) | yes |
| [03](03-queue-and-claim.md) | Queue tasks and claim them by persona | 02 | 2 | ledger store, ledger CLI, fleet recipe (run row), orchestrator wake | backend-developer (opus) | yes |
| [04](04-worker-identity-heartbeat.md) | Workers record their PIDs, session and heartbeat | 02 | 2 | spawn, worker lifecycle, harness launch, fleet recipe (orchestrator row) | opus (backend-developer) | yes |
| [05](05-deterministic-stop.md) | Stop 1 worker by its recorded PIDs, and ban greedy kills | 01, 04 | 3 | ledger CLI (stop), procid callers, briefings (1 line), ban test | opus-hardening (opus) | yes |
| [06](06-land-and-cleanup.md) | Land through the ledger and verify the worktree is gone | 03 | 3 | ledger CLI (worktree, land), vcs landing | backend-developer (opus) | yes |
| [07](07-progress-and-done.md) | Worker progress and done through the ledger | 04, 06 | 4 | ledger CLI (progress, note, done), messaging done/note | backend-developer (opus) | yes |
| [09](09-reap-and-replace.md) | Reap a dead worker and hand its task to a new one | 03, 05, 06 | 4 | ledger CLI (reap), claim output, spawn `--replaces` | opus (backend-developer) | yes |
| [11](11-import-and-unfinished.md) | Import the old ledgers and list work that never landed | 03, 06 | 4 | ledger import, ledger CLI (unfinished, reopen, dismiss), claim output | backend-developer (opus) | yes |
| [10](10-watchdog.md) | The watchdog: no worker works silently | 04, 08, 09 | 5 | watchdog, every-command hook, ledger status | opus-hardening (opus) | yes |
| [12](12-briefings-switch.md) | Switch the briefings to add, spawn, claim | 07, 09, 10, 11 | 6 | briefings, orchestrate skill, golden prompts, docs, `horch assign` | opus (backend-developer) | yes |
| [13](13-end-to-end.md) | End-to-end run on the fake herdr | 12 | 7 | e2e | qa-engineer (opus) | no |

Every persona is a Claude teammate: every ticket commits, and a Codex
sandbox cannot write `.git`.

## Waves
- Wave 1: 01, 02, 08
- Wave 2: 03, 04
  - Both touch the fleet recipe: 03 adds the run row, 04 adds the
    orchestrator row. The lines are separate; land 03 first if both are
    ready together.
- Wave 3: 05, 06
  - Both add a `horch ledger` subcommand. Each command lives in its own
    module; the dispatcher gets 1 line per command, in alphabetical order.
- Wave 4: 07, 09, 11
  - 09 and 11 both change what `claim` prints for a task with earlier work.
    Serialize them: 09 first, then 11 builds on its "previous attempts"
    block.
- Wave 5: 10
- Wave 6: 12
- Wave 7: 13

## Critical path
02 -> 04 -> 05 -> 09 -> 10 -> 12 -> 13

## Scope notes
- The JSON session ledger stays. `horch sessions`, `horch cost`, telemetry
  and the dataset competition keep reading it. The SQLite ledger is written
  beside it and is the source of truth for tasks, liveness, stops and
  worktrees. Moving those readers to SQLite is a later expand-contract
  spec, after ticket 11 has imported the history.
- `horch ledger` now names the task ledger. The old plumbing lives under
  `horch ledger legacy` (ticket 02).

## Open questions
None.
