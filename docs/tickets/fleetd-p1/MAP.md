# Map: fleetd build, Phase 1 (horch fleetd, supervised and level-triggered)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The fleetd module folders, their fitness tests and their rows in the architecture doc](01-fleetd-modules.md) | T-32 | 1 |
| 02 | [The Phase 1 concern files of horch-platform](02-platform-phase1-files.md) | T-32 | 1 |
| 03 | [Schema v8: the fleetd, attempts, jobs, pass_cursors and needs_you tables, the events columns, and the backfill of attempts](03-schema-v8.md) | T-32 | 1 |
| 04 | [The fleet root and the Phase 1 keys of fleet.toml](04-fleet-root-and-config.md) | 01 | 2 |
| 05 | [Platform: the fleetd lock and the awake clock](05-platform-lock-and-awake.md) | 02 | 2 |
| 06 | [Platform: the service definitions, install and restart through a service manager, and the systemd watchdog ping](06-platform-service.md) | 02 | 2 |
| 07 | [Platform: file watching as a speed-up](07-platform-file-watch.md) | 02 | 2 |
| 08 | [Platform: the transport of the command channel, with the peer user and the peer process](08-platform-channel.md) | 02 | 2 |
| 09 | [Platform: the count of processes and of open files of the user in the machine probe](09-platform-machine-counts.md) | T-32 | 1 |
| 10 | [The needs-you list: 1 item for each key, with a state](10-needs-you.md) | 01, 03, 04 | 3 |
| 11 | [transition(): the 1 door, its table of allowed transitions and its events](11-transition.md) | 10 | 4 |
| 12 | [Route the status writes of tasks.rs and workers.rs through transition()](12-transition-tasks-workers.md) | 11 | 5 |
| 13 | [Route the other status writes through transition(), and the audit rule of OR-2](13-transition-other-writers.md) | 12 | 6 |
| 14 | [The fleetd lock, the generation of the fleetd row, and the first start](14-fleetd-lock-generation.md) | 03, 04, 05, 11 | 5 |
| 15 | [The fleet clock: 1 count of seconds that stops while the machine sleeps and while fleetd is down](15-fleet-clock.md) | 05, 14 | 6 |
| 16 | [The 1 launcher for each process that fleetd starts](16-launcher.md) | 01, 11 | 5 |
| 17 | [The jobs table at work: job kinds, the run number, the bound on abandoned runs, and the job runner](17-jobs.md) | 04, 10, 11, 15, 16 | 7 |
| 18 | [The work budget of a pass, the cursors in pass_cursors, and the fact scans with their time budget](18-pass-budgets-and-scans.md) | 01, 03, 04 | 3 |
| 19 | [The health guards and pacing: holds on new dispatch, with their events](19-health-and-pacing.md) | 04, 09, 11, 13 | 7 |
| 20 | [The reap path of task-execution: I1 (process part), I4 and I5](20-task-reap-path.md) | 11, 13 | 7 |
| 21 | [The reconcile pass in 3 parts: facts, 1 transaction of rows, then the effects and the heartbeat](21-reconcile-pass.md) | 06, 07, 14, 15, 16, 17, 18, 19, 20 | 8 |
| 22 | [Move the duties of today's watchdog into the pass of fleetd](22-watchdog-duties-in-pass.md) | 21 | 9 |
| 23 | [horch fleetd, horch fleetd check, horch job requeue and horch job run; the fleetd check before each command](23-fleetd-cli.md) | 06, 19, 22 | 10 |
| 24 | [horch install and horch self-update restart fleetd; fleetd restarts itself on a changed binary](24-install-selfupdate-restart.md) | 23 | 11 |
| 25 | [Phase 1 stops the old watchdog: fleetd takes its runs, horch ledger watch points at fleetd](25-stop-old-watchdog.md) | 24 | 12 |
| 26 | [Make the Phase 1 red tests of liveness and of the herdr restart green, and add the e2e test of orphans](26-p0-liveness-and-rescue-green.md) | 25 | 13 |
| 27 | [The e2e tests of Phase 1 for jobs: a 5 min job keeps fleetd healthy; a job past its limit runs again, and after 3 abandoned runs goes to you](27-p1-jobs-e2e.md) | 23 | 11 |
| 28 | [The e2e tests of Phase 1 for the pass: 10,000 folders, missed file events, a fleetd killed mid-run, a crash after the commit](28-p1-pass-e2e.md) | 23 | 11 |
| 29 | [Gate of Phase 1: each rule of the gate green, and the gate file](29-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28 | 14 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phase 0 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is not on
the list of expected failures passes, and each test on the list fails for its listed reason. The
list is the files in `crates/horch-e2e/expected-failures/`. Each ticket that makes a red test of
Phase 0 green removes its line and owns the test file and the list file: 23 (FM-12), 24 (FM-34),
25 (FM-7), 26 (FM-4, the part of FM-13 of Phase 1).

## Waves

- Wave 1: 01, 02, 03, 09
- Wave 2: 04, 05, 06, 07, 08
- Wave 3: 10, 18
- Wave 4: 11
- Wave 5: 12, 14, 16
- Wave 6: 13, 15
- Wave 7: 17, 19, 20
- Wave 8: 21
- Wave 9: 22
- Wave 10: 23
- Wave 11: 24, 27, 28
- Wave 12: 25
- Wave 13: 26
- Wave 14: 29

The tickets of 1 wave run at the same time: 01, 02, 03 and 09; then 04 to 08; then 10 and 18; then
12, 14 and 16; then 13 and 15; then 17, 19 and 20; then 24, 27 and 28. No 2 tickets that can run
at the same time own the same file. Tickets that change the same file run one after the other
through their blockers (for example 22, 23, 24 and 25 on the `horch` CLI files and on
`crates/horch-e2e/expected-failures/p0_supervision.txt`). Ticket 01 makes each module file of
`crates/horch-core/src/fleetd/` and ticket 02 each concern file of `crates/horch-platform`, so that
later tickets own different files.

Each ticket with no blocker in this map (01, 02, 03, 09) is blocked by `T-32`, the gate of Phase P.

## Critical path

01 -> 04 -> 10 -> 11 -> 12 -> 13 -> 19 (or 20) -> 21 -> 22 -> 23 -> 24 -> 25 -> 26 -> 29

## Open questions

Q1 to Q3 are answers of the orchestrator. Q4 to Q21 are choices of the planner where the design
leaves a point open; each ticket follows them, and the orchestrator can change each one.

- **Q1. The safety floors of the health guards (OR-5).** Answer: starting values in `fleet.toml`,
  with 1 rule first: where the code of today has a threshold for the same fact, keep the value of
  today and name its source. Load: 1.0 for each CPU (`load >= cpus`,
  `crates/horch/src/cmd/spawn.rs:908`). Free disk 10 GB, free memory 1024 MB, process share 0.9,
  open file share 0.9 (no threshold today). Tickets 04 and 19.
- **Q2. The budgets.** Answer: starting values, each with its key in `fleet.toml`:
  `pass_time_budget_s` 20, `scan_budget_ms` 2000, `pass_budget` 200; `job_limit_s` 600 for
  `worktree_add`, `worktree_remove` and `worktree_list`, 3600 for `docs_sync` and `conformance`;
  `task_check` uses the check timeout of today, 20 min (`CHECK_TIMEOUT`,
  `crates/horch-core/src/taskledger/finish/land.rs:31`). Tickets 04, 17 and 18.
- **Q3. The schema version.** Answer: Phase 1 is schema v8, and each later phase takes the next
  number. From Phase 2 on, the schema numbers of the design text are 1 lower than the numbers of
  the code. Each ticket that names a schema version says so (ticket 03).
- **Q4. The fleet root and its files.** `$FLEET_ROOT`, else `<ledger dir>/fleet`. The lock
  `<root>/run/fleetd.lock`, the heartbeat `<root>/run/heartbeat.json`, `fleet.toml` at
  `<root>/fleet.toml`, the outboxes `<root>/tasks/<task>/attempts/<attempt>/outbox/`, the requests
  `<root>/run/requests/`. The workspace has no TOML crate today; ticket 04 adds `toml`.
- **Q5. What the 1 door covers in Phase 1.** `transition()` covers task (the `TaskStatus` of
  today), worker, attempt, job and needs-you item. Runs and worktrees keep their writes until
  Phases 4 and 3. The task table of 12 states is Phase 2.
- **Q6. Attempts.** The backfill writes the state `live` (the release table of the design says
  `running`; the CHECK of the `attempts` table has `launching`, `live` and `ended`). A claim writes
  a live attempt. The respawn of the orchestrator is an attempt in `launching` with a null
  `task_id`. No `idea_id` column until Phase 6.
- **Q7. Events.** Schema v8 rebuilds `events` with `entity`, `entity_id`, `from_state`,
  `to_state`, `cause` and `data_json`, and adds the `EventKind` `transition`.
- **Q8. The service mode.** fleetd uses the service manager of the OS only for the default ledger
  of the OS user (the home folder from the OS user database, not `$HOME`) and with no
  `FLEET_ROOT`. Else the mode is stand-in: files only; a restart stops by PID and start time,
  waits for the lock, and starts `horch fleetd` detached through the launcher. No test and no
  worker registers a real service on this machine.
- **Q9. Jobs in Phase 1.** A job process is `horch job run <id>` through the launcher. Phase 1 has
  an executor only for `task_check`. Phase 1 queues no job by itself; later phases queue them.
- **Q10. The old watchdog in tests.** A stand-in process (a `sleep` child) recorded as the lock
  owner and in `runs.watchdog_pid`.
- **Q11. `horch ledger watch`.** `--once` runs 1 fleetd pass in the foreground, with the JSON of
  today, when no fleetd holds the lock (many e2e tests use it). With no `--once` it points at
  fleetd. Ticket 25.
- **Q12. Orphans.** An orphan is a process or a pane that a `workers`, `attempts` or `jobs` row
  names and that no live attempt or job owns. fleetd never touches a process that no row names.
- **Q13. The fitness tests of Phase 1.** Domain imports, and foreign tables with these owners:
  `fleet_runtime` owns `fleetd`, `jobs`, `pass_cursors` and `events`; `task_execution` owns
  `tasks`, `task_deps`, `workers` and `attempts`; `ideas` owns `needs_you`. The 1 door
  (`fleetd/fleet_runtime/adapters/sqlite.rs`) is exempt.
  `fitness_only_fleet_runtime_opens_the_ledger_writable` is Phase 5.
- **Q14. Module files.** 1 file for each concern under `application/` and `adapters/` (more files
  than `ddd/08-code` lists), so that tickets of 1 wave own different files.
- **Q15. The machine probe.** The probe of Phase P has no count of user processes or open files.
  Ticket 09 adds them.
- **Q16. Dispatch points of Phase 1.** The holds of ticket 19 act on `horch spawn` and
  `horch ledger spawn`. Push dispatch by fleetd is Phase 4.
- **Q17. The CLI check.** Ticket 23 adds the check of `horch fleetd check` before each command at
  `crates/horch/src/main.rs:822`. Ticket 25 removes the lazy `ensure` start there, with the old
  loop and the tests that pin it. `HORCH_NO_WATCHDOG=1` keeps its meaning: no check and no start.
  The check never starts a fleetd that never ran.
- **Q18. Platform names.** The platform function names in these tickets are proposals. The module
  doc of each concern file of `crates/horch-platform` gives the final name.
- **Q19. The 5 min job test.** A `task_check` job with the check `sleep 300` (ticket 27).
- **Q20. The heartbeat.** `heartbeat.json` has `pid`, `pid_start`, `generation`,
  `awake` (`boot_id`, `awake_ms`), `at`, `exe` and `version` (ticket 21).
- **Q21. The release test of Phase 1.** The old-watchdog test of "What a release does to a run in
  flight" uses the stand-in of Q10 (ticket 25).

SR-7 becomes green in Phase 7. Phase 1 builds the launcher only (ticket 16).

## Coverage

Each mechanism of the row Phase 1 in "What each gate needs" and in "New work for each phase" of
the design, and each rule of the gate line, with the ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| Module layout of the 4 contexts and the fitness tests | 01 |
| The concern files of horch-platform for Phase 1 | 02 |
| Schema v8, its backup, the refusal of a newer schema, the backfill of attempts | 03 |
| The fleet root and the keys of `fleet.toml` | 04 |
| Platform: the fleetd lock (file lock) and sleep and wake (the awake clock) | 05 |
| Platform: service install and restart, the systemd watchdog ping | 06 |
| Platform: file watching | 07 |
| Platform: the transport of the command channel | 08 |
| Platform: the machine probe counts of processes and open files | 09 |
| The `needs_you` table (FM-44) | 10 |
| `transition()` and its events (OR-2) | 11 |
| The status writes of tasks and workers through `transition()` | 12 |
| The other status writes through `transition()`, and the audit of OR-2 | 13 |
| The fleetd file lock in use, the generation of the `fleetd` row, the backfill of the `fleetd` row | 14 |
| The fleet clock | 15 |
| The launcher | 16 |
| The `jobs` table, the job kinds, the run number, `abandoned_run_limit`, `job_backoff_s` | 17 |
| The `pass_cursors` work budget, the fact scan budgets, the time budget of a pass | 18 |
| The health guards and pacing (OR-4, OR-5) | 19 |
| I4, I5 and the process part of I1 (FM-4, FM-22) | 20 |
| The 3-part pass with intended rows and attempts in `launching` (RR-4, RR-7, FM-11), the heartbeat | 21 |
| The duties of today's watchdog in the pass; it acts in code (FM-7) | 22 |
| `horch fleetd`, `horch fleetd check`, hang detection (FM-12), `horch job requeue`, the CLI check of an older daemon | 23 |
| The self-update restart and the restart on a changed binary (OR-6, FM-34) | 24 |
| The stop of the old watchdog, `horch ledger watch` points at fleetd, the release test | 25 |
| Gate tests: FM-4, the part of FM-13 of Phase 1, FM-22 e2e | 26 |
| Gate tests: the 5 min job, the job backoff, the needs-you item and `horch job requeue` | 27 |
| Gate tests: 10,000 folders, RR-4, RR-7, FM-11 e2e | 28 |
| PL-1 (CI on 3 OSes), PL-3 (the `NotSupported` tests of 05 to 09), the gate file | 29 |
| The quota probes | today (no ticket) |
