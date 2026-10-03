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
