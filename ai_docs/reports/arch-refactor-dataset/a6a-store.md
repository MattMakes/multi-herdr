# U19 a6a-store: report

Phase A6, commit 1 (store). Requirements ARC-17, SKL-04. Branch `ard/a6a-store`.

## Module map

| File | Holds |
|---|---|
| `execution/legacy.rs` | `LedgerRecordV1`, `pub type Record = LedgerRecordV1`, `HistoryEntry`, `KIND_WORKER`, `KIND_ORCHESTRATOR`; methods `is_orchestrator`, `matches` (now `pub`), `execution_status`, `set_state`, `set_legacy_status`, `sync_legacy_status` |
| `execution/store.rs` | `slug`, `ExecutionStore::{for_project, path, read, render_json, update, update_key, insert, get, set_state, set_skills, live}` |
| `ledger.rs` | facade: `Ledger` holds an `ExecutionStore`; lifecycle rules only (insert defaults, supersede, resume, assign, done, render). No `std::fs`. Added `Ledger::store()` |
| `fsx.rs` | `pid_alive` is `pub`; the one copy |
| `telemetry/lock.rs` | `pub use crate::fsx::pid_alive;` (lock protocol unchanged) |

## Facade map

| `Ledger` | store call |
|---|---|
| `read`, `get`, `path` | `ExecutionStore::{read, get, path}` |
| `insert`, `add`, `add_with_phase` | lifecycle defaults, then `ExecutionStore::insert` |
| `set_session`, `set_effort`, `set_routing`, `resume*`, `note`, `done` | `ExecutionStore::update_key` (fails with `no record matches '<key>' in <path>`, as before) |
| `supersede_orchestrators`, `assign` | `ExecutionStore::update` |
| `has_session`, `render` | over `read` |

## The `state` shape

The typed status is under the key `state`, as a nested `ExecutionStatus`
object with its own internal `state` tag. The keys do not clash because the
tag is one level down. No `state_detail` key exists.

```json
"status": "done",
"state": {"state": "launch_failed", "stage": "split", "reason": "no such pane"},
"finished_at": "2026-10-02T10:00:00Z"
```

- On every write, the store sets `status = state.legacy_status()` for a
  record with `state` (`sync_legacy_status`).
- On read, `execution_status()` = `ExecutionStatus::resolve(state, &status)`.
- `ExecutionStore::set_state` stamps `finished_at` once on a terminal state
  and clears it on a non-terminal one.
- The legacy setters (`Ledger::done`, `resume*`, `insert`,
  `supersede_orchestrators`) call `set_legacy_status`. It moves a set `state`
  only when it disagrees: `working` reopens a terminal state as `Running`;
  `done` closes a non-terminal state as `Done`. A record without `state`
  gets none. So `horch done` on a `Failed` record keeps `Failed`.

## Field order

The new keys come after `routing`. The design doc lists `routing` last; A5
already wrote `routing` after `substitution_reason`, so it stays there to keep
the bytes of A5-era ledgers. New keys: `pane_id`, `task_id`, `workdir`,
`experiment_id`, `round_id`, `label`, `state`, `exit_code`, `finished_at`,
`skills`. All are skipped when empty.

## Lock path

`<state_root>/<slug>.json.lock/`, the same directory as the old mkdir
spinlock. `DirLock::acquire(<state_root>, "<slug>.json", 15 s stale, 60 s timeout)`.

- New binary vs old holder: the old lock has no `owner` file, so `DirLock`
  breaks it when it is older than `OWNERLESS_GRACE` (5 s). An old write is
  far shorter.
- Old binary vs new holder: the old binary waits about 15 s, then
  `remove_dir_all`. This is the old behavior.
- The old stale-lock message `horch ledger: breaking stale lock` is now
  `horch: broke stale lock <path>` from `fsx`.

## Writes

`fsx::write_atomic` with mode `0o644` (what `std::fs::write` gave the file
under the usual umask). The bytes are `serde_json::to_string_pretty`, no
trailing newline. `arc_17_roundtrip_byte_identical` compares the exact bytes
with the A0 `.saved.json` oracles. No normalization was necessary.
`oracle_ledgers_match` now calls `ExecutionStore::render_json`, the real
writer, in place of its own `to_string_pretty`. The oracle data is unchanged.

## Tests

`crates/horch-core/tests/execution_store.rs`:

- `arc_17_legacy_ledgers_load_and_resume` (3 of 4 fixtures have a worker
  record; `orchestrator.json` is only loaded)
- `arc_17_roundtrip_byte_identical`
- `arc_17_old_reader_sees_compat_status` (13 states: 4 plain, 6 failures,
  3 launch stages; also checks `live()` and the JSON shape)
- `skl_04_execution_records_skill_refs`
- `store_dirlock_excludes_concurrent_writers`
- 4 ledger unit tests moved unchanged from `ledger.rs`, because they touch
  the file directly: `effort_is_recorded_rendered_and_optional_on_disk`,
  `a_stale_lock_is_broken_rather_than_deadlocking`,
  `tel_08_insert_fills_plan_project_and_workspace`,
  `reads_a_ledger_written_by_the_bash_version`.

## For the later A6 units

- SKL-04 wiring is not done. `ExecutionStore::set_skills(key, refs)` exists;
  the spawn flow must call it with `SkillActivationPlan::activated` after
  the record is inserted.
- The store API is record-based (`LedgerRecordV1`). The design's
  `Execution`, `to_execution`/`from_execution`, `load`, `update(&ExecutionId)`
  and `find_by_idempotency` are not built. `Execution` does not exist yet.
- `ExecutionStore::open(paths, project)` from the design is not built; use
  `for_project(&ctx.paths.state_root, project)` or `Ledger::open_in(ctx)`.
- `Ledger::assign` and `supersede_orchestrators` still select on the legacy
  `status == "working"`, so a `Planned` record counts as working there.
  `ExecutionStore::live()` uses the typed status (Starting and Running only).
- `ledger::slug` is now a re-export of `execution::store::slug`.

## Checklist

- [x] `just gate` is green on this commit (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`).
- [ ] Every ID of this phase has a test (`scripts/check-req-coverage.sh --phase <P>` exits 0). (n/a: ARC-15, ARC-16, ARC-18, ARC-22, ARC-23, ARC-26 belong to later A6 units; ARC-17 is covered)
- [x] Every new test name starts with its lowercase requirement ID (`ARC-02` -> `arc_02_...`).
- [x] No golden prompt changed; any prose change is a named sanctioned block.
- [x] No serialization golden re-blessed; a format change bumped its schema version.
- [x] No oracle under `crates/*/tests/oracles/` regenerated.
- [x] No existing test changed to make it pass, unless the unit plan says so.
- [x] No new crate outside the allowlist (`scripts/check-deps.sh` and `nfr_05`, `nfr_06`, `nfr_09`, `nfr_11` pass).
- [x] No new `std::env` read outside `runtime/` and the binary's bootstrap.
- [x] No `ANTHROPIC_API_KEY` reaches a child process (`FORBIDDEN_ENV` strips it).
- [x] Tests are hermetic: no network, no real harness binary, no herdr server, no file outside a temp dir; real `git` only on temp repos.
- [x] Every moved module left a re-export shim (until A12).
- [x] Old ledgers, old briefs and old teammate frontmatter still load.
- [x] New `pub mod` lines in `crates/horch-core/src/lib.rs` are in alphabetical order; no other line changed. (n/a: lib.rs unchanged; `execution/mod.rs` got `legacy` and `store`)
- [x] The diff was re-read adversarially; the unit report lists gotchas.
