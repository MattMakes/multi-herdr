# U19 a6a-store: execution store, legacy ledger DTO, typed state, DirLock

Unit slug: `a6a-store`. Branch: `ard/a6a-store`. Phase: A6 (commit 1, store).
Requirements: ARC-17, SKL-04.

## GOAL

Executions are read and written through `execution::store`, which keeps the
exact on-disk ledger format (`LedgerRecordV1`) and adds the new optional
fields, including the typed `state`. Old binaries still read new ledgers
correctly, a failed or launch-failed record never looks live, and old
ledgers load and resume. Writes are durable and locked with `fsx::DirLock`.
`ledger.rs` becomes a facade over the store.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §2 (`ExecutionStatus` legacy compatibility,
  `Execution`, `LedgerRecordV1`), §3 "A6" commit 1, §4 ARC-17 and SKL-04 rows,
  §6 risk 5.
- Design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`:
  `Execution`, `LedgerRecordV1`, the status/state rule, the A6 section. The
  design wins over this plan where they differ.
- Merged reports to read in `ai_docs/reports/arch-refactor-dataset/`:
  `a1-vocabulary.md` (`ExecutionStatus` serde is `#[serde(tag = "state")]`,
  `legacy_status()`, `from_legacy`, `resolve`), `b1-primitives.md`
  (`fsx::{write_atomic, replace_durable, DirLock}`; note: fsx has a private
  copy of `pid_alive` that must merge with `telemetry::lock::pid_alive`),
  `a2-runtime.md` (`Ledger::open_in(ctx)`), `a5-routing.md`
  (`Record.routing`, `Ledger::set_routing`), `a9a-skills.md`
  (`ResolvedSkillRef`, `SkillActivationPlan`), `a0-oracles.md` (ledger
  oracles: `crates/horch-core/tests/oracles/ledgers/{bash-era,pre-effort,pr14-substituted,orchestrator}.json`
  with `.loaded.json` and `.saved.json`; the saved oracle uses
  `to_string_pretty` with no trailing newline because `Ledger::write` was
  private; CLI `sessions` oracles in `crates/horch/tests/oracles/sessions/`).
- Today: `crates/horch-core/src/ledger.rs` (1087 lines): `HistoryEntry`,
  `Record` (52), `Ledger` (142) with a private mkdir `LockGuard` (150),
  `slug`, `read`, `add`, `add_with_phase`, `insert`,
  `supersede_orchestrators`, `set_session`, `set_effort`, `set_routing`,
  `has_session`, `resume`, `resume_with_phase`, `assign`, `note`, `done`,
  `get`, `render`.
- A parallel unit U18 `a4-harness` edits `launch.rs`, `codex.rs`,
  `opencode.rs`, `prime.rs`, `plugins.rs`, `harness/**`, `cmd/worker.rs`
  `launch_agent` and `cmd/recipes.rs` `pane_launch`. Do not touch them.
  A later A6 unit (plan, service, lifecycle) builds on your store.

## FILES

own:
- `crates/horch-core/src/execution/{legacy,store}.rs` (new)
- `crates/horch-core/src/execution/mod.rs` (your `pub mod` lines)
- `crates/horch-core/src/ledger.rs` (becomes a facade over the store)
- `crates/horch-core/src/fsx.rs` (only to share `pid_alive`)
- `crates/horch-core/src/telemetry/lock.rs` (only to use the shared `pid_alive`; do not change its lock protocol)
- `crates/horch-core/tests/execution_store.rs` (new)
- `crates/horch-core/tests/baseline_oracles.rs` (call sites only; never oracle data)
- `ai_docs/reports/arch-refactor-dataset/a6a-store.md`

do not touch: oracle and golden data, `cmd/*` except where a `ledger::`
signature you change forces a one-line call-site update, U18's files.

## STEPS

1. Create the worktree (conventions §2).
2. `execution/legacy.rs`: `LedgerRecordV1` = today's `Record` serde exactly
   (same field names, order, defaults, `skip_serializing_if`, `tier` key),
   plus new optional skip-if-empty fields: `pane_id`, `task_id`, `workdir`,
   `experiment_id`, `round_id`, `label`, `state` (an `ExecutionStatus`,
   serialized under the key `state_detail` if the key `state` would clash
   with the `#[serde(tag = "state")]` shape; choose one shape, document it,
   and test it), `exit_code`, `finished_at`, `skills`
   (`Vec<ResolvedSkillRef>`). `routing` already exists. Keep
   `pub type Record = LedgerRecordV1;` so every caller compiles.
   Compatibility rule: on write, `status` is `state.legacy_status()` when
   `state` is set; on read, the typed status is
   `ExecutionStatus::resolve(state, &status)`.
3. `execution/store.rs`: `ExecutionStore` over `<state_root>/<slug>.json`
   (same path as today) with the operations the `Ledger` has today
   (read, insert, update by key with a closure, get), plus
   `set_state(key, ExecutionStatus)`, `set_skills(key, Vec<ResolvedSkillRef>)`,
   and `live()` = records whose typed status `is_live()`.
   Locking: replace the private mkdir `LockGuard` with `fsx::DirLock`
   (same lock dir name as today, so an old binary and a new binary exclude
   each other; check the old lock path and keep it). Writes use
   `fsx::write_atomic` (temp + fsync + rename + dir fsync).
4. `ledger.rs`: keep every public function and signature; implement them on
   top of `ExecutionStore`. `ledger.rs` holds no file I/O of its own after
   this step. Merge `pid_alive` into one shared function (in `fsx.rs` or a
   small `runtime::process` helper) used by both `fsx::DirLock` and
   `telemetry::lock`.
5. SKL-04: add `ExecutionStore::set_skills`, and a test that a record with
   skills round-trips. Wiring the call into the spawn flow is the later A6
   unit's job; note it in the report.
6. Tests in `crates/horch-core/tests/execution_store.rs`:
   - `arc_17_legacy_ledgers_load_and_resume`: each of the 4 A0 legacy
     fixtures loads through the store, and a resume of one worker record
     succeeds and writes a ledger that still loads.
   - `arc_17_roundtrip_byte_identical`: load then save each legacy fixture
     without changes gives the same bytes as the A0 `.saved.json` oracle
     (account for the oracle's `to_string_pretty`/no trailing newline: if the
     real writer differs only in that, compare after normalizing the final
     newline, and say so in the report; any other difference fails).
   - `arc_17_old_reader_sees_compat_status`: write records with every
     `ExecutionStatus`; deserialize them with a copy of the OLD `Record`
     struct (copy it into the test as `OldRecord`); `status` is `working`
     only for Planned/Starting/Running and `done` for every terminal state.
   - `skl_04_execution_records_skill_refs`.
   - `store_dirlock_excludes_concurrent_writers`: 2 threads each append 50
     records through separate store handles; all 100 are present.
   - `oracle_ledgers_match` and `oracle_cli_sessions_match` stay green.
7. Gate after each step. Commits: `A6: Add LedgerRecordV1 with typed state`,
   `A6: Add execution store with DirLock and durable writes`,
   `A6: Make ledger a facade over the store`, `A6: Add ARC-17 and SKL-04 tests`.
8. Write and commit the report (the `state` serde shape, the lock path, the
   facade map, gotchas for the A6 service unit). Follow conventions §6.

## DONE WHEN

- The 5 named tests pass; ledger and sessions oracles pass unchanged.
- `ledger.rs` contains no `std::fs` call (grep).
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, state shape, lock path, gotchas.
