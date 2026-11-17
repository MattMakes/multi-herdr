# Map: fleetd build, Phase 5 (one ledger writer and the operator boundary)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of Phase 5, their rows in the fitness rules and in the architecture doc](01-phase5-modules.md) | T-404 | 1 |
| 02 | [The next schema version: the requests table with class and auth, and the polite_stops table](02-schema-phase5.md) | T-404 | 1 |
| 03 | [The Phase 5 keys of fleet.toml: the size limits of worker and request files, and the list of orchestrator seats](03-fleet-toml-phase5-keys.md) | T-404 | 1 |
| 04 | [Platform: the Windows command channel has 1 pipe instance only, and a client tries again on PIPE_BUSY](04-windows-pipe-single-instance.md) | T-404 | 1 |
| 05 | [Platform: a client of the channel gets the process of the server, on each OS](05-platform-server-process.md) | 04 | 2 |
| 06 | [Platform sandbox: deny the read of the token file, the connect to the channel and a write into the request folder](06-platform-sandbox-operator-denials.md) | T-404 | 1 |
| 07 | [The protected file reader: the owner, mode, link, real path and size checks of SR-5 and SR-6, in 1 place](07-protected-files.md) | 01 | 2 |
| 08 | [The request contract: the wire line, the request_id, the payload hash, and the kind catalog with the class of each kind](08-request-contract.md) | 01 | 2 |
| 09 | [The authorization decision of fleetd, and the 1 function that knows an orchestrator seat](09-authorize-and-orchestrator-seat.md) | 03, 08 | 3 |
| 10 | [The operator token: made 1 time with mode 0600, read by the horch command, compared in constant time](10-operator-token.md) | 07 | 3 |
| 11 | [The peer process fact: is the process of a channel peer in a live attempt](11-peer-process-facts.md) | 01 | 2 |
| 12 | [The requests rows: pending, then applied or refused in 1 transaction, and the stored result for a seen request_id](12-requests-store-and-apply.md) | 02, 09 | 4 |
| 13 | [The channel server of fleetd: bind, the peer user check, 1 request line with a deadline, the decision, the reply](13-channel-server.md) | 05, 10, 11, 12 | 5 |
| 14 | [The request files: the folder modes, the checks of each file, the requests row, then the archive](14-request-file-spool.md) | 07, 10, 12 | 5 |
| 15 | [The client of the channel: send 1 request, check that the peer is fleetd, ask for the result after a lost reply, or write a request file](15-channel-client.md) | 05, 07, 08, 10 | 4 |
| 16 | [horch request <request_id>: print the state and the result of a request](16-horch-request-command.md) | 02 | 2 |
| 17 | [fleetd handlers for the run and plan kinds: run_new, run_close, run_gate, task_add, map_add, task_solo, plan_workers, ledger_import, ledger_gc](17-handlers-run.md) | 12 | 5 |
| 18 | [fleetd handlers for the task kinds: task_requeue, task_cancel, task_unblock, task_fallback, ledger_waive, ledger_reopen, ledger_dismiss](18-handlers-task.md) | 12 | 5 |
| 19 | [fleetd handlers for the worker kinds: spawn, worker_stop, worker_reap, worker_new, worker_pid, orchestrator_claim, orchestrator_stop](19-handlers-worker.md) | 12 | 5 |
| 20 | [fleetd handlers for the custody, job and integration kinds: job_requeue, worktree_scratch, worktree_drop, worktree_restore, worktree_triage, feature_abandon, branch_abandon, rescue_drop](20-handlers-custody.md) | 12 | 5 |
| 21 | [Route the run and plan commands of horch ledger through requests: run new, run close, run gate, add, add-map, task, plan-workers, import, gc](21-route-ledger-run-commands.md) | 15, 17 | 6 |
| 22 | [Route the operator task and worker commands of horch ledger through requests: cancel, unblock, requeue, waive, fallback, reopen, dismiss, stop, reap, worker new, worker pid, worktree scratch and drop](22-route-ledger-task-and-worker-commands.md) | 15, 18, 19, 20, 21 | 7 |
| 23 | [Route horch spawn and horch ledger spawn through the spawn request](23-route-spawn.md) | 15, 19 | 6 |
| 24 | [Route the orchestrator claim and stop of horch fleet start and stop through requests, and make the reads of horch sessions read-only](24-route-fleet-start-and-sessions.md) | 15, 19 | 6 |
| 25 | [Route horch job requeue, horch task, horch worktree, horch feature, horch branch and horch rescue through requests](25-route-job-task-worktree-integration-commands.md) | 15, 18, 20 | 6 |
| 26 | [The outbox writer of a worker command: find the own attempt by the process chain, make the outbox, write each file atomically](26-outbox-writer.md) | 03, 07 | 3 |
| 27 | [horch ledger progress, note, done, finish and handoff write the outbox; claim and worktree new are refused to a worker](27-worker-ledger-commands-write-files.md) | 22, 26 | 8 |
| 28 | [horch ask, the QUESTION path, horch done and horch result write through the outbox writer](28-ask-done-result-question-files.md) | 16, 26, 27 | 9 |
| 29 | [fleetd ingests progress.jsonl, the handoff result and the owner and mode checks of each outbox file](29-ingest-outbox-files.md) | 07, 26 | 4 |
| 30 | [The wrapper (horch worker) writes heartbeat.json, exit.json and session.json, and no ledger row](30-wrapper-writes-files.md) | 26 | 4 |
| 31 | [fleetd ingests heartbeat.json, exit.json and session.json of each attempt](31-ingest-wrapper-files.md) | 07, 30 | 5 |
| 32 | [A horch_access: report pane reports progress and done through its outbox; the report socket keeps tell only](32-report-pane-writes-files.md) | 26 | 4 |
| 33 | [A job process writes its result file; fleetd ingests it and ignores a result of another run](33-job-result-file.md) | 07 | 3 |
| 34 | [The polite stop of a process is a ledger row, not a file in run/stops](34-polite-stops-row.md) | 01, 02 | 2 |
| 35 | [The Claude Code adapter denies the 3 operator acts and writes only the task folder and the worktree](35-claude-operator-boundary.md) | 01, 06 | 2 |
| 36 | [The Codex adapter: remove the horch ledger sandbox escape, write only the task folder and the worktree, and deny the 3 operator acts or say NotSupported](36-codex-operator-boundary.md) | 35 | 3 |
| 37 | [The Antigravity, OpenCode, pi, Prime and none adapters: the 3 operator denials or NotSupported](37-other-adapters-operator-boundary.md) | 35 | 3 |
| 38 | [Profile validation refuses a worker profile whose sandbox allows an operator act or does not exist on this OS; the no-sandbox mark](38-profile-validation-operator-boundary.md) | 03, 09, 36, 37 | 4 |
| 39 | [Each worker teammate gets a sandbox that passes the operator boundary](39-worker-teammates-get-sandboxes.md) | 38 | 5 |
| 40 | [FR-7: the only bytes that horch types into a pane are fixed lines from 1 list](40-only-fixed-lines-are-typed.md) | T-404 | 1 |
| 41 | [Wire Phase 5 into fleetd: the token, the channel server, the request files, the outbox and wrapper files, the job result files and the polite stop rows](41-pass-wiring-phase5.md) | 10, 13, 14, 29, 31, 33, 34 | 6 |
| 42 | [Each process other than fleetd opens the ledger read-only, and the arch rule that enforces it](42-read-only-ledger-outside-fleetd.md) | 21, 22, 23, 24, 25, 27, 28, 30, 32, 33, 41 | 10 |
| 43 | [The SR-8 gate test: from inside a worker sandbox, each operator kind on the channel and as a request file is refused and recorded](43-p5-operator-boundary-e2e.md) | 38, 42 | 11 |
| 44 | [The e2e tests of untrusted files: SR-5 request files, SR-6 worker files and FM-23](44-p5-untrusted-files-e2e.md) | 41, 42 | 11 |
| 45 | [The e2e tests of requests: 1 request_id has 1 effect, the crash cases, and a lost reply](45-p5-requests-e2e.md) | 41, 42 | 11 |
| 46 | [The e2e tests of a worker whose sandbox makes the ledger read-only, and of FM-17 for Codex](46-p5-read-only-worker-e2e.md) | 27, 36, 41, 42 | 11 |
| 47 | [The release test of Phase 5: an old worker of Phase 4 runs progress and done after the update, and only fleetd writes](47-p5-release.md) | 41, 42 | 11 |
| 48 | [The worker briefing of Phase 5: report through files, no claim section](48-worker-briefing-phase5.md) | 27, 28, 39 | 10 |
| 49 | [Gate of Phase 5: each rule of the gate green, the arch test, the run-in-flight test, and the gate file](49-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48 | 12 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phase 0 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is not on
the list of expected failures passes, and each test on the list fails for its listed reason. The
Check of each ticket also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet
itself uses the installed `horch`. No worker installs a development build, and each test runs the
development binary only with temporary folders (build rule 1).

## Waves

- Wave 1: 01, 02, 03, 04, 06, 40
- Wave 2: 05, 07, 08, 11, 16, 34, 35
- Wave 3: 09, 10, 26, 33, 36, 37
- Wave 4: 12, 15, 29, 30, 32, 38
- Wave 5: 13, 14, 17, 18, 19, 20, 31, 39
- Wave 6: 21, 23, 24, 25, 41
- Wave 7: 22
- Wave 8: 27
- Wave 9: 28
- Wave 10: 42, 48
- Wave 11: 43, 44, 45, 46, 47
- Wave 12: 49

The tickets of 1 wave run at the same time. No 2 tickets that can run at the same time own the same
file: the generator of this map checked each pair. Tickets that change the same file run one after
the other through their blockers: 04 then 05 (`windows/channel.rs`); 21, 22 then 27
(`crates/horch/src/cmd/taskledger.rs`); 16 then 28 (`main.rs`, `cmd/mod.rs`); 34 then 41 (the pass);
36, 38, 39 then 48 (`teammates/`); 01 then 42 (`arch_scan.rs`). Ticket 01 makes each new module file,
so later tickets own different files. The handlers (17 to 20) and the routing tickets (21 to 25) are
1 group of commands each.

Each ticket with no blocker in this map (01, 02, 03, 04, 06, 40) is blocked by `T-404`, the gate of
Phase 4.

## Critical path

01 -> 08 -> 09 -> 12 -> 17 -> 21 -> 22 -> 27 -> 28 -> 42 -> 43 -> 49

## Decisions that hold

- A guard whose fact is unknown does not hold.
- fleetd closes a pane, and removes a tree, only with a fact that proves that it is its own; else it opens a needs-you item.
- What the user reads keeps the words of today, and a new event has a word that says what happened.
- A job starts only through the launcher.
- Each ledger write goes through `transition()`.
- Each effect is a committed row first.
- The scans and the run duties of the pass have a work budget and a cursor.
- OS-specific code is only in `crates/horch-platform`.
- No sandbox for a profile on an OS means no dispatch of that profile and 1 needs-you item.
- The schema: no ticket writes a schema number. A ticket writes "the next schema version" and reads `VERSION` in `crates/horch-core/src/taskledger/schema.rs`.

## Request kinds

1 row for each command of today that changes state (Q6). A command that only reads has no kind. The
kinds of class `worker` are files of the attempt and never use the channel or the request folder.
fleetd refuses an unknown kind and a worker kind on the channel or in a request file.

| Command | Kind | Class |
|---|---|---|
| `horch ledger run new` | `run_new` | operator |
| `horch ledger run close` | `run_close` | operator |
| `horch ledger run gate` | `run_gate` | operator |
| `horch ledger add` | `task_add` | operator |
| `horch ledger add-map (not --dry-run)` | `map_add` | operator |
| `horch ledger task` | `task_solo` | operator |
| `horch ledger plan-workers` | `plan_workers` | operator |
| `horch ledger import` | `ledger_import` | operator |
| `horch ledger gc` | `ledger_gc` | operator |
| `horch ledger requeue, horch task requeue` | `task_requeue` | operator |
| `horch ledger cancel, horch task cancel` | `task_cancel` | operator |
| `horch ledger unblock` | `task_unblock` | operator |
| `horch ledger fallback` | `task_fallback` | operator |
| `horch ledger waive` | `ledger_waive` | operator |
| `horch ledger reopen` | `ledger_reopen` | operator |
| `horch ledger dismiss` | `ledger_dismiss` | operator |
| `horch ledger stop` | `worker_stop` | operator |
| `horch ledger reap` | `worker_reap` | operator |
| `horch ledger worker new` | `worker_new` | operator |
| `horch ledger worker pid` | `worker_pid` | operator |
| `horch spawn, horch ledger spawn` | `spawn` | operator |
| `horch fleet start (the orchestrator claim)` | `orchestrator_claim` | operator |
| `horch fleet stop (the record of the stop)` | `orchestrator_stop` | operator |
| `horch job requeue` | `job_requeue` | operator |
| `horch ledger worktree scratch (from you)` | `worktree_scratch` | operator |
| `horch ledger worktree drop` | `worktree_drop` | operator |
| `horch worktree restore` | `worktree_restore` | operator |
| `horch worktree triage, horch worktree apply` | `worktree_triage` | operator |
| `horch feature abandon` | `feature_abandon` | operator |
| `horch branch abandon` | `branch_abandon` | operator |
| `horch rescue drop` | `rescue_drop` | operator |
| `horch ledger progress, horch ledger note` | progress (a line of outbox/progress.jsonl) | worker |
| `horch result, horch ledger done, horch ledger finish, horch done, horch ledger handoff` | result (outbox/result.json) | worker |
| `horch ask, the QUESTION line of horch tell` | ask (a file of outbox/questions/) | worker |

## Open questions

Q1, Q2, Q3, Q4 and Q14 are answers of the orchestrator (QUESTION of T-436). Q6 is an answer of the
orchestrator with its table below. The other points are choices of the planner where the design and the
code of today do not agree, or where the design leaves a point open. Each ticket follows them, and the
orchestrator can change each one.

- **Q1. The Windows pipe (REQUIRED item 1).** Choice (a): 1 pipe instance only (`nMaxInstances` 1
  with `FILE_FLAG_FIRST_PIPE_INSTANCE`). fleetd never closes the handle while it lives and reuses it
  for each client (`DisconnectNamedPipe`, `ConnectNamedPipe`), and a client tries again on
  `PIPE_BUSY`. Why: (b) needs a worker sandbox that denies the creation of a pipe by name, and no CLI
  sandbox on Windows gives that today (the Claude sandbox is not supported on Windows). (a) depends on
  fleetd alone. The client check of the server process (Q8) is still necessary in each case. Ticket 04.
- **Q2. The orchestrator of today (security choice for the operator).** An attempt with a null task
  whose persona is in `orchestrator_seats` of `fleet.toml` is an orchestrator seat. fleetd knows it
  only from the ledger and `fleet.toml`, never from an environment variable. The exemption is in 1
  function (`is_orchestrator_seat`, ticket 09) with tests that a worker cannot get it. A seat is not a
  worker profile: it has no operator-path deny, it reads the token, and the peer process check exempts
  it. Phase 6 removes the exemption. **Operator: until Phase 6 the orchestrator agent has operator
  rights.** Tickets 03, 09, 38, 43.
- **Q3. Worker commands write files; no worker kind uses the channel.** The release table says that an
  old worker command sends a request when its attempt has no outbox, but SR-8 refuses a worker kind
  on the channel. Choice: fleetd makes the outbox of each live attempt at its first pass, and a worker
  command makes its own outbox when it is missing. The command finds its attempt by the process chain
  (the wrapper of a live attempt is its parent), never by the environment. fleetd checks each outbox
  that a worker made with the same SR-6 checks (owner, mode, no link, size). `claim` and
  `worktree new` and `scratch` are refused to a worker, because each task is pushed since Phase 4.
  Tickets 26, 27, 29, 41.
- **Q4. `horch ledger handoff`.** It writes `result.json` with `status: blocked`, `reason: handoff`
  and the path (the status list of the design stays closed). The attempt ends with the outcome
  `handed_off` (today's word), burns no attempt, and the event says `handoff`. The path is an
  untrusted worker path. `START.md` of the next attempt names it. Tickets 26, 27, 29.
- **Q5. The `fleet` helper.** The design names `fleet progress`, `fleet result` and `fleet ask`.
  Choice: the helper is the 1 `horch` binary: `horch ledger progress`, `horch result` and a new
  `horch ask`. No second binary. Ticket 28.
- **Q6. The request kinds of the commands of today.** The design lists the kinds of Phase 6. Each
  command of today that changes state gets 1 operator kind named after the command. A command that
  only reads has no kind. Phase 6 retires the kinds that it replaces. The table "Request kinds" below
  is the full list. Ticket 08.
- **Q7. The shown task words.** `docs/tickets/fleetd-p2/MAP.md` Q3 says "the shown words change in
  Phase 5". The design does not name it, and a decision that holds says that what the user reads
  keeps the words of today. Choice: Phase 5 keeps the shown words.
- **Q8. The unix socket (REQUIRED item 2).** A process of the same user can remove the socket file and
  bind its own. The client checks that the server process (PID and start time) equals the `fleetd`
  row before it sends (tickets 05, 15), and the worker sandbox denies each write in `<fleet root>/run/`
  (tickets 35 to 37).
- **Q9. FR-7 and messages.** A task pane gets only the constant `DOORBELL` (Phase 2). A message to a
  pane that is not a task attempt (the orchestrator, you) types 1 other constant line, `You have new
  mail. Run horch inbox.`, and the message stays in its mail file. The Claude channel and
  `codex queue` are not a terminal. **Operator: this changes what the orchestrator agent sees after
  the release of Phase 5.** Ticket 40.
- **Q10. Codex and the deny of a read.** The Codex sandbox may not deny a read of 1 file. If it
  cannot, the Codex adapter returns `NotSupported`, and the Codex profiles are refused on that OS
  (A37). That removes `codex-sol`, the fallback of each Claude persona (Phase 2 Q8). Ticket 36 sends
  QUESTION with the facts before it lands that result.
- **Q11. The peer process on Windows.** The job object fact is `NotSupported` today, so the peer
  process check gives unknown on Windows, and an operator request on the Windows channel is refused.
  The request file path works for you (with the token), and fleetd reads it at each pass. Ticket 11.
- **Q12. The pane proof of a politely stopped worker (REQUIRED item 4).** Choice: a ledger row
  (`polite_stops`), not the file `run/stops/<pid>-<start>.json` of T-414. From Phase 5 fleetd is the
  only writer, so the row is cheap; a row needs no SR-6 checks; and it does not depend on the sandbox
  deny of the run folder. Tickets 02, 34.
- **Q13. A file of another owner in a test.** This Mac cannot make a file of another user without
  root. The owner fact comes through a test seam of the protected file reader. Tickets 07, 14, 44.
- **Q14. Job results.** A job process writes `<fleet root>/run/jobs/<job id>-<run>.json` atomically.
  It is a child of fleetd through the launcher, so the file is fleetd-owned; fleetd still reads it
  with the SR-6 checks, and a result of a run that is not the current run is ignored and logged.
  Ticket 33.
- **Q15. Large inputs.** A request file and a request line have the limit `request_max_bytes`
  (64 KiB). `add-map` and other commands with large inputs send the absolute path and the sha256 of
  each file; fleetd reads the files and refuses the request when a sha256 differs. Ticket 17.
- **Q16. Module files.** Ticket 01 makes each new file of the 3 contexts (1 file for each concern),
  so that tickets of 1 wave own different files.
- **Q17. Paths.** The channel is `<fleet root>/run/fleetd.sock` (a pipe name from that path on
  Windows), the token `<fleet root>/run/operator.token`, the request folder
  `<fleet root>/run/requests/` (Q4 of `docs/tickets/fleetd-p1/MAP.md`). The worker sandbox denies
  each write in `<fleet root>/run/`.
- **Q18. `approve_refused_inside_a_fleet_attempt`.** The DDD Slice 5 lists this test, but
  `horch idea approve` is Phase 6. Phase 5 has no idea command, so the test is Phase 6.
- **Q19. Expected failures.** No line of `crates/horch-e2e/expected-failures/` names Phase 5 today.
  The gate ticket checks it again.
- **Q20. The read deadline of the channel.** 5 s, the connect time of today
  (`preservation-05-contracts-and-values.md`, row V-18). Ticket 13.

Risks of the phase:

- **R1.** Codex may have no deny of a read (Q10). Then no Codex worker runs, and the fallbacks of
  Phase 2 need a new choice.
- **R2.** Ticket 39 gives each worker teammate a sandbox. A worker that needs a path or the network
  outside its sandbox fails its task. The first runs after the release show it.
- **R3.** The Windows channel (tickets 04, 05) and the Windows request file rules have no test on this
  Mac. CI on Windows is the only proof.
- **R4.** Ticket 42 changes the open of the ledger in each caller. A missed writer fails at run time.
  The seam `HORCH_TEST_ASSERT_READ_ONLY` and the arch rule catch it in tests.
- **R5.** The orchestrator keeps operator rights until Phase 6 (Q2).
- **R6.** Self-hosting: each `horch` command reaches the ledger in a new way. The build fleet uses the
  installed `horch`, and no worker installs a development build.

## Coverage

Each mechanism of the row Phase 5 in "What each gate needs" and in "New work for each phase" of the
design, each rule of the gate line and each REQUIRED item of the ticket of this map, with the 1
ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| The fleetd lock and the generation number | Phase 1 (no ticket); its test in the gate, 49 |
| The transport of the command channel | Phase 1 (no ticket) |
| The Windows transport: 1 pipe instance (REQUIRED item 1) | 04 |
| The server process for a client of the channel | 05 |
| The 3 operator denials of the platform sandbox | 06 |
| The protected file reader (owner, mode, link, real path, size) | 07 |
| The class of each request kind, the request contract | 08 |
| The authorization decision, the orchestrator seat (Q2) | 09 |
| The operator token | 10 |
| The peer process check | 11 |
| The `ingested` table | Phase 4 (no ticket) |
| The `requests` table with `class` and `auth`, and `polite_stops` | 02 |
| The `requests` rows: pending, applied or refused, the stored result | 12 |
| The command channel server and the peer-user check | 13 |
| The request files and the modes of the request folder | 14 |
| Each `horch` command sends a request, or writes a request file when fleetd is down | 15 |
| `horch request` | 16 |
| The handlers of the run and plan kinds | 17 |
| The handlers of the task kinds | 18 |
| The handlers of the worker kinds | 19 |
| The handlers of the custody, job and integration kinds | 20 |
| The run and plan commands send requests | 21 |
| The operator task and worker commands send requests; HORCH_ROLE is not trusted (REQUIRED item 3) | 22 |
| `horch spawn` sends a request | 23 |
| `horch fleet start` and `stop` send requests | 24 |
| The operator commands of Phases 1 to 4 send requests | 25 |
| The outbox files and the outbox writer | 26 |
| The worker commands write files; no "readonly database" (REQUIRED item 5) | 27 |
| The `fleet` helper (`horch ask`, `horch result`) | 28 |
| The SR-6 checks and the ingest of each outbox file | 29 |
| The wrapper writes files (`heartbeat.json`) | 30 |
| The SR-6 checks and the ingest of each wrapper file | 31 |
| The worker report of a report pane is a file (06 S11) | 32 |
| The job result file and its SR-6 checks (Q14) | 33 |
| The pane proof of a polite stop is a row (REQUIRED item 4) | 34 |
| The deny rules of the Claude sandbox | 35 |
| The removal of Codex's `broad_ledger_rule` escape, FM-17 | 36 |
| The deny rules of the other adapters | 37 |
| Profile validation of the deny rules, the refusal of a profile on an OS that cannot sandbox it, the "no sandbox" mark | 38 |
| A sandbox for each worker teammate | 39 |
| FR-7: only fixed lines are typed | 40 |
| The wiring in fleetd | 41 |
| The read-only ledger outside fleetd and its `arch_scan` rule (RR-5, FM-24, the arch test) | 42 |
| The SR-8 gate test from inside a worker sandbox; REQUIRED item 3 tests | 43 |
| SR-5, SR-6 and FM-23 e2e | 44 |
| RR-8 and FM-15 e2e | 45 |
| FM-17 e2e and the read-only worker e2e (REQUIRED item 5) | 46 |
| The run-in-flight test of Phase 5 | 47 |
| The worker briefing | 48 |
| The gate file | 49 |
| The unix socket (REQUIRED item 2): the client check of the server and the deny of writes in `run/` | 15 (client check); the deny is 35 to 37 |
