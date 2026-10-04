# U12 a5-routing: report

Phase A5. Requirements ARC-12, ARC-13, ARC-14. Branch `ard/a5-routing`.

## Module map

| File | Holds |
|---|---|
| `routing/mod.rs` | module list, purity note |
| `routing/policy.rs` | `BalanceMode`, `Policy`, `parse`, `path` (no I/O) |
| `routing/quota.rs` | `Window`, pools, `PoolReading`, `QuotaFile` (struct, `apply_signals`), `State`, `Assessment`, `QuotaView`, `refusal_in_force`, `Probed`, `parse_claude_usage`, `parse_codex_limits`, `last_probe`, `probe_due` |
| `routing/quota_probe.rs` | `Child`, `probe_claude`, `probe_codex`, `run_short`, `probe_local`, `harness_version`, `newest_rollout_snapshot`, `ProbeBins`, `PROBE_TIMEOUT`, `probe_all` |
| `routing/snapshot.rs` | `QuotaEnv`, `Policy::load`, `QuotaFile::{read, load, write}`, `obtain` (the only probing path) |
| `routing/decision.rs` | `GateFlags`, `PoolLine`, `Decision` (JSON unchanged), `merge`, `summary`, `decide`, `resolve`, `RoutingMode`, `RoutingProvenance`, `RoutingDecision` |
| `routing/eligible.rs` | `ExclusionReason`, `Verdict`, `EligibleEntry`, `EligibilityFilter`, `trains_on_input`, `eligible_fallbacks`, `roster_eligibility` |
| `routing/balance.rs` | `touched_pools` (the pool list `horch route` prints) |

Shims: `policy.rs`, `quota.rs` (plus `current_view` = alias of `snapshot::obtain`,
and `QuotaEnv`), `balance_policy.rs` (`decision::*`, `trains_on_input`, and
A3's `roster::validation` re-exports).

## Eligible API (for B3)

- `eligible_fallbacks(req, roster, view)`: the request (`fallback_index: None`),
  then each `fallbacks:` entry (`Some(i)`). Fallback rules in order:
  `NotInRoster`, `Hidden`, `ReservedTier`, `TrainsOnInput`. The request gets
  only `ReservedTier`. Pool states are reported, not judged: the gate decides
  on them. The gate's `candidates()` = entries with `fallback_index` and
  `Verdict::Eligible`.
- `roster_eligibility(roster, view, filter)`: every teammate, sorted by name.
  Rules in order: `ExcludedByConfig`, `Hidden`, `AgentNone`, `ReservedTier`,
  `Unspawnable` (no model), `EffortUnsupported`, `TrainsOnInput`,
  `HarnessUnavailable`, `PoolBlocked` (`State::blocks()`), `OverBudget`.
- `EligibilityFilter { excluded, max_cost_microusd, estimated_cost_microusd, available_harnesses: Option<Vec<HarnessKind>> }`.
  B3 supplies the cost estimates. No estimate is never over budget.
- Wire: `{"teammate", "harness", "model", "effort", "fallback_index", "pool", "pool_state", "verdict": "eligible"}`
  or `..., "verdict": "excluded", "reason": "hidden"`.

## Provenance shape

`ledger::Record.routing: Option<RoutingProvenance>`, skipped when `None`:

```json
"routing": {"requested": "researcher", "resolved": "codex-sol", "fallback_index": 0,
            "pool": "codex", "pool_state": "ok",
            "reason": "claude 7d 100%, resets 2026-10-02T14:00Z", "mode": "auto"}
```

- `mode`: `auto`, `advise`, `off`, `exact`, `force` (from `RoutingMode::for_gate`),
  `pinned` (B3), `resume`, `ungated` (the `none` agent skips the gate).
- Spawn and substitute: `RoutingProvenance::from_decision`. The pool is the
  resolved launch's pool.
- Resume: the record's provenance with `mode: resume`, written by the new
  `Ledger::set_routing` (approved by the orchestrator). A pre-A5 record gets
  `RoutingProvenance::legacy` from `tier`, `via`, `agent`, `model`;
  its `pool_state` is `unknown`.
- `via` and `substitution_reason` are written as before.

## Decisions

- Types follow design doc §4.4 where the unit plan differs (orchestrator
  confirmed): `TeammateName`, `fallback_index: Option<u32>`, `Verdict` enum,
  typed `pool_state`, `RoutingMode`, `EligibilityFilter`.
- Differences from §4.4: `EligibleEntry.model` is `Option<ModelId>` (some
  teammates have no model). `available_harnesses` is a `Vec`, because
  `HarnessKind` has no `Ord`. `RoutingMode` adds `Resume` and `Ungated`.
- `Decision` stays the gate's return type, as the unit plan says.
  `RoutingDecision` is `From<&Decision>`, with `String` fields so the
  conversion cannot fail. Design §4.4 wants `decide` to return
  `RoutingDecision` plus a `DecisionDto`; a later phase can do that switch.
- `probe_due` no longer checks the quota override. Both callers
  (`obtain`, `Collector::update_quota`) return early on an override first.
- The env parameters: `Policy::load(root, balance_override)`,
  `QuotaFile::load(root, quota_file)`, `probe_all(.., timeout, temp_root)`,
  `probe_claude(.., temp_root)`, and `QuotaEnv` for `obtain`.

## Gotchas

- The env reads moved out of routing, but 2 places still read them:
  - `crates/horch/src/cmd/quotacmd.rs`: `balance_override()`, `load_policy()`,
    `quota_env()`. `route`, `spawn`, `recipes` and `telemetry` call these.
  - `crates/horch-core/src/telemetry/collect.rs`: `Collector::open_at` reads
    `HORCH_BALANCE`, and `quota_env_from_process()` reads the rest. This is
    core code. The reads moved here from `quota.rs`/`policy.rs`; they are not
    new reads, but the A2 scan will see them. Both carry
    `// A2: from RuntimeContext`. A2 must thread them through `Collector::open*`.
- `ProbeBins::from_env` (in `quota_probe.rs`) still reads env through
  `agent::*_bin`. That is U10's area.
- `arc_13_routing_never_launches` scans only the code before `#[cfg(test)]`,
  because the QUO unit tests read fixtures with `std::fs`.
- `tests/routing.rs` builds the roster as `Roster::builtin()` + `overlay(teammates/)`,
  so it does not read `HOME`.
- No `arc_05_no_ambient_env_in_core` test on the base, so no `PENDING` edit.

## Verification

- `horch route` (text and `--json`), base binary vs branch binary: 1080 runs
  (12 fixtures x 5 teammates x 3 flags x 3 modes x 2 formats), 0 differences.
- `oracle_routing_matches` passes. Oracle data unchanged.
- Tests added: 6 (`arc_12_decisions_match_baseline`, `arc_13_routing_never_launches`,
  `arc_13_exclusion_reasons`, `arc_13_candidates_equivalent`,
  `arc_14_legacy_record_resumes_with_provenance`,
  `arc_14_provenance_on_spawn_substitute_resume`).
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: GATE GREEN.
