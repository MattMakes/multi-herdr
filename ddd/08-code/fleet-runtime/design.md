# Design - Fleet runtime (`fleet-runtime`)

| | |
|---|---|
| Classification | supporting (cost-reducer) · sourcing build · pattern **active-record** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/fleet_runtime` |
| Structure | records + use cases (active record); calls the platform crate crates/horch-platform |
| Purpose | Fleet runtime keeps fleetd alive and current on macOS, Linux and Windows, so that supervision never stops silently and no work is lost on a restart (G1, G2). It provides every other context with the single ledger writer, the command channel and the herdr adapter. It decides when fleetd is hung or old and restarts it. It does not own the platform layer, which each context calls directly, and it does not start agent processes (worker-profiles). It does not decide the rows of other contexts' tables (organise N2) or any task, idea, worktree or branch state. - canvas: `ddd/07-define/fleet-runtime/bounded-context-canvas.md` |
| Domain roles | gateway, execution-context |

## 1. Module layout
```
crates/horch-core/src/fleetd/fleet_runtime/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    ledger.rs          one-door transition(entity, id, to_state, cause) and the allowed-transition table
    pass.rs            the reconcile pass order and check_invariants (I1 to I13, each check delegated to its owning module)
    channel.rs         command channel requests and request files, request classes, the operator token check
    jobs.rs            the job runner: jobs table, job kinds and classes, run number, abandoned run limit
    clock.rs           the fleet clock
    heartbeat.rs       heartbeat and the fleetd check rule
  application/
    commands.rs        1 use case per owned command
    handlers.rs        policies on consumed events
    queries.rs         read models
    ports.rs           driven port traits
  adapters/
    sqlite.rs          ledger-backed store or repositories
    memory.rs          in-memory fakes for tests
    gateways.rs        adapters to other contexts and adopted systems
```
Dependency rule: `domain/` imports nothing outside itself and std; `application/` imports `domain/` only; `adapters/` may import both and existing horch-core modules. No production file of the workspace outside `crates/horch-platform` holds `cfg(target_os)`, `cfg(unix)`, `cfg(windows)` or `cfg(target_family)` (PL-2, from Phase P). Enforced by `crates/horch-core/tests/arch_scan.rs` (tests: `fitness_domain_imports_nothing_outward`, `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform`, `fitness_only_fleet_runtime_opens_the_ledger_writable`, `fitness_no_context_touches_foreign_tables`).
Composition root: `crates/horch/src/cmd/fleetd.rs` (daemon and `horch fleetd check`) wires every adapter to its port; short-lived CLI commands wire the read-only queries and the command channel client.

## 2. Domain model
No aggregates by design (pattern = active-record, strategize N2). The records and their rules:
- Exactly 1 process, fleetd, writes the ledger; every other process opens it read-only and sends requests on the command channel or writes files (RR-5, C4, understand N5). Each context still decides the rows of its own tables (organise N2, decompose N5).
- fleetd writes a heartbeat after each full pass. When the heartbeat is older than 30 s of awake time, horch fleetd check restarts the service; on Linux the systemd watchdog does it. Every horch CLI command makes the same check before it sends its request (PL-5).
- When the horch binary changes, fleetd restarts on the new binary at once; a CLI that finds an older daemon restarts it. The control loop exits only between passes (OR-6, 04 S10, 04 S7).
- Schema migration is forward-only at daemon start: a backup is written first and 3 are kept, a newer schema is refused, and a development build does not migrate the shared ledger (OR-6, C4).
- Production code of the workspace has no OS cfg outside the leaf crate crates/horch-platform (lib.rs, mac.rs, linux.rs, windows.rs), which no context owns (A7, decompose N7). Each caller decides on an explicit not-supported value (PL-2, PL-3). Phase P creates the crate and moves the 178 existing production sites with no change of behavior (PL-6, D16). fleet-runtime adds the Phase 1 concerns: service install and restart, hang detection, sleep and wake, file watching and the channel transport.
- The command channel applies a request once per request_id and returns its stored result again; a known request_id with a different payload hash is refused; a refused request returns its reason, stored in its requests row (A24). Each request kind has a class, operator or worker. An operator request needs the operator token from /fleet/run/operator.token (mode 0600), and fleetd refuses one from a process of a live attempt or a descendant of its wrapper; a worker request is a file and never uses the channel (SR-8, A35).
- A pass has 3 parts: read facts within the scan budgets from the cursors in pass_cursors; 1 transaction that writes each effect as a row in an intended state; then the short effects and the heartbeat. The whole pass has a time budget below the hang limit (A24, A32).
- Long work is a job (A23, A33): the jobs table has 2 classes of job, pure and effect, a run number and a bound on runs that end abandoned (abandoned_run_limit 3). A pure job result passed, or failed after a run to its end, is used again; an effect job result never is. horch job requeue sets abandoned_runs to 0.
- 1 fleetd runs for 1 ledger: the fleetd lock and the generation number of the fleetd row; each write transaction checks the generation (A24).
- 1 fleet clock serves each operational timer; the hang check uses awake time from the platform layer (PL-4, A25).
- From Phase 5 fleetd is the only ledger writer; in Phases 1 to 4 the horch commands also write, through the same transition() (A36).
- New dispatch pauses only while free disk or machine load is past a safety floor, and resumes by itself. A probe that returns unknown (Windows today, 06 S12) is reported, never read as healthy or unhealthy (OR-5).
- Mail and outbox directories are mode 0700 and files 0600; a directory that another user owns or can write is refused (04 M5). All protocol file writes are atomic (RR-8).
- The only bytes typed into a terminal are 1 fixed doorbell line from fleetd's code (FR-7, C11). horch warns when herdr resume_agents_on_restore is not false (06 S2).
- start-agent-process starts each process in its own process group or job object and runs no process start inside a ledger transaction (connect N4). After a herdr restart, fleetd rebuilds from the ledger and the files (C14).
- Worker reports reach fleetd as files, not through the Unix-socket channel relay or report socket, so Windows loses no report (06 S11).

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `CheckFleetd` | `check-fleetd` | Every 30 s on macOS and Windows; WatchdogSec=30 on Linux; before each horch command. | store rows | see §2 |
| `SelfUpdate` | `self-update` | horch self-update. The operator runs it too. | store rows | see §2 |
| `RestartFleetd` | `restart-fleetd` | Restart always, through the platform layer. | store rows | see §2 |
| `ObserveJobs` | `observe-jobs` | Each pass reads the ended jobs and their results for the owner contexts. A running job past its time limit is stopped; a running job whose process is gone, also after a restart of fleetd, ends abandoned, and its next run is queued until abandoned_run_limit (3); then job-abandoned (A23, A33). | store rows | see §2 |
| `RequeueJob` | `requeue-job` | horch job requeue, an operator request with request_id and the operator token. Sets abandoned_runs of the job key to 0 and queues 1 more run; publishes job-requeued (A33). | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `FleetRuntimeCommands` | check-fleetd, self-update, restart-fleetd; called by horch fleetd check, horch self-update and the service manager | FleetdCli (crates/horch/src/cmd/fleetd.rs) |
| `CommandChannel` | operator requests from horch CLI commands over the socket or named pipe, or request files in /fleet/run/requests/ when fleetd is down; each request carries request_id and the operator token, and a requests row stores its class, how it was authorized, its result or its refusal reason; a known request_id returns the stored result, and a known request_id with a different payload hash is refused; fleetd refuses an operator request without a valid token, or from a process of a live attempt (A24, A35). Requests of class worker are files and never use the channel | PlatformChannelServer, RequestFileSpool |
| `ReconcilePass` | the 5 s level-triggered pass in 3 parts: read facts within the scan budgets from the cursors in pass_cursors; 1 transaction that calls each context in order, writes each effect as a row in an intended state and runs check_invariants; then the short effects (a process start through the launcher, a signal, a small file write, git update-ref on a local ref) and the heartbeat. The work budget and the time budget of a pass are below the hang limit (A24, A32) | FleetdDaemonLoop |
| `JobQueue` | the open host service of the job runner for task-execution, integration, worktree-custody and worker-profiles (R12, R14 to R16): queue a job of a kind for a subject and an input key in part 2 of a pass, read the ended jobs in part 1 of a later pass; a pure job result passed, or failed after a run to its end, is used again; an effect job result never is (A23, A33) | LedgerJobQueue |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `Ledger` | the one-door transition(entity, id, to_state, cause) with the allowed-transition table of the task, task branch, bundle, feature, worktree and idea state tables; an illegal transition is refused, recorded and put on the needs-you list, and the pass goes on; each write transaction first checks the generation of the fleetd row; writable only in the fleetd role from Phase 5, and in Phases 1 to 4 also by the horch commands through the same transition() (A24, A36) | persistence, collaborator or infrastructure need named in the operations |
| `Platform` | the platform interface of the leaf crate crates/horch-platform, which no context owns (decompose N7); fleet-runtime calls it for service install and restart, hang detection, sleep and wake, file watching and the channel transport (Phase 1 concerns); returns NotSupported where an OS has no equivalent | persistence, collaborator or infrastructure need named in the operations |
| `HerdrGateway` | herdr panes, the agent status (published as agent-status-changed to task-execution and ideas), the 1 doorbell line, notifications (adopted system, 06 S2 warning) | persistence, collaborator or infrastructure need named in the operations |
| `ProtectedFiles` | atomic writes of protocol files; mail and outbox dirs 0700 and files 0600 (04 M5, RR-8) | persistence, collaborator or infrastructure need named in the operations |
| `BinaryWatch` | detects a changed horch binary for the restart (04 S10) | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | the fleet clock for each operational timer and the due_at of each row; awake time from the platform layer for the heartbeat age (PL-4, PL-5, A25) | persistence, collaborator or infrastructure need named in the operations |
| `FleetRuntimeStore` | the active-record tables of fleet-runtime: jobs, pass_cursors, requests and the fleetd row (organise N6) | persistence, collaborator or infrastructure need named in the operations |
| `JobProcesses` | starts each job in its own process through the launcher of worker-profiles, outside the pass; alive by PID and start time; stops it at its time limit | persistence, collaborator or infrastructure need named in the operations |
| `FleetdLock` | the file lock of 1 fleetd for 1 ledger, through the platform layer, and the generation number of the fleetd row (A24) | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | publishes job-abandoned and job-requeued; writes the event row in the same ledger transaction | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `FleetRuntimeCommands` | `FleetdCli` | production | crates/horch/src/cmd/fleetd.rs |
| `CommandChannel` | `PlatformChannelServer` | production | - |
| `CommandChannel` | `RequestFileSpool` | production | - |
| `ReconcilePass` | `FleetdDaemonLoop` | production | - |
| `Ledger` | `SqliteLedger` | production | taskledger::store, schema migrations |
| `Ledger` | `InMemoryLedger` | in-memory fake | - |
| `Platform` | `MacPlatform` | production | crates/horch-platform/src/mac.rs |
| `Platform` | `LinuxPlatform` | production | crates/horch-platform/src/linux.rs |
| `Platform` | `WindowsPlatform` | production | crates/horch-platform/src/windows.rs |
| `Platform` | `FakePlatform` | in-memory fake | - |
| `HerdrGateway` | `HerdrCli` | production | workspace::herdr |
| `HerdrGateway` | `FakeHerdr` | in-memory fake | - |
| `ProtectedFiles` | `FsProtectedFiles` | production | fsx, messaging::mailbox |
| `ProtectedFiles` | `TempDirProtectedFiles` | in-memory fake | - |
| `BinaryWatch` | `MtimeBinaryWatch` | production | - |
| `BinaryWatch` | `FakeBinaryWatch` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `JobQueue` | `LedgerJobQueue` | production | - |
| `FleetRuntimeStore` | `SqliteFleetRuntimeStore` | production | - |
| `FleetRuntimeStore` | `InMemoryFleetRuntimeStore` | in-memory fake | - |
| `JobProcesses` | `LauncherJobProcesses` | production | - |
| `JobProcesses` | `FakeJobProcesses` | in-memory fake | - |
| `FleetdLock` | `PlatformFileLock` | production | - |
| `FleetdLock` | `FakeFleetdLock` | in-memory fake | - |
| `EventPublisher` | `LedgerEventPublisher` | production | - |
| `EventPublisher` | `InMemoryEventPublisher` | in-memory fake | - |
| `Clock` | `LedgerFleetClock` | production | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `job-abandoned` | use case that emits it (§3) | task-execution, integration, worktree-custody, worker-profiles, ideas | job_id, kind, subject, abandoned_runs | at-least-once | `contract: job-abandoned` |
| `job-requeued` | use case that emits it (§3) | task-execution, integration, worktree-custody, worker-profiles | job_id, kind, subject | at-least-once | `contract: job-requeued` |

### Consumed
No event from another context is consumed. Requests arrive as calls on the driving ports.

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `FleetdHeartbeat` | fleetd-hang-detected, fleetd-restarted, horch-version-installed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `PassBacklog` | job-abandoned, job-requeued | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Owns the ledger file, the events table, the ingested table and schema migrations (forward-only, backup first, 3 kept, newer schema refused, development build does not migrate). Each context decides the rows of its own tables; fleet-runtime commits them (organise N2). Owns the tables jobs, pass_cursors, requests and the fleetd row (organise N6). The platform layer is the leaf crate crates/horch-platform with lib.rs, mac.rs, linux.rs and windows.rs (A7).

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| fitness | `fitness_only_fleet_runtime_opens_the_ledger_writable` | From Phase 5, exactly 1 process writes the ledger (RR-5, C4, A36) |
| invariant | `cli_command_opens_ledger_read_only_and_sends_a_request` | From Phase 5, exactly 1 process writes the ledger (RR-5, C4, A36) |
| fault injection | `fm12_hung_fleetd_restarted_on_each_os` | A hung fleetd is restarted on all 3 OSes (PL-5) |
| preservation | `pres_04_s10_changed_binary_restarts_between_passes` | The control loop restarts on a changed binary, only between passes (04 S10) |
| preservation | `pres_04_m5_foreign_owned_outbox_is_refused` | Mail and outbox dirs are 0700, files 0600; a foreign-owned dir is refused (04 M5) |
| preservation | `pres_06_s2_warns_on_herdr_resume_agents_on_restore` | horch warns when herdr resume_agents_on_restore is not false (06 S2) |
| preservation | `pres_06_s11_worker_report_is_a_file_on_every_os` | Worker reports reach fleetd as files, not sockets (06 S11) |
| preservation | `pres_06_s12_unknown_probe_neither_pauses_nor_passes` | A probe that returns unknown is reported, never healthy or unhealthy (06 S12, OR-5) |
| fitness | `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform` | Production code of the workspace has no OS cfg outside crates/horch-platform; a build script that cannot call it is named with its reason (PL-2) |
| invariant | `platform_not_supported_is_explicit_on_each_module` | A missing OS equivalent returns NotSupported (PL-3) |
| invariant | `newer_schema_is_refused_and_backup_precedes_migration` | Schema migration: backup first, newer schema refused, dev build does not migrate (OR-6, C4) |
| invariant | `illegal_transition_is_refused_and_writes_nothing` | All writes go through the one-door transition with the allowed-transition table |
| invariant | `start_agent_process_inside_transaction_panics_in_debug` | No process start runs inside a ledger transaction (connect N4) |
| invariant | `only_the_fixed_doorbell_line_is_typed` | The only bytes typed into a terminal are the 1 fixed doorbell line (FR-7) |
| invariant | `pl6_each_test_has_the_same_result_before_and_after_the_move` | The Phase P move changes no behavior: on macOS, Linux and Windows each test has the same result before and after it, and a test on the list of expected failures still fails (PL-6) |
| invariant | `expected_failures_list_gates_the_run` | A run passes when each test off the list of expected failures passes and each test on it fails; each entry names the phase that makes it green (A22) |
| fitness | `fitness_no_context_touches_foreign_tables` | No context reads or writes the ledger tables or workspace folders of another context (organise N4) |
| preservation | `pres_06_s14_hung_fleetd_restarted_on_macos` | A hung fleetd is detected and restarted on macOS (06 S14) |
| preservation | `pres_06_s14_hung_fleetd_restarted_on_linux` | A hung fleetd is detected and restarted on Linux (06 S14) |
| preservation | `pres_06_s14_hung_fleetd_restarted_on_windows` | A hung fleetd is detected and restarted on Windows (06 S14) |
| invariant | `request_with_a_seen_request_id_returns_the_stored_result` | fleetd applies a request once per request_id and returns the stored result again (A24) |
| invariant | `refused_request_returns_its_reason` | A refused request returns its reason, stored in its requests row (A24) |
| contract | `published job-abandoned carries exactly job_id, kind, subject, abandoned_runs (connect.json payload) and nothing consumers did not ask for` | contract: job-abandoned |
| contract | `published job-requeued carries exactly job_id, kind, subject (connect.json payload) and nothing consumers did not ask for` | contract: job-requeued |
| invariant | `known_request_id_with_another_payload_hash_is_refused` | A known request_id with a different payload hash is refused (A24) |
| invariant | `operator_request_without_a_valid_token_is_refused_and_recorded` | fleetd refuses an operator request without a valid operator token and records the refusal (SR-8, A35) |
| invariant | `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` | From inside a worker sandbox, a raw request on the channel and a request file with the right owner are refused for each operator kind (SR-8, Phase 5 gate) |
| invariant | `operator_request_from_a_process_of_a_live_attempt_is_refused` | fleetd refuses an operator request on the channel from a process of a live attempt or a descendant of its wrapper (SR-8, A35) |
| invariant | `second_fleetd_exits_on_a_generation_mismatch` | 1 fleetd for 1 ledger: the file lock and the generation number of the fleetd row (A24) |
| invariant | `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart` | An effect is a row in an intended state first, then the effect after the commit, then the observation (A24) |
| invariant | `slow_worktree_add_and_10000_outboxes_keep_the_heartbeat_under_30_s` | A pass never waits for long work; the scans of part 1 and the rows of part 2 have budgets, and the pass ends before the hang limit (PL-5, A23, A32) |
| invariant | `each_scan_continues_at_its_cursor_after_a_restart` | A pass never waits for long work; the scans of part 1 and the rows of part 2 have budgets, and the pass ends before the hang limit (PL-5, A23, A32) |
| invariant | `pure_job_result_is_used_again_and_effect_job_runs_again` | A pure job result passed, or failed after a run to its end, is used again; an effect job result never is; uniqueness is kind, subject, input key and run number (A33) |
| invariant | `job_abandoned_3_times_goes_on_the_needs_you_list_and_requeue_resets_it` | A job that ends abandoned runs again until abandoned_run_limit, then job-abandoned; horch job requeue sets abandoned_runs to 0 (A33) |
| invariant | `running_job_with_a_lost_process_runs_again_after_restart` | A running job whose process is gone after a restart of fleetd ends abandoned and runs again (A23) |
| invariant | `fleet_clock_does_not_advance_during_sleep_or_downtime` | Each operational timer is a deadline on the fleet clock; a sleep or a down fleetd fires no deadline (PL-4, A25) |
| invariant | `illegal_transition_does_not_stop_the_pass` | An illegal transition is refused, recorded with its row and cause, put on the needs-you list, and each other row of the pass goes on (A24) |
| invariant | `before_phase_5_a_horch_command_write_goes_through_the_same_transition` | In Phases 1 to 4 the horch commands write the ledger through the same transition() and allowed-transition table as fleetd (A36) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
herdr and the OS service managers are adopted systems behind HerdrGateway and Platform (strategize N1). Each OS module returns NotSupported where it has no equivalent, and core code decides (PL-3).

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A11, A15, A16 · Open questions: none (Q3 is answered, see deprecated[] in code.json).
