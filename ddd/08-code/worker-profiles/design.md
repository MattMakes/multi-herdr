# Design - Worker profiles and adapters (`worker-profiles`)

| | |
|---|---|
| Classification | supporting (compliance-enforcer) · sourcing build · pattern **active-record** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/worker_profiles` |
| Structure | records + use cases (active record) plus the fleet-run wrapper |
| Purpose | Worker profiles turns 1 named worker profile into the exact CLI, flags, environment, config files and sandbox of each agent launch, so that every agent runs on the operator's subscription logins and never with ANTHROPIC_API_KEY (C2). It serves ideas and task-execution, which ask it to start each agent process: it resolves the launch environment, runs the fleet-run wrapper and starts it through the platform layer. It keeps the adapters of every agent CLI correct by docs sync and conformance tests, and marks a profile unhealthy when its adapter fails. It does not choose which task runs (task-execution). - canvas: `ddd/07-define/worker-profiles/bounded-context-canvas.md` |
| Domain roles | enforcer |

## 1. Module layout
```
crates/horch-core/src/fleetd/worker_profiles/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    profile.rs         profile record, health, fallback
    launch_env.rs      builds the scrubbed launch environment
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
- ANTHROPIC_API_KEY is removed from the environment of every launch: attempt, sub-orchestrator pane, detached child, updater and probe. The roster rejects a teammate file that sets it (05 E-10, 06 S1, understand N16, decompose N2).
- agy children lose GEMINI_API_KEY, GOOGLE_API_KEY and GOOGLE_GEMINI_BASE_URL, and the roster rejects them (06 S3). Secrets reach agents only through env vars that the profile names, never through task files (SR-2).
- A profile must declare CLI, model, skills, MCP servers, sandbox, writable paths, context budget, timeouts and restart policy, and every worker persona must name a fallback persona; the roster refuses an incomplete profile (FR-6).
- A sandboxed persona does not start on a host that cannot sandbox it (06 S9). Each worker writes only to its task directory, its worktree and the paths its profile names (SR-1).
- When an adapter conformance test fails, its profile is marked unhealthy, and dispatch does not choose an unhealthy profile until a conformance test passes. A waiting_approval stall also marks the profile unhealthy.
- The private CODEX_HOME links every entry of the real codex home except rules/, the Prime agent dir links auth.json, and every codex launch gets --dangerously-bypass-hook-trust (06 S4, S5, S6). Trust writes stay narrow (06 S10).
- A new agent CLI release is used only after the update cooldown of more than 1 week; a CLI with unreadable release dates is held back. Pane auto-update stays off for claude and codex (06 S7, S8, C10).
- worker-profiles owns start-agent-process and the fleet-run wrapper (D3 option B, A11). The wrapper builds the private config home, scrubs the environment, maps the CLI events of its adapter, writes heartbeat.json every 10 s and exit.json, and stops or kills its process group through the platform interface. 1 process per attempt_id; a start for a live recorded process is a no-op.
- Each agent runs with a context limit of 256k tokens, set through the CLI's own setting where one exists; else the wrapper ends the attempt with a handoff at 256k (FR-9). The OpenCode, pi and Prime adapters carry over (06 S15).

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `RunConformanceTests` | `run-conformance-tests` | Daily plus on any CLI version change. | store rows | see §2 |
| `MarkProfileUnhealthy` | `mark-profile-unhealthy` | Blocks new dispatch on the profile. | store rows | see §2 |
| `StartAgentProcess` | `start-agent-process` | Resolves the launch environment, writes the private config home and starts the fleet-run wrapper (or the sub-orchestrator pane) in its own process group through the platform interface. Runs after the ledger transaction commits. 1 process per attempt_id (A11). | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `WorkerProfilesCommands` | run-conformance-tests, mark-profile-unhealthy | ReconcilePassDriver |
| `LaunchEnvironmentQuery` | launch-environment: the scrubbed env, private config home, CLI argv, sandbox profile, context limit of a profile; answered to task-execution and ideas (connect message launch-environment) | InProcessWorkerProfiles |
| `AgentProcesses` | start-agent-process and stop for task-execution and ideas (connect message start-agent-process, in-process) | InProcessWorkerProfiles |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `WorkerProfilesStore` | profile health rows | persistence, collaborator or infrastructure need named in the operations |
| `RosterSource` | reads and validates teammate files (roster, harness defaults) | persistence, collaborator or infrastructure need named in the operations |
| `HarnessAdapters` | per-CLI launch builders: claude, codex, antigravity, opencode, pi, prime | persistence, collaborator or infrastructure need named in the operations |
| `ConformanceRunner` | runs the adapter conformance suite against an installed CLI | persistence, collaborator or infrastructure need named in the operations |
| `ParentEnvironment` | the environment of the fleetd process, read only to build a scrubbed child env | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | update cooldown of more than 1 week (06 S7) | persistence, collaborator or infrastructure need named in the operations |
| `ProcessHost` | starts, stops and kills a process group through the platform interface; opens a pane through the fleet-runtime HerdrGateway | persistence, collaborator or infrastructure need named in the operations |
| `HeartbeatPublisher` | the fleet-run wrapper writes heartbeat.json every 10 s (heartbeat-written) and exit.json, atomic, in the attempt outbox | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `WorkerProfilesCommands` | `ReconcilePassDriver` | production | - |
| `LaunchEnvironmentQuery` | `InProcessWorkerProfiles` | production | - |
| `WorkerProfilesStore` | `SqliteProfileHealthStore` | production | - |
| `WorkerProfilesStore` | `InMemoryProfileHealthStore` | in-memory fake | - |
| `RosterSource` | `TeammateFileRoster` | production | roster:: |
| `RosterSource` | `FixtureRoster` | in-memory fake | - |
| `HarnessAdapters` | `HarnessLaunchBuilders` | production | harness:: |
| `HarnessAdapters` | `FakeHarness` | in-memory fake | - |
| `ConformanceRunner` | `CliConformanceRunner` | production | - |
| `ConformanceRunner` | `ScriptedConformance` | in-memory fake | - |
| `ParentEnvironment` | `ProcessParentEnvironment` | production | - |
| `ParentEnvironment` | `FixtureEnvironment` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `AgentProcesses` | `InProcessWorkerProfiles` | production | - |
| `ProcessHost` | `PlatformProcessHost` | production | crates/horch-platform |
| `ProcessHost` | `FakeProcessHost` | in-memory fake | - |
| `HeartbeatPublisher` | `FleetRunWrapper` | production | crates/horch/src/cmd/fleet_run.rs |
| `HeartbeatPublisher` | `InMemoryHeartbeats` | in-memory fake | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `heartbeat-written` | use case that emits it (§3) | task-execution | task, attempt, pid, pgid, mux_pane, proc_state, agent_state, last_output_at, last_tool_event, tokens, seq, written_at | at-most-once | `contract: heartbeat-written` |

### Consumed
No event from another context is consumed. Requests arrive as calls on the driving ports.

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `ProfileHealth` | adapter-conformance-failed, profile-marked-unhealthy | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
A profile_health table (new) holds the unhealthy mark and the last conformance result. Profiles themselves stay teammate files read through RosterSource.

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| fault injection | `fm_anthropic_api_key_reaches_no_child_process` | ANTHROPIC_API_KEY never reaches a child process (05 E-10, 06 S1) |
| preservation | `pres_05_e10_roster_rejects_a_teammate_file_that_sets_it` | ANTHROPIC_API_KEY never reaches a child process (05 E-10, 06 S1) |
| preservation | `pres_06_s3_agy_env_is_scrubbed_and_roster_rejects` | agy children lose GEMINI_API_KEY, GOOGLE_API_KEY, GOOGLE_GEMINI_BASE_URL (06 S3) |
| preservation | `pres_06_s4_codex_home_links_all_but_rules` | The private CODEX_HOME links every entry except rules/ (06 S4) |
| preservation | `pres_06_s5_prime_links_auth_json` | The Prime agent dir links auth.json (06 S5) |
| preservation | `pres_06_s6_codex_launch_bypasses_hook_trust` | Every codex launch gets --dangerously-bypass-hook-trust (06 S6) |
| preservation | `pres_06_s7_cli_release_younger_than_cooldown_is_held` | Agent CLI updates wait out a cooldown; unreadable dates are held back (06 S7) |
| preservation | `pres_06_s8_pane_auto_update_off` | Pane auto-update is off for claude and codex (06 S8) |
| preservation | `pres_06_s9_sandboxed_persona_refused_without_sandbox` | A sandboxed persona does not start on a host that cannot sandbox it (06 S9) |
| preservation | `pres_06_s10_trust_never_for_root_home_or_ancestor` | Trust writes stay narrow (06 S10) |
| preservation | `pres_06_s15_opencode_pi_prime_adapters_build_a_launch` | The OpenCode, pi and Prime adapters carry over (06 S15) |
| invariant | `profile_without_fallback_is_refused` | Every worker persona names a fallback; an incomplete profile is refused (FR-6, RR-11) |
| invariant | `unhealthy_profile_is_skipped_by_dispatch` | An unhealthy profile is not dispatched until a conformance test passes |
| invariant | `launch_environment_sets_256k_context_limit` | Each agent runs with a 256k context limit (FR-9) |
| invariant | `start_agent_process_for_a_live_attempt_is_a_no_op` | 1 process per attempt_id; a start for a live recorded process is a no-op |
| invariant | `wrapper_scrubs_env_and_builds_config_home_before_exec` | The fleet-run wrapper scrubs the environment and builds the private config home before the CLI starts (A11) |
| contract | `published heartbeat-written carries exactly task, attempt, pid, pgid, mux_pane, proc_state, agent_state, last_output_at, last_tool_event, tokens, seq, written_at (connect.json payload) and nothing consumers did not ask for` | contract: heartbeat-written |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Wraps roster:: and harness:: behind RosterSource and HarnessAdapters. The launch environment is built from ParentEnvironment with ANTHROPIC_API_KEY removed in 1 place, so every launch path (attempt, sub-orchestrator pane, probe, updater) gets the same scrub (decompose N2). The fleet-run wrapper is a process role of the horch binary with its composition root at crates/horch/src/cmd/fleet_run.rs; processes start and stop through ProcessHost over the platform interface.

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A11 · Open question: Q3 in code.json where it names this context.
