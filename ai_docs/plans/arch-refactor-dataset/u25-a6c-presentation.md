# U25 a6c-presentation: telemetry and CLI reports read executions through the store

Unit slug: `a6c-presentation`. Branch: `ard/a6c-presentation`. Phase: A6 (commit 5).
Requirements: ARC-23, ARC-22 (the scan part).

## GOAL

Telemetry and the `cost`, `usage` and `sessions` commands read execution
records through `execution::store` and decide "live" with the typed status
(`is_live()`), not raw `status == "working"`. No application code matches on
error message text. Every output stays byte-identical on existing fixtures.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "A6" commit 5 (telemetry and
  presentation), §4 ARC-22 and ARC-23 rows, §6 risk 5.
- Merged reports (`ai_docs/reports/arch-refactor-dataset/`): `a6a-store.md`
  (`ExecutionStore` API: `for_project`, `read`, `render_json`, `update`,
  `update_key`, `insert`, `get`, `set_state`, `set_skills`, `live`; typed
  `state` nested under key `state`; `Ledger::assign` and
  `supersede_orchestrators` still select on `status == "working"`, so a
  Planned record counts as working there — fix them to use the typed
  status), `a2-runtime.md` (`telemetry/readers.rs` still uses a
  `ProcessEnv` shim: remove it; pass values from `RuntimeContext`),
  `a5-routing.md`.
- Source today: `crates/horch-core/src/telemetry/collect.rs`:
  `read_ledgers(state_root)` (about line 29) parses every
  `state_root/*.json` directly; `Collector::ledgers` (about 268);
  `build snapshot` filters on `r.status == STATUS_DONE` (about 444) and
  `STATUS_WORKING` (about 488). CLI readers: `crates/horch/src/cmd/cost.rs`,
  `usagecmd.rs`, `ledgercmd.rs` (`sessions`), `telemetry.rs`.
- Mind OD3: the dataset dir `state_root/multi-herdr/` must still never be
  read as a ledger (test `mea_08_dataset_dir_not_read_as_ledger` exists).
- Risk 5 decision: `horch sessions` keeps printing the legacy status words
  (`working`/`done`), because the orchestrator prompt golden reads that
  output; add the typed status only to `horch sessions --json` as a new key
  `state` (additive). Do not change the text render.
- Parallel units: U18 `a4-harness`, U20 `a11-marketplace-cli`, U21
  `a7b-messaging` (owns `cmd/messaging.rs`, `cmd/tilecmd.rs`), B-phase units.
  A later unit (A6 service) owns `cmd/spawn.rs`, `cmd/worker.rs`,
  `execution/{plan,service,lifecycle}.rs`.

## FILES

own:
- `crates/horch-core/src/telemetry/collect.rs`, `telemetry/readers.rs`
- `crates/horch-core/src/execution/store.rs` (add a multi-ledger reader:
  `read_all_ledgers(state_root) -> Vec<(PathBuf, Vec<LedgerRecordV1>)>`,
  keeping the "last good copy" behavior that `Collector` has)
- `crates/horch-core/src/ledger.rs` (`assign`, `supersede_orchestrators` only)
- `crates/horch/src/cmd/cost.rs`, `usagecmd.rs`, `ledgercmd.rs`, `telemetry.rs`
- `crates/horch-core/tests/arch_scan.rs` (add `arc_22_no_error_string_matching`)
- `crates/horch-core/tests/telemetry_executions.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a6c-presentation.md`

do not touch: oracle and golden data (the `sessions` text oracle must stay
green unchanged; the `sessions --json` oracle gains only the additive key —
if the JSON oracle compares whole documents, send `QUESTION:` before you
change anything), U18/U20/U21 files, `cmd/spawn.rs`, `cmd/worker.rs`.

## STEPS

1. Create the worktree (conventions §2).
2. Store reader: add `read_all_ledgers` in `execution/store.rs` (skips the
   dataset subdir and non-ledger files exactly as `read_ledgers` does) and
   make `telemetry::collect::read_ledgers` and `Collector::ledgers` call it.
3. Live and done decisions in `collect.rs` use
   `ExecutionStatus::resolve(record.state.as_ref(), &record.status)` and
   `is_live()` / `is_terminal()`. A record with `state` LaunchFailed or
   Failed is never live.
4. `Ledger::assign` and `supersede_orchestrators` select on the typed status.
5. `cmd/cost.rs`, `usagecmd.rs`, `ledgercmd.rs`, `telemetry.rs` read through
   the store API (no direct `serde_json::from_str` of ledger files, no
   `status == "working"` comparisons). `sessions --json` adds `state` per
   record (the typed status) when present.
6. `telemetry/readers.rs`: remove the `ProcessEnv` shim; take the values
   (home, codex home, opencode paths, sqlite3 bin) as parameters from the
   caller's context.
7. Tests:
   - `arc_23_telemetry_reads_executions` (in
     `crates/horch-core/tests/telemetry_executions.rs`): a state root with
     records in every typed state; the collector snapshot shows live rows
     only for Starting/Running; a LaunchFailed record is not live; a ledger
     written by the old format still loads; the dataset subdir is skipped.
   - `arc_22_no_error_string_matching` (in `arch_scan.rs`): outside tests
     and outside a short `ALLOWED` list (each entry with a reason; for
     example parsing an external CLI's stderr is allowed), no code in
     `crates/horch-core/src` or `crates/horch/src` matches on error text:
     `.to_string().contains(`, `format!("{e}").contains(`, `err.to_string() ==`,
     `.contains("error` on an error value. List what you allow in the report.
   - All `tel_*`, `spc_*`, `quo_*` tests and the CLI oracles stay green.
8. Gate after each step. Commits: `A6: Read ledgers through the execution store`,
   `A6: Decide live and done from the typed status`, `A6: Reports read the store`,
   `A6: Remove the ProcessEnv shim from telemetry readers`, `A6: Add ARC-22 and ARC-23 tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- `arc_23_telemetry_reads_executions` and `arc_22_no_error_string_matching` pass.
- `grep -rn '"working"' crates/horch/src crates/horch-core/src/telemetry`
  shows no status comparison (constants and the legacy mapping excepted).
- `just gate` is green; every existing output oracle and golden is unchanged.

## REPORT

- `horch note` after each commit.
- `horch done` summary: what moved, the ALLOWED list, gotchas.
