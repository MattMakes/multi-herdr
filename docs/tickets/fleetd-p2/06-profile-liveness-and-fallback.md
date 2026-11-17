# 06: The profile of a persona: the liveness and retry numbers, the mode, and the required fallback

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib roster && cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/roster/teammate.rs`, `crates/horch-core/src/roster/validation.rs`, `crates/horch-core/src/roster/tests.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs`, `teammates/`

Spec: `docs/specs/fleetd/design.md`, "Worker profiles and adapters" (the YAML block with `fallback`, `mode`, `liveness` and `retry`; "The clock of each timer"; "Every worker persona names a fallback persona"), "The numbers of the design" (the rows of the profile: lease, stall, start timeout, retry backoff, attempts, declared wait).

## Goal

Each teammate file can state the numbers that the lease and the stall ladder read, and the roster refuses a worker persona with no fallback. Each worker persona of `teammates/` names 1 fallback, so the roster of this repository still loads.

## Requirements

- **RR-11** (the roster part): every worker persona names a fallback persona. The roster refuses to load a persona without one.
- **FM-3** (the number): `liveness.max_wait_s` caps a declared wait of an interactive persona (default 1 h).
- **RR-6** (the numbers): `retry.backoff_s` (30, 120, 480) and `retry.max_attempts`.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q8 and Q10 apply here).
- The teammate frontmatter is parsed into `Teammate` in `crates/horch-core/src/roster/teammate.rs`. It has `fallbacks: Vec<String>` (`teammate.rs:250`) today: "Teammates whose launch settings a spawn borrows when this one's usage pool cannot serve it". The design's `fallback` is the first entry of `fallbacks` (MAP.md, Q8). Do not add a second field with the same meaning.
- Today's rules of fallbacks are in `crates/horch-core/src/roster/validation.rs`: `fallback_problems` (`:1775`, each fallback exists) and the rule at `:323`: a top-tier seat (`TOP_TIER_SEATS`, `teammate.rs:34`) must not define fallbacks. Keep that rule. The new rule of "required" exempts (MAP.md, Q8):
  - the 2 top-tier seats (`fable-creative`, `astra-creative`): they keep no fallback, so on a usage limit their task waits in `waiting_quota`;
  - the fleet orchestrators (`FLEET_ORCHESTRATORS`, `teammate.rs:38`): not a worker persona;
  - a teammate with `agent: none` (`teammates/smoke.md`): it runs no agent.
  Each other teammate needs at least 1 entry in `fallbacks`. `Roster::check` (`validation.rs:864`) reports each one that has none, with the file name.
- The new optional frontmatter fields, with the design defaults when absent:
  - `mode: exec | interactive`. Default `interactive` in Phase 2 (MAP.md, Q10): each worker of today runs in a herdr pane. Phase 7 makes exec the default.
  - `liveness: { start_timeout_s: 90, stall_after_s: 300, lease_s: 120, max_wait_s: 3600 }`.
  - `retry: { max_attempts, backoff_s: [30, 120, 480] }`. `max_attempts` absent means the run's `max_attempts` of today (`runs.max_attempts`, preservation 04 row L4).
  - Validation: each number above 0; `lease_s` below `stall_after_s`; `backoff_s` not empty. An error names the file and the field.
- The pure read of these values for fleetd is `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs` (a stub from ticket 01): 1 value type, for example `Liveness { start_timeout_s, stall_after_s, lease_s, max_wait_s }`, `Retry { max_attempts: Option<u32>, backoff_s: Vec<u64> }`, `Mode`, and `fn fallback(&self) -> Option<&str>`. The domain imports nothing outward (`fitness_domain_imports_nothing_outward`): the roster builds the value and passes it in.
- The teammate files with no `fallbacks` today: `rg -L '^fallbacks:' teammates/*.md`, and `rg -l '^fallbacks: \[\]' teammates/*.md`. The rule for a new entry (MAP.md, Q8): `agent: claude` -> `codex-sol`; `agent: codex` -> `opus`; each other agent -> `opus`. A file with `fallbacks: []` gets the same rule, unless it is exempt. Do not change a file that is exempt. Do not change a fallback that a file names today. `teammates/_template.md` gets a commented line that names the rule.
- `teammates/README.md` describes the fields. Add `mode`, `liveness` and `retry` in its form, 1 line each.

## Steps

1. Write the tests first, in `crates/horch-core/src/roster/tests.rs` and in `profile.rs`. Each fails before the code exists:
   - `profile_without_fallback_is_refused` (`ddd/08-code/implementation-plan.md`, Slice 2).
   - `top_tier_seat_orchestrator_and_agent_none_need_no_fallback`.
   - `liveness_and_retry_absent_give_the_design_defaults`.
   - `mode_absent_is_interactive_in_phase_2`.
   - `lease_not_below_stall_is_refused_with_the_field`.
   - `fallback_is_the_first_entry_of_fallbacks`.
2. Write the code. Add the fallback to each teammate file that needs one.
3. Run the Check (`nfr_03_repo_roster_check_passes` is in it) and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. The roster refuses a worker persona with no fallback, and each teammate file of this repository loads.
