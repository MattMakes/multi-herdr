# a12-cleanup report (A12)

Branch `ard/a12-cleanup`. Requirement ARC-25. Behavior is unchanged.

## Shim list

Written before any change (plan step 2). Each line names the shim, the
module that owns the item, and the group commit that removes it.

### Whole-file re-export shims (`crates/horch-core/src`)

| Shim | Owner | Group |
|---|---|---|
| `balance.rs` | `workspace::balance` | workspace |
| `herdr.rs` | `workspace::herdr` | workspace |
| `layout.rs` | `workspace::layout` | workspace |
| `paneshell.rs` | `workspace::paneshell` | workspace |
| `tile.rs` | `workspace::tile` | workspace |
| `codex.rs` | `harness::codex` | harness |
| `launch.rs` | `harness::launch` | harness |
| `opencode.rs` | `harness::opencode` | harness |
| `plugins.rs` | `harness::claude_plugins` | harness |
| `prime.rs` | `harness::prime` | harness |
| `mailbox.rs` | `messaging::mailbox` | messaging |
| `message.rs` | `messaging::message` | messaging |
| `policy.rs` | `routing::policy` | routing |
| `quota.rs` (+ `current_view`, `QuotaEnv`) | `routing::quota`, `routing::quota_probe`, `routing::snapshot` | routing |
| `balance_policy.rs` (re-exports + tests) | `routing::decision`, `routing::eligible`, `roster::validation` | routing |
| `teammates.rs` (+ `Roster::load`, `Roster::load_with`) | `roster` | roster |
| `ledger.rs` facade (+ `state_root`, `project_dir`, `Ledger::open`, re-exports of `Record`, `HistoryEntry`, `KIND_*`, `slug`) | `execution` | ledger |

### Wrappers kept "until A12"

| Shim | Replacement | Group |
|---|---|---|
| `agent.rs`: `*_bin`, `which`, `on_path`, `home_dir` (`ProcessEnv`), re-exports of `runtime::process` | `runtime::bins`, `runtime::process`, `runtime::paths` | runtime |
| `Herdr::send_line` | `messaging::delivery::send_line` | workspace |
| `tilecmd::after_change` re-export | `workspace::arrange::after_change` | workspace |
| `balancecmd::equalize_quietly` re-export | `workspace::arrange::equalize_quietly` | workspace |
| `workspace::herdr` re-export of `workspace::model::*` | `workspace::model` | workspace |
| `LaunchEnv::from_process`, `launch::command`, `launch::command_with_skills` | `LaunchEnv::from_context`, `command_in`, `command_with_skills_in` | harness |
| `Bundle::configure`, `Bundle::apply_env` | the adapter hooks they call | harness |
| `Bundle::briefing` (`ProcessEnv` for `HOME`) | `Bundle::briefing_in` | harness |
| `claude_plugins::resolve_all` (`ProcessEnv`) | `resolve_all_in` | harness |
| `prime::Daemon::finish` through `agent::prime_bin` | a bin path from `HarnessBins` | harness |
| `HarnessKind::mints_session_id`, `harvests_session_id`, `runs_a_daemon`, `uses_execpolicy` | `Capabilities` fields | harness |
| `skills::ensure_supported` | `skills::ensure_supported_in` | harness |
| `roster::Agent` alias of `HarnessKind` | `harness::HarnessKind` | roster |
| `roster` re-export of `validation::{fallback_problems, fallback_warnings}` through `balance_policy` | `roster::validation` | routing |
| `arch_scan.rs` `PENDING = ["teammates.rs"]` | empty | roster |

### Cross-module re-exports (an item re-exported from a module that does not own it)

| Re-export | Owner |
|---|---|
| `execution::model` re-exports `HistoryEntry`, `ReportTarget` | `execution::legacy`, `execution::lifecycle` |
| `measure` re-exports `NumstatLine` | `vcs::git` |
| `telemetry::lock` re-exports `pid_alive` | `fsx` |
| `competition::preflight` re-exports `HARNESS_FOOTPRINT_BYTES`, `NONE_FOOTPRINT_BYTES` | `harness::capabilities` |
| `messaging::mailbox` re-exports `Brief` | `messaging::brief` |
| `skills::catalog` re-exports `horch_marketplace` as `marketplace`, `SkillVersion` | `horch_marketplace` |

### Kept

- A `mod.rs` (or `skills.rs`, a crate `lib.rs`) that re-exports its own child
  modules' items is the module's API, not a shim. Examples: `roster::Roster`,
  `runtime::Paths`, `harness::Capabilities`, `execution::Execution`, the
  `horch_marketplace` crate root.
- `LedgerRecordV1` (`execution::legacy`), the Brief v1 defaults, the `tier`
  key and the legacy `status` words: old files on disk still use them.

## Commits

| Commit | Group |
|---|---|
| `A12: Remove the workspace shims` | workspace |
| `A12: Remove the messaging and routing shims` | messaging, routing |
| `A12: Remove the harness shims and process-env wrappers` | harness |
| `A12: Remove agent.rs, teammates.rs and the Agent alias` | runtime, roster |
| `A12: Remove the ledger.rs facade` | ledger |
| `A12: Remove the cross-module re-exports` | cross-module |
| `A12: Add arc_25_no_shim_modules` | test |
| `A12: Make pub(crate) the default visibility` | visibility |
| `A12: Update the module table, README architecture and design statuses` | docs |
| `A12: Drop the A12 wording from the arch scan comments` | docs |

Every commit built and passed `cargo test --workspace`.

## Where things moved

- `ledger::Ledger` is `execution::records::Ledger` (`git mv`, same code).
  `STATUS_WORKING` and `STATUS_DONE` are in `execution::legacy` beside
  `KIND_*`. `Record`, `HistoryEntry` and `KIND_*` come from
  `execution::legacy`; `slug` from `execution::store`.
- `NumstatLine` is defined in `measure` (a domain value). `vcs::git`
  imports it. Its serde is unchanged; the WorkerRun golden still matches.
- The BAL table tests of `balance_policy.rs` are the `bal_tables` test
  module at the end of `routing/decision.rs`, unchanged. A new
  `routing/tests.rs` would break `arc_13_routing_never_launches`, which
  counts the files in `routing/`.
- `prime::Daemon::install(state_root, role, bin)` takes the `prime-agent`
  path. `Prime::prepare` passes `ctx.bins.harness.prime`.
- `Roster::check` resolves plugin skills under `Roster::home` (the home
  the roster was loaded under), not the process `$HOME`. `horch teammates
  --check` loads with the real home, so it behaves as before. A built-in
  roster (no home) finds no installed plugins; no repo teammate uses
  `plugin_skills`.
- `command_with_skills_in` calls the adapter's `expose_skills` and
  `expose_skills_env` directly.

## Tests

- Added: `arc_25_no_shim_modules`, `arc_25_scan_tells_shims_from_module_apis`
  (`crates/horch-core/tests/arch_scan.rs`). I checked that the scan fails
  on a reintroduced glob shim file, a cross-module `pub use`, and a
  `from_process` wrapper.
- Changed call sites only (plan step 3), never an expected value:
  - Oracle tests (`baseline_oracles.rs`) build the `LaunchEnv` with
    `LaunchEnv::from_context(&RuntimeContext::from_env(&ProcessEnv))` under
    the pinned `World` environment, and load the roster with
    `Roster::load_layered(Some(home), None, Some(repo))`. Every oracle
    matches.
  - Unit tests that used `LaunchEnv::from_process()` or `launch::command`
    use the new `#[cfg(test)] LaunchEnv::for_test()` (bare program names,
    no home). They no longer read the operator's real `$HOME` or `PATH`.
  - `skills_catalog.rs`, `judge_input.rs` and `nfr.rs` load the repo roster
    with `Roster::load_layered(None, None, Some(dir))`: hermetic, no
    `~/.config/horch/teammates` overlay.
  - `harness::tests::arc_10_capabilities_match_legacy_predicates` no longer
    asserts the 4 deleted predicates; it still asserts the same values on
    `Capabilities`. The alias half of the HarnessKind spelling test is gone
    with the alias.
- `arc_05`: `PENDING` is empty and the scan passes with no exception.

## Visibility

- `#![warn(unreachable_pub)]` is on in `horch-core` and `horch` (lib).
  `cargo rustc -p <crate> --lib -- -D unreachable_pub` is clean for both,
  and the workspace builds with `RUSTFLAGS="-D warnings"`.
- 358 lines changed from `pub` to `pub(crate)`. Method: an item is
  `pub(crate)` when no file outside the crate names it (the binaries,
  `horch-e2e`, integration tests). The compiler then decided the rest:
  - A type that a public signature or field exposes stays `pub`
    (`private_interfaces`), for example `Paths`, `Capabilities`, `IdError`,
    `Window`.
  - Modules that become `pub(crate)` (31): `competition::diversity`,
    `execution::model`, `harness::{claude, claude_plugins, none, pi}`,
    `roster::{effort, operator, parser, permission, phase, repository,
    teammate}`, `runtime::{context, paths}`, `skills::{activation,
    materialize, selection}`, `teacher::system_one`, `telemetry::cursor`,
    and in `horch`: `dataset::{cleanup, export, judge_job, preflight,
    promote, readiness, rebuild, rollback, run, status, watch}`.
  - `mod.rs` re-exports that only the crate uses are `pub(crate) use`.
- `build.rs` emits `pub(crate) static` for the roster tables.
- Doc links to items that became `pub(crate)` are code spans now, so
  `cargo doc` has no warning for either crate.

### Kept `pub` although nothing uses them

These items have no caller anywhere (not even in the crate), so
`pub(crate)` turns them into `dead_code` warnings. I kept them `pub` and did
not delete them: deletion is outside a visibility change, and some are
designed seams. A later unit can delete or wire them.

- `competition::judging::DEFAULT_JUDGE_TIMEOUT`
- `competition::model::{JudgmentRef, ValidationRun}`
- `execution::records::Ledger::set_routing`
- `execution::store::ExecutionStore::find_by_idempotency`
- `fsx::replace_durable`
- `harness::HarnessKind::{takes_tool_lists, takes_tool_denylist}` (tests use them)
- `measure::testkit` `next_f64`
- `messaging::mailbox::Mailbox::write_brief`
- `teacher::{DecisionModel, inert::Inert, system_one::{DecisionRequest, DecisionResponse}}`
- `workspace::client::HerdrClient`
- `workspace::herdr::Herdr::{focused_pane, tab_create, tab_close}`
- `workspace::layout::analyze`, `workspace::model::NewTab`,
  `workspace::testing` `workspace_ids`
- `horch::dataset::NOT_IMPLEMENTED`
- Modules kept `pub` for the same reason: `workspace::{layout, tile}`,
  `teacher::inert`. `skills::briefing` stays `pub`: `skills_catalog.rs` uses it.

## Gate

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`:
799 passed, 0 failed, 1 ignored (`nfr_02`, the perf run of
`just verify-perf`). The e2e suites (`crates/horch-e2e/tests`, all fault
and crash-point tests) run inside `cargo test --workspace`.
`./scripts/check-req-coverage.sh --phase A12` exits 0.
`grep -rn 'A12' crates/` finds nothing.

## Outside my scope (noticed, not fixed)

- `crates/horch-e2e/tests/skills_exposure.rs:265` has a comment that
  says `horch spawn` checks against the compiled-in catalog "until U26
  switches it". U26 did switch it (`execution/plan.rs` calls
  `ensure_supported_in`), so the workaround in `skl_06_e2e_marketplace_offline`
  (editing the brief) can go. I left the test as it is.
- `RuntimeContext::from_env` is the only way the oracle tests get a
  `LaunchEnv`; a small `LaunchEnv::from_env(&dyn EnvSource)` would make
  that clearer, but nothing else needs it.
