# Phase 5 end review

## Summary

- Critical: 1. [verified]
- Important: 4. [verified]
- Minor: 0. [verified]

## Findings

### R4-F1 — Critical — RR-8, FM-15

**Location:** `crates/horch-core/src/execution/store.rs:995`; `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs:180`. [verified]

`ExecutionStore::update` reports success as soon as `session_records` is durably queued. If another process changes a session row before fleetd starts, the queued compare-and-swap is refused. The spool then archives the request, and the original caller has already returned success. A session update is lost without a retry, an error to its caller, or a needs-you item. A malformed or permission-rejected queued file has the same invisible terminal state because its path is recorded as ingested and never read again. [verified]

The smallest fix is to make every rejected or refused queued state change visible and recoverable. Keep the request for reconciliation or retry, and open one needs-you item that identifies the request and refusal. Do not describe a queued session update as committed success. [verified]

### R4-F2 — Important — RR-8, FM-15

**Location:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:167`. [verified]

`ChannelServer::end` sets its stop flag, reconnects to the socket path to wake `accept`, and then joins the server thread. If the socket folder or socket file was removed while the listener remained open, the reconnect fails and `accept` stays blocked. Drop or explicit stop then waits forever in `join`, so fleetd can hang during shutdown or generation replacement. [verified]

The smallest fix is a wake mechanism that does not depend on the bound pathname, such as a retained wake handle or a nonblocking listener with a bounded stop poll. Add a test that removes the socket folder before stopping the server. [verified]

### R4-F3 — Important — SR-5

**Location:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs:116`; `crates/horch-platform/src/windows/fs.rs:102`. [verified]

Every request file is refused on Windows because the protected reader requires a mode, while the Windows platform adapter always returns `NotSupported` for `mode_of`. When fleetd is down, a Windows operator can queue a request file but fleetd can never ingest it. The SR-5 fallback channel is therefore not implemented on a supported platform. [verified]

The smallest fix is a Windows protected-file rule based on owner SID and ACLs, with the same fail-closed link, type, containment, and size checks. If Windows fleetd is not a release target, state that product limit in the design and service command instead. [verified]

### R4-F4 — Important — SR-4, RR-5

**Location:** `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:315`; `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1404`. [verified]

Binding a task calls the plain `taskledger::git::Git::rev` while the fleet pass holds its ledger transaction. This is an external project operation inside the single-writer transaction, and it does not use the sandboxed fleetd git gateway. A slow or stuck `git rev-parse` blocks every ledger write and can hang the whole fleet pass. [verified]

The smallest fix is to read the base tip before opening the write transaction through the sandboxed fleetd git gateway, then pass the immutable tip into the row decision and validate that the target row is still eligible. [verified]

### R4-F5 — Important — SR-8, FM-32

**Location:** `crates/horch-core/src/execution/store.rs:1029`; `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs:90`. [verified]

The worker wrapper writes session state by sending the operator-class `session_records` request, which makes the wrapper read the operator token. Channel authorization rejects a live attempt peer, but a fallback request file is authorized by token and file ownership alone. This contradicts the Phase 5 boundary that a worker wrapper writes attempt files for fleetd to ingest, and it makes sandbox isolation the only fact separating the wrapper's operator credential from agent code. [verified]

The smallest fix is a worker-owned session-record file under the attempt folder, protected and ingested like other worker files. The wrapper must not read the operator token or write `run/requests`. [verified]

## Watchlist lines covered

| Line | Point | Result |
|---:|---|---|
| 3 | T-484 | checked, no fault: `crates/horch-core/src/harness/claude.rs:1293` denies the ledger, WAL, SHM, journal, and the fleet run folder; an unrelated new sibling file is not trusted by SQLite. [verified] |
| 5 | T-457 | checked, no fault: `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_wire.rs:1` keeps JSON and SHA-256 outside the domain module. [verified] |
| 9 | T-456 | finding R4-F3. [verified] |
| 12 | T-455 | checked, no fault: `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:207` and `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs:458` require token, channel, and request-folder denials. [verified] |
| 15 | T-485 | checked, no source fault: `crates/horch-core/src/harness/codex.rs:1261` pins and probes the minimum Codex version; behavior in a real future Codex is not verified. |
| 17 | T-466 | checked, no fault: `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:166` classes these kinds as operator, and `authorize.rs:78` refuses worker kinds and live attempt peers. Run-close and gate request kinds are retired. [verified] |
| 18 | T-468 | finding R4-F4. [verified] |
| 21 | T-481 | checked, no fault: `crates/horch-core/src/harness/claude.rs:1029` limits a report pane to ledger read, task-folder write, and its report socket; `claude.rs:1293` still seals operator files. [verified] |
| 27 | T-472 | checked, no fault: `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:166` classes all 4 spawn record kinds as operator, and `authorize.rs:78` blocks live attempt peers. Payload handlers validate referenced rows. [verified] |
| 31 | T-472 | finding R4-F2. [verified] |
| 33 | T-885 | checked, no fault: `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_cli.rs:41` registers the 4 operator handlers, whose strict payload parsers validate IDs and referenced rows; `authorize.rs:78` blocks worker access. [verified] |
| 38 | 8 request kinds | operator decision OD-1; the source now has 36 total kinds, not 42, at `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:85`. [verified] |
| 42 | T-884 | findings R4-F1 and R4-F5: `worker_checkin` and `worker_exit` are absent, but `session_records` remains operator-class. [verified] |
| 49 | T-884 | finding R4-F1. [verified] |
| 52 | T-884/T-890 | finding R4-F5. [verified] |
| 54 | T-890 | checked, no fault: `crates/horch-core/src/execution/store.rs:328` gives an explicit ledger priority, and `crates/horch-core/src/runtime/paths.rs:104` exports both fleet paths. [verified] |
| 55 | T-890 | checked, no source fault: `execution/store.rs:2053` proves explicit-ledger precedence with a map environment; inherited live shell state across every test harness is not verified. |
| 57 | T-870 | checked, no fault: `crates/horch/src/cmd/taskledger.rs:578` opens command ledgers read-only. Workspace adoption is an explicit `run_adopt` request at `taskledger.rs:1410`, not a hidden database write. [verified] |

## Tests that do not prove their rule

| Rule | Test | Missing proof |
|---|---|---|
| RR-8 / FM-15 | `session_records_applies_a_row_only_when_it_is_as_read` | It proves compare-and-swap refusal in-process, but not visibility, retry, or recovery after a queued request is refused. [verified] |
| RR-8 / FM-15 | Channel server start/stop unit tests | They stop with the socket pathname intact; no test removes the socket folder before `stop` or drop. [verified] |
| SR-5 | protected-file reader tests | Unix mode tests prove fail-closed input checks, but no Windows test proves that a valid operator request file can be accepted. [verified] |
| SR-4 / RR-5 | dispatch binding tests | They use `Git::new("git")` and prove the selected base tip, but not sandboxing or that no external process runs while the write transaction is held. [verified] |
| SR-8 / FM-32 | request authorization and operator-boundary adapter tests | They prove channel peer refusal and sandbox deny rules separately, but not that the wrapper never receives or reads the operator token. [verified] |
| SR-8 / FM-32 | Codex adapter version and rendering tests | They prove rendered configuration and a feature probe, but only a real CLI sandbox probe can prove enforcement after a Codex update. [verified] |
| RR-5 | `trf_45_session_db_is_the_task_ledger_unless_the_state_dir_moved` | It uses a synthetic environment and does not prove that every test harness strips an inherited live `HORCH_LEDGER_DB`. [verified] |

## Operator decisions

### OD-1 — Unlisted request kinds

Keep `worker_progress`, `worker_done`, `run_adopt`, and `question_answer`, and add them to the Phase 6 MAP because current CLI flows use them. Retire `spawn_adopt`, which has no client. Keep the other 3 spawn record kinds only if the fleetd launch protocol still emits each one; otherwise retire them. The current catalog accepts all 8 as operator kinds. [verified recommendation; client-use inventory not fully verified]

### OD-2 — Windows support

Decide whether Windows fleetd is a release target. The recommendation is to implement ACL-based protected-file reads before claiming Windows support. Otherwise, document that request-file fallback and sandboxed workers are unavailable there. [verified recommendation]

### OD-3 — Real CLI boundary gate

Require one real Claude and one real Codex operator-boundary conformance run before release and after supported CLI version changes. Fake CLIs and rendered-policy tests cannot prove the external sandbox implementation. [verified recommendation]
