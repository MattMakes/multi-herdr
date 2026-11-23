# End review of phases 0, P and 1

## Summary

- Critical: 1 verified finding.
- Important: 4 verified findings.
- Minor: 0 findings.
- The code does not satisfy SR-4 or the event-driven half of RR-4, and the required cross-platform proof is incomplete.

## Findings

### R1-F1 — Critical — SR-4, SR-6

**Claim: verified.** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:1132` runs the complexity-sweep task check in the job process, explicitly with that process's rights, instead of using the worker's sandbox. `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs:121` accepts a result file by path containment, same-user ownership and size, and `:159` only binds the payload to the job id and run in its filename.

A concrete attack is a worker commit that makes `cargo clippy` execute a project-controlled build script. During complexity-sweep, that script has fleetd's access to `<fleet root>/run/jobs`, can inspect job filenames, and can write a passing result for another running gate job. The ingest validates the shape and matching id, but it cannot distinguish the real job process from another process of the same user. A forged passing gate can let unchecked worker code land, which gives a worker an operator effect.

Smallest fix: run complexity-sweep through the same `SandboxProfiles::check_launch` path as every other task check, including the operator denials. Add a test whose build script tries to write a result for a different running job and prove that the write fails and the gate remains unchanged.

### R1-F2 — Important — PL-3, RR-7

**Claim: verified.** `crates/horch-platform/src/channel.rs:131` documents that Windows cannot set a pipe write timeout. `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:373` treats `Unsupported` as permission to call blocking `write_all`, and `:167` joins that server thread without a bound during shutdown.

A concrete case is a Windows client that submits a valid request and then stops reading a reply larger than the remaining pipe buffer. The only channel thread blocks in `write_all`; later clients cannot connect, and a fleetd restart can hang while `ChannelServer::drop` joins the thread. Core code therefore does not decide a safe response to the platform's explicit unsupported value.

Smallest fix: make Windows reply writes cancellable or bounded in the platform implementation, and make server shutdown cancel the active write before joining. Add a Windows test with a non-reading client and a bounded server stop.

### R1-F3 — Important — RR-4

**Claim: verified.** `crates/horch/src/cmd/fleetd.rs:1162` runs a pass, then sleeps until `pass_period_s`; only `TERM` shortens the sleep. No file watcher, request arrival, or job completion wakes this loop.

A concrete case is a five-second period where `result.json` arrives immediately after a pass. Even though RR-4 requires reconciliation on file events as well as on a fixed period, the result waits almost five seconds. The same delay applies to an operator request file and an ended job. The test `rr4_tasks_complete_with_all_file_watch_events_dropped` proves periodic recovery from missed events, but it does not prove that a delivered event wakes a pass.

Smallest fix: give the loop a wake primitive fed by the platform watcher, request spool/channel, and child/job completion, with the period as the fallback deadline. Add one test with a long period that proves each event source starts a pass promptly.

### R1-F4 — Important — OR-2, design rule “one bad row fails only its row”

**Claim: verified.** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4558` places all integration work in one savepoint and rolls all of it back on an error. `crates/horch-core/src/fleetd/integration/application/features.rs:108` and `crates/horch-core/src/fleetd/integration/application/landing.rs:237` propagate an error from one feature or branch without a per-row savepoint.

A concrete case is one branch row whose referenced task or landing data cannot be read. Its `follow_one` error rolls back valid transitions for every other feature and branch selected in that pass. The same bad row is selected again, so unrelated landing state machines can hang indefinitely. Repair attempts already avoid this pattern with a per-branch savepoint at `crates/horch-core/src/fleetd/task_execution/application/repair.rs:323`.

Smallest fix: use a savepoint for each feature and branch, record one stable needs-you item for the failed row, advance the cursor, and continue. Add a test with one invalid row between two valid rows and prove both valid rows advance.

### R1-F5 — Important — PL-1, PL-3, PL-6

**Claim: verified.** The phase audit records no required Linux CI run id and no Windows run for PL-1, PL-3, or PL-6. The code and test definitions exist, but the design makes execution on macOS, Linux, and Windows part of each rule.

A concrete release can pass the local macOS gate while Windows pipe shutdown, service, sandbox, or `NotSupported` behavior remains broken. The missing proof is especially material because R1-F2 is Windows-only.

Smallest fix: run and record the required Linux and Windows CI jobs, run the PL-6 before/after comparison on all three operating systems, and keep these rules unverified until those runs pass.

## Watchlist lines covered

| Line | Point | Result |
| ---: | --- | --- |
| 4 | T-460 | checked, no fault: `crates/horch-platform/src/mac/process.rs:45` uses short BSD info so root parents remain readable; `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs:103` fails closed on unknown group or parent facts and compares wrapper start time (verified) |
| 16 | T-580 | checked, no fault: `crates/horch-platform/src/sandbox.rs:256` preserves the original command's explicit environment changes, and `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs:43` applies the final forbidden and repository-variable scrub before launch (verified) |
| 20 | T-462 | finding R1-F2 (verified) |
| 23 | T-363 | finding R1-F1 (verified) |
| 31 | T-472 channel stop | finding R1-F2 for Windows; Unix accept wake remains bounded at `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:167` (verified) |
| 32 | T-883 | finding R1-F1: the file checks do not identify the writing process (verified) |
| 45 | T-399 | checked, no design fault: rescue-ref retention is calendar time under PL-4, so `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4690` correctly does not use the fleet clock; deterministic injection is not verified (verified classification, not verified testability) |
| 50 | T-396/T-397 | checked, original fault fixed: `crates/horch-core/src/fleetd/task_execution/application/repair.rs:323` isolates each repair bind; finding R1-F4 covers remaining unisolated integration rows (verified) |
| 59 | T-393 | finding R1-F3 (verified) |
| 60 | T-896 | checked, original dispatcher fault fixed: `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:3486` reads committed request effects, while recovery at `:3716` derives pending effects from ledger rows; no remaining decision from an effect output was found in this scope (verified) |
| 63 | T-397 rescue queue | checked, no fault in the static path: `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:652` visits every orphaned row and queues its rescue when facts provide a HEAD; the reported flaky execution is not verified (not verified dynamically) |
| 69 | T-558 / FR-10 | checked, no fault: `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs:103` checks group membership before walking the parent chain and fails closed on unknown facts (verified) |
| 70 | T-898 / FR-10 | checked, no code fault in this scope; real Claude and Codex sandbox execution is not part of this review and remains unverified (not verified dynamically) |
| 72 | T-397 fetch job | checked, the job observer treats a missing process as abandoned and bounds retries; the reported 1-of-4 stall cause is not established by static evidence (not verified) |
| 74 | T-898 process group | checked, wrapper identity comparisons include PID start time; group-id reuse behavior has no cross-platform execution proof in this review (not verified dynamically) |
| 75 | T-898 G1 | checked, the token, protected file reader, and sandbox denials are the intended layered boundary; finding R1-F1 is a concrete path that breaks the sandbox layer for complexity-sweep (verified) |

## Tests that do not prove their rule

| Rule | Test | What is not proven |
| --- | --- | --- |
| RR-4 | `rr4_tasks_complete_with_all_file_watch_events_dropped` | It proves the fixed-period fallback only. It does not prove that a delivered file, request, or job event wakes the pass (verified). |
| SR-4 | task-check sandbox tests | They exercise the normal `task_check` path, not the complexity-sweep branch that bypasses `SandboxProfiles` (verified). |
| SR-6 | job result protected-file tests | They prove type, link, owner, size, id, and run checks. They do not prove that the expected job process wrote the file (verified). |
| PL-1 | `cargo test --workspace --locked` CI jobs | No required Linux or Windows run id is recorded for this gate (verified). |
| PL-3 | `not_supported_*` and `*_not_supported_on_windows` | Unit tests establish explicit values, but no Windows run proves that core callers handle them safely; R1-F2 shows one caller that does not (verified). |
| PL-6 | `scripts/pl6/check.sh` and `pl6.py compare` | Windows has no run, and Linux has no recorded CI run id (verified). |
| FM-12 | `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` | It proves that a long job does not block the pass. It does not cover a blocked channel reply thread or bounded channel shutdown (verified). |

## Operator decisions

- **Event wake latency (verified decision needed).** Recommend implementing prompt wakes because RR-4 already says “as well as on file events.” If only periodic polling is intended, change RR-4 and define the maximum accepted latency.
- **Windows channel reply behavior (verified decision needed).** Recommend a cancellable bounded write and bounded shutdown. Silently accepting an unbounded write is inconsistent with PL-3.
- **Cross-platform release proof (verified requirement, no product choice).** Keep PL-1, PL-3, and PL-6 red until Linux and Windows runs are recorded.
- **Calendar-time injection (not verified as required).** Recommend a calendar-clock port for deterministic rescue-retention tests, while keeping retention on calendar time rather than the fleet clock.
