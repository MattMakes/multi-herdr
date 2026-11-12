# Design - Fleet runtime (`fleet-runtime`)

| | |
|---|---|
| Classification | supporting (cost-reducer) · sourcing build · pattern **active-record** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/fleet_runtime` |
| Structure | records + use cases (active record) plus the platform layer |
| Purpose | Fleet runtime keeps fleetd alive and current on macOS, Linux and Windows, so that supervision never stops silently and no work is lost on a restart (G1, G2). It provides every other context with the single ledger writer, the command channel, the platform layer and the start of agent processes. It decides when fleetd is hung or old and restarts it. It does not decide the rows of other contexts' tables (organise N2) or any task, idea, worktree or branch state. - canvas: `ddd/07-define/fleet-runtime/bounded-context-canvas.md` |
| Domain roles | gateway, execution-context |

## 1. Module layout
```
crates/horch-core/src/fleetd/fleet_runtime/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    ledger.rs          one-door transition(entity, id, to_state, cause) and the allowed-transition table
    pass.rs            the reconcile pass order and check_invariants (I1 to I13, each check delegated to its owning module)
    channel.rs         command channel requests and request files
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
Dependency rule: `domain/` imports nothing outside itself and std; `application/` imports `domain/` only; `adapters/` may import both and existing horch-core modules. No file under `crates/horch-core/src/fleetd/` holds `cfg(target_os)`, `cfg(unix)` or `cfg(windows)`. Enforced by `crates/horch-core/tests/arch_scan.rs` (tests: `fitness_domain_imports_nothing_outward`, `fitness_fleetd_tree_has_no_os_cfg`, `fitness_only_fleet_runtime_opens_the_ledger_writable`).
Composition root: `crates/horch/src/cmd/fleetd.rs` (daemon and `horch fleetd check`) wires every adapter to its port; short-lived CLI commands wire the read-only queries and the command channel client.

## 2. Domain model
No aggregates by design (pattern = active-record, strategize N2). The records and their rules:
- Exactly 1 process, fleetd, writes the ledger; every other process opens it read-only and sends requests on the command channel or writes files (RR-5, C4, understand N5). Each context still decides the rows of its own tables (organise N2, decompose N5).
- fleetd writes a heartbeat after each full pass. When the heartbeat is older than 30 s of awake time, horch fleetd check restarts the service; on Linux the systemd watchdog does it. Every horch CLI command makes the same check before it sends its request (PL-5).
- When the horch binary changes, fleetd restarts on the new binary at once; a CLI that finds an older daemon restarts it. The control loop exits only between passes (OR-6, 04 S10, 04 S7).
- Schema migration is forward-only at daemon start: a backup is written first and 3 are kept, a newer schema is refused, and a development build does not migrate the shared ledger (OR-6, C4).
- Core fleetd code has no OS cfg. It calls 1 platform interface with 1 module per OS; an OS without an equivalent returns an explicit not-supported value that core code decides on (PL-2, PL-3).
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

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `FleetRuntimeCommands` | check-fleetd, self-update, restart-fleetd; called by horch fleetd check, horch self-update and the service manager | FleetdCli (crates/horch/src/cmd/fleetd.rs) |
| `AgentProcesses` | start-agent-process and stop-process-group for task-execution and ideas (connect message start-agent-process) | InProcessFleetRuntime |
| `CommandChannel` | operator requests from horch CLI commands over the socket or named pipe, or request files when fleetd is down | PlatformChannelServer, RequestFileSpool |
| `ReconcilePass` | the 5 s level-triggered pass: read facts, call each context in order, check_invariants, write heartbeat | FleetdDaemonLoop |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `Ledger` | the one-door transition(entity, id, to_state, cause) with the allowed-transition table; writable only in the fleetd role | persistence, collaborator or infrastructure need named in the operations |
| `Platform` | the 1 platform interface: service install and restart, hang detection, process facts, stop a process group, file watching, sandbox, sleep and wake, channel transport, machine probe; returns NotSupported where an OS has no equivalent | persistence, collaborator or infrastructure need named in the operations |
| `HerdrGateway` | herdr panes, agent status, the 1 doorbell line, notifications (adopted system, 06 S2 warning) | persistence, collaborator or infrastructure need named in the operations |
| `ProtectedFiles` | atomic writes of protocol files; mail and outbox dirs 0700 and files 0600 (04 M5, RR-8) | persistence, collaborator or infrastructure need named in the operations |
| `BinaryWatch` | detects a changed horch binary for the restart (04 S10) | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | awake time for the heartbeat age (PL-5) | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `FleetRuntimeCommands` | `FleetdCli` | production | crates/horch/src/cmd/fleetd.rs |
| `AgentProcesses` | `InProcessFleetRuntime` | production | - |
| `CommandChannel` | `PlatformChannelServer` | production | - |
| `CommandChannel` | `RequestFileSpool` | production | - |
| `ReconcilePass` | `FleetdDaemonLoop` | production | - |
| `Ledger` | `SqliteLedger` | production | taskledger::store, schema migrations |
| `Ledger` | `InMemoryLedger` | in-memory fake | - |
| `Platform` | `MacPlatform` | production | platform/mac.rs |
| `Platform` | `LinuxPlatform` | production | platform/linux.rs |
| `Platform` | `WindowsPlatform` | production | platform/windows.rs |
| `Platform` | `FakePlatform` | in-memory fake | - |
| `HerdrGateway` | `HerdrCli` | production | workspace::herdr |
| `HerdrGateway` | `FakeHerdr` | in-memory fake | - |
| `ProtectedFiles` | `FsProtectedFiles` | production | fsx, messaging::mailbox |
| `ProtectedFiles` | `TempDirProtectedFiles` | in-memory fake | - |
| `BinaryWatch` | `MtimeBinaryWatch` | production | - |
| `BinaryWatch` | `FakeBinaryWatch` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |

## 6. Domain events
### Published
No event of this context crosses to another context.

### Consumed
No event from another context is consumed. Requests arrive as calls on the driving ports.

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `FleetdHeartbeat` | fleetd-hang-detected, fleetd-restarted, horch-version-installed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Owns the ledger file, the events table, the ingested table and schema migrations (forward-only, backup first, 3 kept, newer schema refused, development build does not migrate). Each context decides the rows of its own tables; fleet-runtime commits them (organise N2). The platform layer lives at crates/horch-core/src/platform/ with mod.rs, mac.rs, linux.rs and windows.rs.

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| fitness | `fitness_only_fleet_runtime_opens_the_ledger_writable` | Exactly 1 process writes the ledger (RR-5, C4) |
| invariant | `cli_command_opens_ledger_read_only_and_sends_a_request` | Exactly 1 process writes the ledger (RR-5, C4) |
| fault injection | `fm12_hung_fleetd_restarted_on_each_os` | A hung fleetd is restarted on all 3 OSes (PL-5) |
| preservation | `pres_04_s10_changed_binary_restarts_between_passes` | The control loop restarts on a changed binary, only between passes (04 S10) |
| preservation | `pres_04_m5_foreign_owned_outbox_is_refused` | Mail and outbox dirs are 0700, files 0600; a foreign-owned dir is refused (04 M5) |
| preservation | `pres_06_s2_warns_on_herdr_resume_agents_on_restore` | horch warns when herdr resume_agents_on_restore is not false (06 S2) |
| preservation | `pres_06_s11_worker_report_is_a_file_on_every_os` | Worker reports reach fleetd as files, not sockets (06 S11) |
| preservation | `pres_06_s12_unknown_probe_neither_pauses_nor_passes` | A probe that returns unknown is reported, never healthy or unhealthy (06 S12, OR-5) |
| fitness | `fitness_fleetd_tree_has_no_os_cfg` | Core fleetd code has no OS cfg (PL-2, PL-3) |
| invariant | `platform_not_supported_is_explicit_on_each_module` | A missing OS equivalent returns NotSupported (PL-3) |
| invariant | `newer_schema_is_refused_and_backup_precedes_migration` | Schema migration: backup first, newer schema refused, dev build does not migrate (OR-6, C4) |
| invariant | `illegal_transition_is_refused_and_writes_nothing` | All writes go through the one-door transition with the allowed-transition table |
| invariant | `start_agent_process_inside_transaction_panics_in_debug` | No process start runs inside a ledger transaction (connect N4) |
| invariant | `only_the_fixed_doorbell_line_is_typed` | The only bytes typed into a terminal are the 1 fixed doorbell line (FR-7) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
herdr and the OS service managers are adopted systems behind HerdrGateway and Platform (strategize N1). Each OS module returns NotSupported where it has no equivalent, and core code decides (PL-3).

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is open.
- Assumptions: A1, A2, A5, A7, A9 · Open questions: Q1 to Q6 in code.json where they name this context.
