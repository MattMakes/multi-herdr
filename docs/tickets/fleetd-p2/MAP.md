# Map: fleetd build, Phase 2 (leases and capped waits)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of Phase 2 in task_execution, ideas and worker_profiles](01-phase2-modules.md) | T-92 | 1 |
| 02 | [The next schema version: the leases and questions tables, the new task and wait columns, and their backfill](02-schema-phase2.md) | T-92 | 1 |
| 03 | [The 12 task states as the stored words of tasks.status, with today's words in what you read](03-task-states-12-words.md) | 01, 02, 05 | 2 |
| 04 | [transition() has each exit of the 12-state task table, and I2 promotes pending tasks in the pass](04-task-exits-table.md) | 03 | 3 |
| 05 | [The Phase 2 keys of fleet.toml](05-fleet-toml-phase2-keys.md) | T-92 | 1 |
| 06 | [The profile of a persona: the liveness and retry numbers, the mode, and the required fallback](06-profile-liveness-and-fallback.md) | 01, 05 | 2 |
| 07 | [The wrapper heartbeat: the supervisor checks in every heartbeat_every_s, apart from the model](07-wrapper-heartbeat.md) | 03, 05 | 3 |
| 08 | [Leases: 1 lease for each held task, renewed only on earned activity, and the lease part of I1](08-leases.md) | 01, 02, 04, 05, 07 | 4 |
| 09 | [The stall ladder: the agent state of each attempt, and rungs 1 to 5](09-stall-ladder.md) | 01, 04, 05, 06, 07 | 4 |
| 10 | [Rung 6: a burned attempt waits for its backoff on the fleet clock, then failed, 1 replan, escalated](10-rung6-retry-backoff.md) | 06, 08 | 5 |
| 11 | [A usage limit: restart at once on the fallback of another provider, else waiting_quota until the pool resets](11-waiting-quota-and-fallback.md) | 09, 10 | 6 |
| 12 | [Capped waits: max_wait_s on the fleet clock, a question wait points at a live questions row, and an answer frees a blocked task](12-capped-waits-and-questions.md) | 03, 04, 06, 10 | 6 |
| 13 | [horch task requeue and horch task cancel; a cancel stops a live attempt before the task is cancelled](13-task-requeue-and-cancel.md) | 03, 08, 10, 11, 12 | 7 |
| 14 | [The pass of Phase 2: leases, the stall ladder, rung 6, quota, cancels and promotion in their order; the end of "3 checks of 600 s"](14-pass-wiring.md) | 08, 09, 10, 11, 12, 13 | 8 |
| 15 | [Make the Phase 0 red tests of liveness green: the idle claim, the usage limit and the machine sleep](15-p0-liveness-green.md) | 14 | 9 |
| 16 | [The e2e tests of Phase 2: a stopped worker is caught within 1 lease, retries are bounded, a cancel stops the process first, and 2 workers never hold 1 task](16-p2-leases-e2e.md) | 14 | 9 |
| 17 | [The release test of Phase 2: a held task with a live worker goes on after the update, with 1 lease](17-p2-release.md) | 14 | 9 |
| 18 | [Gate of Phase 2: each rule of the gate green, and the gate file](18-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17 | 10 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phase 1 (`docs/tickets/fleetd-p1/MAP.md`): this command passes when each test that is not on
the list of expected failures passes, and each test on the list fails for its listed reason. The
list is the files in `crates/horch-e2e/expected-failures/`. Ticket 15 makes the 3 red tests of
Phase 2 green (FM-3, FM-5, the fleet clock of a lease), removes their lines and owns
`crates/horch-e2e/tests/p0_liveness.rs` and `crates/horch-e2e/expected-failures/p0_liveness.txt`.

Each ticket's Check also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

## Waves

- Wave 1: 01, 02, 05
- Wave 2: 03, 06
- Wave 3: 04, 07
- Wave 4: 08, 09
- Wave 5: 10
- Wave 6: 11, 12
- Wave 7: 13
- Wave 8: 14
- Wave 9: 15, 16, 17
- Wave 10: 18

The tickets of 1 wave run at the same time: 01, 02 and 05; then 04 and 07; then 08 and 09; then 11
and 12; then 15, 16 and 17. Tickets 03 and 14 are `Parallel: no`: each changes many files of the
taskledger and the CLI, and runs with no other ticket of this map. So 06 runs before or after 03.
No 2 tickets that can run at the same time own the same file. Tickets that change the same file
run one after the other through their blockers:

- `crates/horch-core/src/taskledger/tasks.rs`: 03 -> 10 -> 12.
- `crates/horch-core/src/fleetd/task_execution/application/commands.rs`: 03 -> 08 -> 10 -> 11.
- `crates/horch-core/src/taskledger/schema.rs`: 02 -> 03.
- `crates/horch/src/cmd/recipes.rs`: 03 -> 11. `crates/horch/src/cmd/taskledger.rs` and
  `crates/horch/tests/w3_ledger_cli_pins.rs`: 03 -> 13. `crates/horch-core/src/taskledger/watchdog.rs`
  and `crates/horch/src/cmd/taskledger_watch.rs`: 03 -> 14.
- Ticket 01 makes each new module file of `crates/horch-core/src/fleetd/`, so that 06 and 08 to 13
  own different files.

Each ticket with no blocker in this map (01, 02, 05) is blocked by `T-92`, the gate of Phase 1.

## Critical path

02 -> 03 -> 04 -> 08 -> 10 -> 11 -> 13 -> 14 -> 15 (or 16, 17) -> 18

## Open questions

Q1 and Q23 are for the operator. Q2 to Q22 and Q24 are choices of the planner where the design
leaves a point open; each ticket follows them, and the orchestrator can change each one.

- **Q1. The full use of a pool, and the name of the replan limit (operator).** RR-11 needs the
  point at which a pool counts as limited. Choice: the key `pool_full_use` in `fleet.toml`, with
  the starting value 0.98: the value of today for the same fact (`exhausted_used`,
  `crates/horch-core/src/routing/policy.rs:75`, where today's routing calls a pool `exhausted`).
  **Operator: confirm 0.98, or give the value.** The design names the replan limit
  `replan_limit`; the key is `replan_limit_s` (1800), so the unit is in the name as in each key of
  Phase 1. Tickets 05 and 11.
- **Q2. 1 schema version for Phase 2.** Tickets 02 (tables, columns, backfill) and 03 (the stored
  task words) share 1 new version: the next number after `VERSION` in
  `crates/horch-core/src/taskledger/schema.rs`. 03 adds its step to the migration of 02. No ticket
  names the number.
- **Q3. The stored words and the shown words.** `tasks.status` stores the 12 words of the design.
  What you read keeps the words of today: `pending` and `ready` show as `queued`, `leased` and
  `running` as `running`, `verifying` as `landing`, `escalated` as `queued` with its hold.
  `--json` adds the field `state` with the stored word. The shown words change in Phase 5. Ticket
  03.
- **Q4. The backfill of the task words.** `queued` with `held_reason` -> `escalated`; `queued` with
  a dependency not met -> `pending`; each other `queued` -> `ready`; `landing` -> `verifying`.
  `running`, `done` and `cancelled` keep their word. Ticket 03.
- **Q5. The claim writes `running`.** A worker of today claims its own task, so the claim goes from
  `ready` to `running` with its lease. `leased` (fleetd chose a worker that has not started) is
  used from Phase 4, with push dispatch. The table has its exits now. Tickets 03, 04 and 08.
- **Q6. The wrapper heartbeat is the supervisor checkin.** Phase 2 has no fleet-run wrapper. The
  checkin thread of today's supervisor (`horch worker`) writes `workers.last_checkin_at` every
  `heartbeat_every_s` (10 s), apart from the model. `heartbeat.json` of the wrapper is Phase 5.
  Ticket 07.
- **Q7. Activity.** Earned activity is today's `Observation` of the watchdog (output, commits, files,
  ledger writes). The new column `attempts.active_at` holds the fleet clock of the last activity,
  so the stall timers run on the fleet clock (PL-4). Tickets 02, 08 and 09.
- **Q8. The required fallback.** The fallback of a task is its `fallback_persona`, else the first
  entry of `fallbacks` of its persona. Each worker persona in `teammates/` gets a fallback of
  another provider where it has none: a `claude` persona -> `codex-sol`, a `codex` persona ->
  `opus`, each other -> `opus`. Exempt: the top-tier seats, `FLEET_ORCHESTRATORS`, and a persona
  with agent `none`. Ticket 06.
- **Q9. The usage-limit signal.** Before Phase 7 the signal is the pool reading of the persona: used
  share at or above `pool_full_use`, or a pool `State` that blocks. The CLI signal of the adapter
  is Phase 7. Tickets 09 and 11.
- **Q10. The mode and the numbers of a profile.** Each persona is `interactive` until Phase 7 (exec
  mode). The keys `liveness` and `retry` of a teammate file are optional and have the defaults of
  the design. With no `retry.max_attempts`, the run's `max_attempts` of today applies. Tickets 06
  and 12.
- **Q11. The cancel is crash safe.** The column `tasks.cancel_requested` holds a cancel whose stop is
  not yet seen. The next pass completes it (`complete_cancels`). Tickets 02, 13 and 14.
- **Q12. The owner of a failed task before Phase 6.** The orchestrator of the run gets 1 wake. A
  requeue of a `failed` task is its 1 replan. Tickets 10 and 13.
- **Q13. The questions rows before Phase 6.** `questions.feature_id` is the run id, with no foreign
  key; the owner is the orchestrator of the run. The question flow of RR-12 (escalation after 15
  min of a running sub-orchestrator) is Phase 6. Ticket 12.
- **Q14. The flags of "3 checks of 600 s".** `--check-every` and `--stall-rounds` of
  `horch ledger run new` stay accepted and stored. They no longer decide a reap. Ticket 14.
- **Q15. A usage limit ends the attempt.** The outcome `quota` ends the attempt and stops its
  process: the task restarts on a new worker of the fallback, or waits in `waiting_quota` with no
  process. The attempt burns nothing: `attempt_floor` goes up by 1. The Phase 0 test
  `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` asserted that the agent stays
  alive; ticket 15 changes that assertion. Tickets 11 and 15.
- **Q16. The tables of DDD Slice 2.** `ddd/08-code/implementation-plan.md`, Slice 2, names
  `verifications` and `profile_health`. The Phase 2 row of the design does not. The design wins:
  Phase 2 adds neither. Ticket 02.
- **Q17. The RR-1 e2e test is Unix only.** `kill -STOP` has no Windows form. On Windows the RR-1
  row rests on the unit tests of the heartbeat and the ladder (tickets 08 and 09). Tickets 16 and
  18.
- **Q18. Rung 4 before Phase 7.** An attempt that waits on a dialog ends with the outcome `stalled`,
  and 1 needs-you item holds the screen excerpt. The profile is marked `unhealthy` from Phase 7.
  Ticket 09.
- **Q19. The doorbell.** A constant text (`DOORBELL`), sent with the pane send of herdr of today.
  Tickets 09 and 14.
- **Q20. `verifying` has no lease.** The supervision of `horch ledger finish` of today stays. Tickets
  04, 08 and 14.
- **Q21. The release backfill.** Each held task with a live attempt gets a lease of 120 s on the
  fleet clock. Each open wait gets `due_at` = the fleet clock + 3600. Tickets 02 and 17.
- **Q22. `cancelled` is terminal.** The exit `cancelled -> ready` of today goes. A requeue of a
  cancelled task is refused with the reason of the door. This changes a behavior of today.
  Tickets 04 and 13.
- **Q23. The background wait: a deviation from the design text (operator).** The design keeps only
  waits with a live question or review behind them, so a background wait (`horch ledger progress
  "waiting: ..."`, 24 h today) would end. An interactive worker of today ends its turn while a long
  command runs, so with no background wait its lease ends during that command. The orchestrator
  chose option 1 (QUESTION #1574b85e). **Operator: confirm option 1, or choose option 2.**
  - Option 1 (in the tickets): keep the background wait while a child process of the worker's
    agent is alive (`group_members`, `crates/horch-platform/src/process.rs:57`), and at most
    `max_wait_s` on the fleet clock. When the child is gone, the wait ends at once and the stall
    ladder applies. `NotSupported` (Windows) keeps the wait to its cap. Phase 7 removes it with
    exec mode.
  - Option 2 (the design text): drop the background wait. A worker that runs a command longer than
    `stall_after` with no output is stalled.
  Tickets 01, 12 and 14.
- **Q24. Module files.** 1 file for each concern under `domain/`, `application/` and `adapters/`
  (more files than `ddd/08-code` lists), so that tickets of 1 wave own different files. Ticket 01.

## Coverage

Each mechanism of the row Phase 2 in "What each gate needs" and in "New work for each phase" of
the design, and each rule of the gate line, with the ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| The module files of Phase 2 | 01 |
| The `leases` table, the `questions` table, the new columns, the release backfill | 02 |
| The 12 task states as stored words | 03 |
| The 12-state table in `transition()`, the exit to `cancelled` from each state that is not terminal, I2 | 04 |
| The keys of `fleet.toml` of Phase 2 | 05 |
| The required `fallback` in the roster, the liveness and retry numbers of a profile | 06 |
| The wrapper heartbeat | 07 |
| The lease part of I1, renewal only on earned activity (RR-1, RR-2, FM-14) | 08 |
| The stall ladder, rungs 1 to 5 (replaces "3 checks of 600 s"; FM-3, FM-5) | 09 |
| Rung 6 with `not_before` on the fleet clock, the 1 replan, escalation (RR-6) | 10 |
| `waiting_quota` and the restart on a fallback of another provider (RR-11, FM-5) | 11 |
| `max_wait_s`, waits that point at a live question or review, the `questions` rows | 12 |
| `horch task requeue` and `horch task cancel`, the stop of a live attempt before `cancelled` | 13 |
| The pass of Phase 2, the end of "3 checks of 600 s" | 14 |
| Gate tests: the usage limit (FM-5), the idle claim (FM-3), the fleet clock of a lease | 15 |
| Gate tests: RR-1, RR-2, RR-6, the cancel tests, FM-14 | 16 |
| The release row of Phase 2 (a run in flight) | 17 |
| The gate file | 18 |
| The fleet clock, the jobs and the launcher | Phase 1 (no ticket) |
| The `needs_you` table | Phase 1 (no ticket) |
