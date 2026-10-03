# U25 a6c-presentation: report

Phase A6, commit 5 (telemetry and presentation). Requirements ARC-23 and
ARC-22 (the scan part). Branch `ard/a6c-presentation`.

## Result

- Telemetry and the `cost`, `usage`, `sessions` and `telemetry` commands read
  execution records through `execution::store`.
- "Live" is `is_live()` (Starting, Running). "Done" is `is_terminal()` (Done,
  Failed, LaunchFailed). No code in `crates/horch/src` or
  `crates/horch-core/src/telemetry` compares `status` with `"working"` or
  `"done"`.
- `telemetry/readers.rs` has no `crate::agent` (`ProcessEnv`) call.
- Every output oracle and golden is unchanged. `HORCH_REQUIRE_GIT=1
  HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN` after each commit.

## Commits

1. `A6: Read ledgers through the execution store`
2. `A6: Decide live and done from the typed status`
3. `A6: Reports read the store`
4. `A6: Remove the ProcessEnv shim from telemetry readers`
5. `A6: Add ARC-22 and ARC-23 tests`
6. This report.

## New API

| Item | Where | What |
|---|---|---|
| `ledger_paths(state_root)` | `execution/store.rs` | sorted `*.json` files directly under the state root, without `policy.json`. A directory is never a ledger, so `multi-herdr/` is skipped (OD3). |
| `read_ledger_file(path)` | `execution/store.rs` | `Option<Vec<LedgerRecordV1>>`. `None` when the file does not read or parse; an empty file is `Some(vec![])`. |
| `read_all_ledgers(state_root)` | `execution/store.rs` | `Vec<(PathBuf, Vec<LedgerRecordV1>)>` of the ledgers that parse now. |
| `ExecutionStore::open_in(ctx)` | `execution/store.rs` | the store for `ctx.paths.state_root` and `ctx.paths.project()`. |
| `done_record_ids(records)` | `telemetry/collect.rs` | ids whose typed status is terminal. The snapshot, `horch usage` and the telemetry project filter share it. |
| `Locations::sqlite3` | `usage.rs` | the `sqlite3` CLI. `from_context` takes `ctx.bins.harness.sqlite3`; `under_home` uses `sqlite3`. |
| `opencode_poll(db, sqlite3, ..)` | `telemetry/readers.rs` | new `sqlite3` parameter. |

The "last good copy" stays in `Collector` (`last_good`), because a free
function has no state across ticks. `Collector::ledgers` calls
`ledger_paths` and `read_ledger_file`.

## Behavior changes (intended)

- A live row needs `is_live()`. A Planned record is no longer a live row.
- A Failed or LaunchFailed record is never a live row, also with a recent
  event. A Done record with an event in the last 10 minutes is still a live
  row, as before.
- `Ledger::assign` and `Ledger::supersede_orchestrators` select live records
  only. A Planned record is not assigned and not superseded.
- For records without `state` the result is the same as before, because
  `from_legacy("working")` is Running and `from_legacy("done")` is Done.

## Risk 5: `horch sessions`

- The text render is unchanged. It prints only the legacy words.
- `horch sessions --json` renders through `ExecutionStore::render_json`. A
  record with a typed status already carries it under the key `state`
  (additive, from A6a). A record without `state` gets no key. The JSON oracle
  bytes did not change, because no oracle record has `state`.

## ARC-22: ALLOWED list

`crates/horch-core/tests/arch_scan.rs` scans the non-test code of
`horch-core/src` and `horch/src`. It flags `.to_string().contains(`,
`format!("{..}").contains(`, `.contains("error` and `<e|err|error|why|cause>.to_string() ==`.

| File | Text | Reason |
|---|---|---|
| `horch-core/telemetry/readers.rs` | `FreeUsageLimitError` | The `error` field of an OpenCode message row is external data from OpenCode's database, not a Rust error value. The name is the only signal. |

The test fails when an ALLOWED entry matches nothing, so the list can only
shrink. `arc_22_matcher_finds_error_text_checks` tests the matcher itself.

## Tests

- `crates/horch-core/tests/telemetry_executions.rs`:
  - `arc_23_telemetry_reads_executions`: a state root with 1 record in each of
    6 typed states, an old-format ledger, and a ledger file inside
    `multi-herdr/`. A collector tick gives live rows for `starting`,
    `running` and the old `working` record only. The dataset file is not read.
  - `arc_23_failed_executions_are_never_live`: `build_snapshot` with recent
    events. Done and Planned with a recent event show; Failed and
    LaunchFailed do not.
- `crates/horch-core/tests/arch_scan.rs`: `arc_22_no_error_string_matching`,
  `arc_22_matcher_finds_error_text_checks`.

## Gotchas

- `done_record_ids` uses `from_legacy`, which maps every status except
  `"working"` to Done. A record with an unknown status word (for example an
  empty string) now counts as done. Before, only `"done"` counted. Real
  ledgers hold only `working` and `done`.
- `tests/common/mod.rs` fills `Locations::sqlite3` with
  `horch_core::agent::sqlite3_bin()`, so the tests still honor
  `HORCH_SQLITE3_BIN`. That is test code; A12 removes the shim.
- `crates/horch-core/tests/execution_store.rs:15` has an unused import
  warning (`KIND_ORCHESTRATOR`). It came from A6a; I did not touch the file.

## Outside my scope (not fixed)

- `crates/horch/src/cmd/spawn.rs:115` compares `record.status == STATUS_WORKING`.
  The A6 service unit owns `spawn.rs`.
- `crates/horch/src/cmd/smoke.rs:748` compares `record.status == STATUS_DONE`.
- `crate::agent::sqlite3_bin` and the other `ProcessEnv` shims still exist
  for `workspace/herdr.rs`, `opencode.rs`, `prime.rs` and `tests/common`.

## Checklist

- [x] `just gate` is green on this commit (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`).
- [ ] Every ID of this phase has a test (`scripts/check-req-coverage.sh --phase <P>` exits 0). (n/a: ARC-15, ARC-16, ARC-18, ARC-26 belong to other A6 units; ARC-17, ARC-22, ARC-23 are covered)
- [x] Every new test name starts with its lowercase requirement ID (`ARC-02` -> `arc_02_...`).
- [x] No golden prompt changed; any prose change is a named sanctioned block.
- [x] No serialization golden re-blessed; a format change bumped its schema version.
- [x] No oracle under `crates/*/tests/oracles/` regenerated.
- [x] No existing test changed to make it pass, unless the unit plan says so. (The `Locations` literals in `tests/common/mod.rs`, `usage.rs` and `cost.rs` tests gained the new field; the orchestrator granted `tests/common/mod.rs`.)
- [x] No new crate outside the allowlist (`scripts/check-deps.sh` and `nfr_05`, `nfr_06`, `nfr_09`, `nfr_11` pass).
- [x] No new `std::env` read outside `runtime/` and the binary's bootstrap.
- [x] No `ANTHROPIC_API_KEY` reaches a child process (`FORBIDDEN_ENV` strips it).
- [x] Tests are hermetic: no network, no real harness binary, no herdr server, no file outside a temp dir; real `git` only on temp repos.
- [x] Every moved module left a re-export shim (until A12). (n/a: no module moved)
- [x] Old ledgers, old briefs and old teammate frontmatter still load.
- [x] New `pub mod` lines in `crates/horch-core/src/lib.rs` are in alphabetical order; no other line changed. (n/a: lib.rs unchanged)
- [x] The diff was re-read adversarially; the unit report lists gotchas.
