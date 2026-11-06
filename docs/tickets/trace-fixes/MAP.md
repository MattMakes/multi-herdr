# Map: trace fixes

Spec: [docs/specs/trace-fixes.md](../../specs/trace-fixes.md)

| # | Title | Persona | Parallel | Wave | Blocked by |
|---|---|---|---|---|---|
| 01 | [Verify the suspected behaviours and measure harness-adapter use](01-verify-suspects.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 02 | [Remove competitive dataset rounds](02-remove-dataset.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 03 | [One schema migration for every new field](03-schema-next.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 04 | [Publish the ticket contract and make the writers produce it](04-ticket-contract.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 05 | [Mail ids on every line, and a courier that stays while mail is pending](05-mail-ids-courier.md) | primary `opus-hardening`, fallback `opus` | yes | 1 |  |
| 06 | [Land: reset after a red check, recover a timed-out lock](06-land-reset-lock.md) | primary `opus-hardening`, fallback `opus` | yes | 1 |  |
| 07 | [complexity-sweep covers Rust](07-rust-complexity.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 08 | [Compaction: show every state, and 1 cycle before handoff](08-compaction-single-cycle.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 09 | [doctor warns about stale ambient herdr skills](09-doctor-stale-skills.md) | primary `opus`, fallback `backend-developer` | yes | 1 |  |
| 10 | [add-map reads the contract, dry-runs, snapshots and refuses duplicates](10-ticket-parser-snapshot.md) | primary `opus`, fallback `backend-developer` | yes | 2 | 03, 04 |
| 11 | [One 'run settled' predicate](11-run-settled.md) | primary `opus`, fallback `backend-developer` | yes | 2 | 03 |
| 12 | [Run default check, no silent no-check, shared diff base](12-run-check-default.md) | primary `opus`, fallback `backend-developer` | yes | 2 | 03, 06 |
| 13 | [ledger run gate, and close guards](13-run-gate-close.md) | primary `opus`, fallback `backend-developer` | yes | 3 | 11 |
| 14 | [Wakes through a transactional outbox](14-durable-wakes.md) | primary `opus-hardening`, fallback `opus` | yes | 3 | 03, 05, 11 |
| 15 | [Watchdog: 30 s reconcile and re-wake](15-fast-reconcile.md) | primary `opus-hardening`, fallback `opus` | yes | 4 | 14 |
| 16 | [horch spawns code-sweep reviewers](16-reviewer-autospawn.md) | primary `opus`, fallback `backend-developer` | yes | 4 | 02, 14 |
| 17 | [Answers close 1 question by id](17-question-answers.md) | primary `opus`, fallback `backend-developer` | yes | 2 | 02, 05 |
| 18 | [Bounded, incremental, cheaper code review](18-review-loop.md) | primary `opus`, fallback `backend-developer` | yes | 3 | 03, 10 |
| 19 | [Board and claim dead spots](19-board-claim.md) | primary `opus`, fallback `backend-developer` | yes | 3 | 10 |
| 20 | [Spawn: resume claims, and runs stay in their workspace](20-spawn-fixes.md) | primary `opus`, fallback `backend-developer` | yes | 5 | 02, 13, 16 |
| 21 | [Park unlanded work instead of cancelling it](21-park-unlanded.md) | primary `opus`, fallback `backend-developer` | yes | 4 | 12, 19 |
| 22 | [cancel and abandon stop the worker](22-cancel-stops.md) | primary `opus-hardening`, fallback `opus` | yes | 5 | 21 |
| 23 | [horch ledger handoff](23-ledger-handoff.md) | primary `opus`, fallback `backend-developer` | yes | 6 | 17, 22 |
| 24 | [Respawn a dead orchestrator, rarely](24-orchestrator-respawn.md) | primary `opus-hardening`, fallback `opus` | yes | 7 | 15, 23 |
| 25 | [Record spawn-gate decisions](25-spawn-gate-record.md) | primary `opus`, fallback `backend-developer` | yes | 6 | 20 |
| 26 | [horch ledger task: a cheap one-off](26-one-off-task.md) | primary `opus`, fallback `backend-developer` | yes | 6 | 20 |
| 27 | [Retire horch assign and task-text spawns](27-retire-push.md) | primary `opus`, fallback `backend-developer` | yes | 7 | 17, 25, 26 |
| 28 | [Every session reader reads the task ledger](28-sessions-from-ledger.md) | primary `opus`, fallback `backend-developer` | yes | 8 | 08, 09, 24, 27 |
| 29 | [Stop writing the JSON session ledger](29-retire-json-ledger.md) | primary `opus`, fallback `backend-developer` | yes | 9 | 28 |
| 30 | [Rewrite the briefings and docs for the new behaviour](30-briefing-rewrite.md) | primary `opus`, fallback `backend-developer` | yes | 10 | 07, 18, 29 |
| 31 | [Review: delivery, supervision and respawn](31-review-supervision.md) | primary `codex-reviewer`, fallback `architect-reviewer` | yes | 8 | 17, 24 |
| 32 | [Review: session ledger retirement](32-review-ledger-retirement.md) | primary `codex-reviewer`, fallback `architect-reviewer` | yes | 10 | 29 |
| 33 | [Full gate on the run branch](33-final-gate.md) | primary `opus`, fallback `backend-developer` | yes | 11 | 30, 31, 32 |
| 34 | [Remove the dataset leftovers that ticket 02 found](34-dataset-leftovers.md) | primary `opus`, fallback `backend-developer` | yes | 2 | 02 |
| 35 | [A dev build cannot migrate the shared ledger, and a failing checkin does not flood the pane](35-ledger-guard-checkin-flood.md) | primary `opus-hardening`, fallback `opus` | yes | 2 |  |
| 36 | [Remove the rusqlite re-export from the outbox (arc_25)](36-outbox-shim.md) | primary `opus`, fallback `backend-developer` | yes | 3 |  |
| 37 | [Fix the 4 horch-e2e spawn and claim tests that fail on the run branch](37-e2e-spawn-regressions.md) | primary `opus`, fallback `backend-developer` | yes | 3 |  |
| 38 | [A light reviewer falls back to the other vendor's light reviewer](38-light-reviewer-fallback.md) | primary `opus`, fallback `backend-developer` | yes | 4 |  |
| 39 | [Round 2-3 review tasks queue with the other vendor's light reviewer as fallback](39-round2-fallback.md) | primary `opus`, fallback `backend-developer` | yes | 5 | 38 |
| 40 | [The pr step waits for GitHub's PR head instead of failing on a fresh push](40-pr-head-lag.md) | primary `opus`, fallback `backend-developer` | yes | 5 |  |
| 41 | [The outbox drain releases its lock explicitly (flaky trf_21 tests)](41-outbox-lock-release.md) | primary `opus-hardening`, fallback `opus` | yes | 5 |  |
| 42 | [An orchestrator launch claims its run: workspace recorded, stop cleared](42-orchestrator-launch-claims-run.md) | primary `opus-hardening`, fallback `opus` | yes | 8 | 24 |
| 43 | [run new refuses a 2nd active run in a workspace](43-run-new-one-per-workspace.md) | primary `opus`, fallback `backend-developer` | yes | 8 | 27 |
| 44 | [Respawn survives a crash before launch, and a losing launch keeps the live route](44-respawn-launch-pending.md) | primary `opus-hardening`, fallback `opus` | yes | 9 | 31 |
| 45 | [An outbox drain never makes duplicate mail](45-idempotent-drain.md) | primary `opus-hardening`, fallback `opus` | yes | 9 | 31 |
| 46 | [The watchdog reconciles every wake that a crash after commit can lose](46-wake-reconcilers.md) | primary `opus-hardening`, fallback `opus` | yes | 10 | 44, 45 |
| 47 | [An auto-spawned reviewer claims the review it was spawned for](47-reviewer-claim-target.md) | primary `opus`, fallback `opus-hardening` | yes | 9 | 31 |
| 48 | [Fix the 8 tests that fail on the run branch](48-run-branch-red-tests.md) | primary `opus`, fallback `backend-developer` | yes | 10 | 27 |
| 49 | [No internal code path types a task into a pane](49-no-typed-task-internal.md) | primary `opus`, fallback `backend-developer` | yes | 10 | 27 |
| 50 | [horch register cannot steal the orchestrator route, and a pending orchestrator does not show as stale](50-orchestrator-route-lock.md) | primary `opus-hardening`, fallback `opus` | yes | 11 | 44, 46 |
| 51 | [The dependency rule knows the horch rusqlite dependency, and a git spawn error does not abort run close](51-deps-rule-gate-close.md) | primary `opus`, fallback `backend-developer` | yes | 11 | 48 |
| 52 | [A plain spawn prefers the planned run over the steward's solo run](52-spawn-prefers-planned-run.md) | primary `opus`, fallback `backend-developer` | yes | 11 | 49 |
| 53 | [No label or diagnostic names the retired JSON session ledger](53-json-path-labels.md) | primary `opus`, fallback `backend-developer` | yes | 12 | 29 |
| 54 | [A skipped run claim refuses the orchestrator launch](54-skipped-claim-refuses-launch.md) | primary `opus-hardening`, fallback `opus` | yes | 12 | 32, 50 |
| 55 | [A refused fleet launch leaves no orchestrator route behind](55-refused-launch-drops-route.md) | primary `opus-hardening`, fallback `opus` | yes | 13 | 54 |
| 56 | [Merge PR #338 (skill-intake) into the run branch](56-merge-skill-intake.md) | primary `opus-hardening`, fallback `opus` | no | 14 | 33 |
| 58 | [The spawn_only controller is identified by its recorded process, intake roles run on Opus](58-intake-identity-model.md) | primary `opus-hardening`, fallback `opus` | no | 15 | 56 |
| 57 | [Full gate on the run branch after the PR #338 merge](57-regate-after-merge.md) | primary `opus`, fallback `backend-developer` | no | 16 | 56, 58 |

## Waves

- Wave 1: 01, 02, 03, 04, 05, 06, 07, 08, 09
- Wave 2: 10, 11, 12, 17, 34, 35
- Wave 3: 13, 14, 18, 19, 36, 37
- Wave 4: 15, 16, 21, 38
- Wave 5: 20, 22, 39, 40, 41
- Wave 6: 23, 25, 26
- Wave 7: 24, 27
- Wave 8: 28, 31, 42, 43
- Wave 9: 29, 44, 45, 47
- Wave 10: 30, 32, 46
- Wave 11: 33

## Critical path

03 → 10 → 19 → 21 → 22 → 23 → 24 → 28 → 29 → 30 → 33

## Shared areas

Tickets whose `Own` paths overlap are ordered by `Blocked by`; the ledger also serialises them by `Own`.

## Open questions

None.
