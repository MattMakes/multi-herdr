# U12 a5-routing: routing consolidation, eligible set, provenance

Unit slug: `a5-routing`. Branch: `ard/a5-routing`. Phase: A5.
Requirements: ARC-12, ARC-13, ARC-14.

## GOAL

All routing lives in `crates/horch-core/src/routing/`. It is pure, except for
`routing/snapshot::obtain`, which is the only path that probes quota. The
eligible set and the reason that each teammate is excluded are explicit
values. Every ledger record that a spawn writes carries `routing`
provenance. Decisions are byte-identical to the A0 routing oracle, and the
`route`/`spawn` CLI behaves exactly as before.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 (layout:
  `routing/{policy,quota,quota_probe,snapshot,balance,eligible,decision}.rs`),
  module rule "routing never launches", §2 "routing", §3 "A5", §4
  ARC-12..14 rows and Spec A §17 item 6.
- Read the merged reports in `ai_docs/reports/arch-refactor-dataset/`:
  `a0-oracles.md` (routing oracle: 12 quota fixtures × 33 teammates × flags
  {none, exact, force} × modes {auto, advise, off}; now pinned to
  `2026-09-28T18:00:00Z` and `Policy::default()`; the test is
  `oracle_routing_matches` in `crates/horch-core/tests/baseline_oracles.rs`)
  and `a1-vocabulary.md`.
- Source today:
  - `policy.rs` (160 lines): `BalanceMode`, `Policy`, `Policy::load`; reads
    `HORCH_BALANCE` at line 99.
  - `quota.rs` (1546 lines): pure parts (`Window`, `pool_for`,
    `model_family`, `PoolReading`, `QuotaFile`, `State`, `Assessment`,
    `QuotaView` and its impl, `refusal_in_force`, `parse_claude_usage`,
    `parse_codex_limits`, `newest_rollout_snapshot`) and impure parts
    (`Child` at 618, `probe_claude`, `probe_codex`, `run_short`,
    `probe_local`, `harness_version`, `ProbeBins`, `probe_timeout` reading
    `HORCH_PROBE_TIMEOUT_MS`, `probe_all`, `current_view` at 1177). It reads
    `HORCH_QUOTA_FILE` (223) and uses `std::env::temp_dir()` (854).
  - `balance_policy.rs` (689 lines): `GateFlags`, `PoolLine`, `Decision`,
    `merge`, `trains_on_input`, `summary`, `reason_line`, private
    `Candidate` and `candidates()` (165), `pool_line`, `decide` (190),
    `resolve` (284), and `fallback_problems`/`fallback_warnings`/`names_tool`
    (306 to 370).
  - Callers: `crates/horch/src/cmd/route.rs`, `quotacmd.rs`, `spawn.rs`
    (lines 120 to 200), `recipes.rs:248`, `telemetry.rs:617`,
    `teammatescmd.rs:232`, `crates/horch-core/src/telemetry/collect.rs:324`.
- The ledger `Record` (`ledger.rs:52`) has `via` and `substitution_reason`.
- Parallel units:
  - U11 `a3-roster` moves `fallback_problems`, `fallback_warnings` and
    `names_tool` from `balance_policy.rs` into `roster/validation.rs` and
    leaves a `pub use` in their place. Do NOT edit those 3 functions or
    their lines. Expect a small rebase conflict at the end of the file;
    keep U11's `pub use`.
  - U10 `a2-runtime` owns `ledger.rs`, `mailbox.rs`, `agent.rs` and most env
    reads in `cmd/*.rs`. You own the env reads in `quota.rs` and `policy.rs`.
    Make them parameters (`balance_override: Option<&str>`,
    `quota_file: Option<&Path>`, `probe_timeout: Option<Duration>`,
    `temp_root: &Path`). At the `cmd/*` call sites, use `ctx` fields if A2
    is on your base, else read env at the call site with a
    `// A2: from RuntimeContext` comment.
  - The provenance field needs one new optional field on `ledger::Record`.
    U10 also edits `ledger.rs`. Add ONLY the field (after
    `substitution_reason`) and its `None` in the constructor; touch no other
    line.
- If the A2 unit's `arc_05_no_ambient_env_in_core` test is on your base,
  delete the `quota.rs`, `policy.rs` and `routing/` entries from its
  `PENDING` list in your last commit.

## FILES

own:
- `crates/horch-core/src/routing/**` (new)
- `crates/horch-core/src/policy.rs`, `quota.rs` (become shims)
- `crates/horch-core/src/balance_policy.rs` (becomes a shim, except the 3
  roster functions)
- `crates/horch-core/src/lib.rs` (add `pub mod routing;`)
- `crates/horch-core/src/ledger.rs` (the one new field only)
- `crates/horch/src/cmd/route.rs`, `quotacmd.rs`, and the routing lines of
  `spawn.rs` (120 to 200), `recipes.rs` (about 248), `telemetry.rs` (about
  617), `crates/horch-core/src/telemetry/collect.rs` (about 324)
- `crates/horch-core/tests/routing.rs` (new)
- `crates/horch-core/tests/baseline_oracles.rs` (call sites only, if paths change)
- `crates/horch-e2e/tests/routing_provenance.rs` (new)
- `crates/horch-core/tests/arch_scan.rs` (PENDING entries only, if present)
- `ai_docs/reports/arch-refactor-dataset/a5-routing.md`

do not touch: oracle data files, `teammates.rs`, `roster/**`, the 3 roster
functions, `workspace/**`, `mailbox.rs`, golden files.

## STEPS

1. Create the worktree (conventions §2).
2. Move with `git mv`, then shim:
   - `policy.rs` → `routing/policy.rs`; `policy.rs` shim re-exports.
   - `quota.rs` → split: pure items to `routing/quota.rs`; `Child`, probes,
     `run_short`, `probe_local`, `harness_version`, `ProbeBins`,
     `probe_all` to `routing/quota_probe.rs`; `current_view` becomes
     `routing/snapshot.rs::obtain(...)` with the same logic. `quota.rs`
     shim re-exports all three (keep the name `current_view` as a
     re-exported alias of `obtain`).
   - `balance_policy.rs` → `routing/decision.rs` (`Decision`, `GateFlags`,
     `PoolLine`, `decide`, `resolve`, `merge`, `summary`, `reason_line`,
     `pool_line`), `routing/eligible.rs` (`trains_on_input`, the candidate
     filter), `routing/balance.rs` (the gate glue if any remains). Keep the
     3 roster functions in `balance_policy.rs` as they are (U11 replaces
     them). Shim the rest.
   Check after each move: the routing oracle passes.
3. `routing/eligible.rs`:
   - `pub enum ExclusionReason { NotInRoster, Hidden, ReservedTier, TrainsOnInput, PoolBlocked, Unspawnable, EffortUnsupported, HarnessUnavailable, AgentNone, ExcludedByConfig, OverBudget }`
     (serde snake_case).
   - `pub struct EligibleEntry { teammate, harness: HarnessKind, model, effort: Option<String>, fallback_index: Option<usize>, pool: String, pool_state: State, verdict: Result<(), ExclusionReason> }`
     (serde: `verdict` as `"eligible"` or the reason string).
   - `pub fn eligible_fallbacks(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<EligibleEntry>`:
     the requested teammate and its `fallbacks` in order, each with a
     verdict. `candidates()` in the decision code becomes
     `eligible_fallbacks(..).into_iter().filter(eligible)`, with the same
     order and the same drop rules. Prove equivalence in a test.
   - `pub fn roster_eligibility(roster: &Roster, view: &QuotaView, cfg: &EligibilityConfig) -> Vec<EligibleEntry>`:
     every teammate in the roster with a verdict (B3's planner uses this).
     `EligibilityConfig` holds an exclude list and an optional budget check
     closure or value; keep it minimal.
   - Both functions are pure: no I/O, no clock read, no env.
4. `routing/decision.rs`: add `pub enum RoutingDecision { Spawn { teammate }, Substitute { requested, resolved, reason }, Refuse { requested, reason, pools } }`
   with `From<&Decision>`; keep `Decision` and its JSON exactly (the `route
   --json` output and the oracle are byte-identical).
   `pub struct RoutingProvenance { requested: String, resolved: String, fallback_index: Option<usize>, pool: Option<String>, pool_state: Option<String>, reason: Option<String>, mode: String }`
   with a constructor from the decision and the mode.
5. Provenance on records: add
   `#[serde(default, skip_serializing_if = "Option::is_none")] pub routing: Option<RoutingProvenance>`
   to `ledger::Record` (store it as `serde_json::Value` if a direct type
   import creates a module cycle; prefer the typed field). `spawn.rs` sets it
   for spawn, substitute and resume (resume copies the provenance of the
   record it resumes, with `mode` set to `"resume"`). `via` and
   `substitution_reason` are still written exactly as today.
6. Purity scan: `arc_13_routing_never_launches` in `tests/routing.rs`: no
   file under `routing/` except `quota_probe.rs` and `snapshot.rs` contains
   `std::process`, `Command::new`, `std::fs`, `herdr`, `launch::` or
   `std::env::var`.
7. Tests:
   - `arc_12_decisions_match_baseline`: run the same matrix as the A0 oracle
     through the new `routing::decision` path and compare with the oracle
     files (reuse the oracle test's helpers; do not edit oracle data).
   - `arc_13_exclusion_reasons`: one test case per `ExclusionReason`
     variant, each with a minimal roster and view that produces exactly that
     reason. If a variant cannot be produced by today's rules
     (for example `OverBudget` or `ExcludedByConfig` without config), produce
     it through `EligibilityConfig`.
   - `arc_13_candidates_equivalent`: for every teammate × quota fixture, the
     filtered `eligible_fallbacks` equals the old `candidates()` order (keep a
     private copy of the old function under `#[cfg(test)]` for the
     comparison).
   - `arc_14_provenance_on_spawn_substitute_resume` in
     `crates/horch-e2e/tests/routing_provenance.rs`: with the e2e harness and
     a quota fixture that forces a substitution, run `horch spawn`
     (normal), `horch spawn` (substituted) and `horch spawn --resume`, then
     read the ledger and assert each record has `routing` with the right
     `requested`, `resolved`, `mode`. Read the existing bal_04/bal_05 e2e
     tests for the pattern.
   - Existing BAL-01..09, QUO-* tests and `oracle_routing_matches` stay green
     and unchanged.
8. Gate after each step. Commits: `A5: Move routing modules into routing/`,
   `A5: Add eligible set and exclusion reasons`, `A5: Persist routing provenance`,
   `A5: Pass quota and policy env as parameters`, `A5: Add ARC-12..14 tests`.
9. Write and commit the report. Follow conventions §6 to finish.

## DONE WHEN

- The 4 arc tests and `arc_13_routing_never_launches` pass.
- `oracle_routing_matches` passes with unchanged data.
- `horch route --json` output on a fixture is byte-identical to your base.
- The full gate is green apart from named base failures.

## REPORT

- `horch note` after each commit.
- `horch done` summary: module map, the eligible API (for B3), the
  provenance shape, gotchas.
