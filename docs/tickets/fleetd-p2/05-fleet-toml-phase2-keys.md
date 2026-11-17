# 05: The Phase 2 keys of fleet.toml

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`

Spec: `docs/specs/fleetd/design.md`, "The numbers of the design" (the rows "Wrapper heartbeat", "Heartbeat age for a lease renewal", "Time limit of a question", "Replan limit"), "Supervision, liveness and reconciliation" ("Lease renewal is earned, not claimed"; "Stall ladder").

## Goal

`fleet.toml` has the keys that Phase 2 reads, each with its default or starting value and its range, in the same form as the keys of Phase 1.

## Requirements

- **RR-1** (the numbers): the wrapper heartbeat every 10 s; a lease renews only when the heartbeat is under 30 s old.
- **RR-11** (the number): the usage of a pool that counts as full use (MAP.md, Q1).
- **RR-6** (the number): the replan limit of a `failed` task.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q1 applies here).
- Phase 1 (ticket 04 of `docs/tickets/fleetd-p1/`) wrote `FleetConfig` in `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs` and the parser in `adapters/config_file.rs` (`load`, `parse`; an unknown key is refused with its name; an absent key keeps its default). Follow that form: 1 field and 1 doc comment for each key, with its class (default or starting value) and its source.
- The keys:

| Key | Value | Class | Range |
| --- | --- | --- | --- |
| `heartbeat_every_s` | 10 | Starting value | 1 to 60 |
| `heartbeat_max_age_s` | 30 | Starting value | greater than `heartbeat_every_s`, at most 300 |
| `question_limit_s` | 900 (15 min) | Starting value | 60 to 86400 |
| `replan_limit_s` | 1800 (30 min) | Starting value | 60 to 86400 |
| `pool_full_use` | 0.98 | Starting value | greater than 0.5, at most 1.0 |

- `pool_full_use`: the used share of the binding window of a pool at which the pool counts as limited (a usage limit, RR-11), and below which the pool probe "reports ok" for the exit of `waiting_quota`. The starting value 0.98 is the value of today for the same fact: `exhausted_used` in `crates/horch-core/src/routing/policy.rs:75`, the threshold at which today's routing calls a pool `exhausted`. Name that source in the doc comment. MAP.md Q1 asks the operator to confirm it.
- The design names the replan limit `replan_limit`. Use the key `replan_limit_s`, so the unit is in the name as in each key of Phase 1 (MAP.md, Q1).
- The per-profile numbers (`lease_s`, `stall_after_s`, `start_timeout_s`, `max_wait_s`, `backoff_s`, `max_attempts`) are not keys of `fleet.toml`. Ticket 06 adds them to the profile.

## Steps

1. Write the tests first, next to the tests of Phase 1 in `config_file.rs` and `config.rs`. Each fails before the code exists:
   - `phase2_keys_have_their_defaults` (a missing file gives each value of the table).
   - `heartbeat_max_age_must_be_above_heartbeat_every` (refused with both key names).
   - `pool_full_use_out_of_range_is_refused`.
   - `a_file_sets_each_phase2_key`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. Each key of the table parses, keeps its default when absent, and is refused out of its range with its name.
