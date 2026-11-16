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
- Each process that fleetd, the fleet-run wrapper, a job, docs sync or the conformance suite starts goes through the 1 launcher, which applies the environment rule of the profile and always removes ANTHROPIC_API_KEY. fleetd does not use the native agent resume of herdr (SR-7, A29).
- The sandbox of each worker denies reading the operator token, connecting to the command channel and writing into the request folder. Profile validation refuses a profile whose sandbox allows 1 of them, and a profile on an OS where its sandbox is not supported: a worker never runs without its sandbox (SR-8, A35, A37).
- docs sync is a horch subcommand that fleetd schedules daily and on a new CLI version as a docs_sync job; a conformance job follows, with the check Operator boundary for each adapter. worker-profiles marks a profile unhealthy when a check fails and healthy again when the checks pass; a drift becomes an idea (A31, A33).

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `RunConformanceTests` | `run-conformance-tests` | A conformance job after each docs_sync job and on a new version of the CLI, keyed by the CLI, its version and the hash of its adapter (A33). docs sync is a horch subcommand that fleetd schedules daily as a docs_sync job. The suite has 1 check Operator boundary for each adapter (A35). A failure publishes adapter-conformance-failed, and a drift becomes an idea. | store rows | see §2 |
| `MarkProfileUnhealthy` | `mark-profile-unhealthy` | Blocks new dispatch on the profile: a failed conformance check or a waiting_approval stall. | store rows | see §2 |
| `StartAgentProcess` | `start-agent-process` | Resolves the launch environment, writes the private config home and starts the fleet-run wrapper (or the sub-orchestrator pane) in its own process group through the platform interface. Runs after the ledger transaction commits. 1 process per attempt_id (A11). Each process starts through the 1 launcher, which applies the environment rule of the profile and always removes ANTHROPIC_API_KEY; no native herdr resume (SR-7). A worker never runs without its sandbox (A37). | store rows | see §2 |
| `MarkProfileHealthy` | `mark-profile-healthy` | Marks a profile healthy again when the conformance checks of its adapter pass; dispatch resumes (design: Docs sync and adapter conformance). | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `WorkerProfilesCommands` | run-conformance-tests, mark-profile-unhealthy, mark-profile-healthy; the docs_sync and conformance jobs and their job-abandoned and job-requeued events | ReconcilePassDriver |
| `LaunchEnvironmentQuery` | launch-environment: the scrubbed env, private config home, CLI argv, sandbox profile, context limit of a profile; answered to task-execution and ideas (connect message launch-environment) | InProcessWorkerProfiles |
| `AgentProcesses` | start-agent-process and stop for task-execution and ideas (connect message start-agent-process, in-process) | InProcessWorkerProfiles |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `WorkerProfilesStore` | profile health rows | persistence, collaborator or infrastructure need named in the operations |
| `RosterSource` | reads and validates teammate files (roster, harness defaults); refuses a profile whose sandbox allows reading the operator token, connecting to the command channel or writing into the request folder, and a profile on an OS where its sandbox is not supported (SR-8, A35, A37) | persistence, collaborator or infrastructure need named in the operations |
| `HarnessAdapters` | per-CLI launch builders: claude, codex, antigravity, opencode, pi, prime | persistence, collaborator or infrastructure need named in the operations |
| `ConformanceRunner` | runs the adapter conformance suite against an installed CLI as a conformance job, with the check Operator boundary for each adapter (A33, A35) | persistence, collaborator or infrastructure need named in the operations |
| `ParentEnvironment` | the environment of the fleetd process, read only to build a scrubbed child env | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | calendar time: the update cooldown of more than 1 week (06 S7) and the daily docs sync; awake time from the platform layer for max_minutes of the wrapper (PL-4, A25) | persistence, collaborator or infrastructure need named in the operations |
| `ProcessHost` | the 1 launcher (SR-7): each process that fleetd, the fleet-run wrapper, a job, docs sync or the conformance suite starts goes through it; it starts, stops and kills a process group through the platform interface and opens a pane through the fleet-runtime HerdrGateway | persistence, collaborator or infrastructure need named in the operations |
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
| `attempt-exited` | use case that emits it (§3) | task-execution | attempt_id, exit_status, has_result | at-least-once | `contract: attempt-exited` |
| `launch-environment` | use case that emits it (§3) | task-execution, ideas | profile, task_id\|idea_id, attempt_id? | sync | - |
| `heartbeat-written` | use case that emits it (§3) | task-execution | task, attempt, pid, pgid, mux_pane, proc_state, agent_state, last_output_at, last_tool_event, tokens, seq, written_at | at-most-once | `contract: heartbeat-written` |
| `adapter-conformance-failed` | use case that emits it (§3) | ideas | cli, version, failed_checks | at-least-once | `contract: adapter-conformance-failed` |
| `profile-marked-unhealthy` | use case that emits it (§3) | task-execution | profile_id, reason | at-least-once | `contract: profile-marked-unhealthy` |
| `profile-marked-healthy` | use case that emits it (§3) | task-execution | profile_id | at-least-once | `contract: profile-marked-healthy` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `job-abandoned` | fleet-runtime | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the owner context reads the job row; 1 needs-you item per job key - test `idempotent: job-abandoned` |
| `job-requeued` | fleet-runtime | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the next run starts once per job key - test `idempotent: job-requeued` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `ProfileHealth` | adapter-conformance-failed, profile-marked-unhealthy, adapter-conformance-passed, profile-marked-healthy | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
A profile_health table (new) holds the unhealthy mark and the last conformance result. Profiles themselves stay teammate files read through RosterSource. The docs_sync and conformance jobs are rows of the jobs table of fleet-runtime, with no repository; their subject is the CLI and its version (A33).

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
| contract | `published attempt-exited carries exactly attempt_id, exit_status, has_result (connect.json payload) and nothing consumers did not ask for` | contract: attempt-exited |
| contract | `published adapter-conformance-failed carries exactly cli, version, failed_checks (connect.json payload) and nothing consumers did not ask for` | contract: adapter-conformance-failed |
| contract | `published profile-marked-unhealthy carries exactly profile_id, reason (connect.json payload) and nothing consumers did not ask for` | contract: profile-marked-unhealthy |
| contract | `published profile-marked-healthy carries exactly profile_id (connect.json payload) and nothing consumers did not ask for` | contract: profile-marked-healthy |
| idempotency | `handling job-abandoned twice (same job key) has the same effect as once` | idempotent: job-abandoned |
| idempotency | `handling job-requeued twice (same job key) has the same effect as once` | idempotent: job-requeued |
| invariant | `profile_whose_sandbox_allows_the_operator_path_is_refused` | Profile validation refuses a profile whose sandbox allows reading the operator token, connecting to the command channel or writing into the request folder (SR-8, A35) |
| invariant | `profile_on_an_os_without_its_sandbox_is_refused` | A worker never runs without its sandbox: a profile on an OS where its sandbox is not supported is refused there (SR-8, A37) |
| invariant | `conformance_operator_boundary_check_runs_for_each_adapter` | The conformance suite has 1 check Operator boundary for each adapter (A35) |
| fitness | `fitness_no_process_start_outside_the_launcher` | Each process starts through the 1 launcher, which always removes ANTHROPIC_API_KEY; no native herdr resume (SR-7, A29) |
| invariant | `no_child_of_a_worker_job_check_docs_sync_or_conformance_has_the_forbidden_key` | Each process starts through the 1 launcher, which always removes ANTHROPIC_API_KEY; no native herdr resume (SR-7, A29) |
| invariant | `profile_is_marked_healthy_again_when_conformance_passes` | worker-profiles marks a profile unhealthy when a conformance check fails and healthy again when the checks pass (design: Docs sync and adapter conformance) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Wraps roster:: and harness:: behind RosterSource and HarnessAdapters. The launch environment is built from ParentEnvironment with ANTHROPIC_API_KEY removed in 1 place, so every launch path (attempt, sub-orchestrator pane, probe, updater) gets the same scrub (decompose N2). The fleet-run wrapper is a process role of the horch binary with its composition root at crates/horch/src/cmd/fleet_run.rs; processes start and stop through ProcessHost, the 1 launcher, over the platform interface. Publishes attempt-exited and heartbeat-written to task-execution as files, adapter-conformance-failed to ideas, and profile-marked-unhealthy and profile-marked-healthy to task-execution. Its jobs run through the JobQueue of fleet-runtime (A16).

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A11, A16 · Open questions: none (Q3 is answered, see deprecated[] in code.json).
