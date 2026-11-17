# 07: The wrapper heartbeat: the supervisor checks in every heartbeat_every_s, apart from the model

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib taskledger::supervisor && cargo test -p horch-core --lib execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch-core/src/execution/lifecycle.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("Liveness comes from outside the model": the row "Wrapper heartbeat"; "Lease renewal is earned, not claimed"), "The numbers of the design" (the row "Wrapper heartbeat").

## Goal

The wrapper heartbeat of Phase 2 is the checkin of today's supervisor (`horch worker`), at the period `heartbeat_every_s` of `fleet.toml` (10 s). A thread that does not depend on the model writes it. The lease of ticket 08 reads it.

## Requirements

- **RR-1** (the signal): liveness is judged from outside the model: the process, the wrapper heartbeat, hook and output activity. The heartbeat is the second of these.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q6 applies here).
- Today: `crates/horch-core/src/taskledger/supervisor.rs` writes `workers.last_checkin_at` every `interval(db, worker_id)` (`supervisor.rs:71`), which reads `runs.heartbeat_every_s` (default 30 s, `schema.rs`). `start_checkins` (`supervisor.rs:91`) runs the thread. `crates/horch-core/src/execution/lifecycle.rs:803` starts it for each supervised agent.
- The change: the period is `heartbeat_every_s` of `fleet.toml` (ticket 05, `FleetConfig`; the parser is `load` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`). The fleet root of the ledger is `RuntimePaths::fleet_root` (`crates/horch-core/src/runtime/paths.rs:39`). A file that cannot be read gives the default 10 s, with 1 line on stderr that names the file: a heartbeat never stops for a config error.
- A run whose `runs.heartbeat_every_s` is set: keep the shorter of the 2 periods, so no run beats more slowly than before.
- The heartbeat file `heartbeat.json` in the attempt folder is Phase 5, with the file protocol (MAP.md, Q6). Do not write it now.
- An old supervisor of a Phase 1 binary checks in every 30 s. The lease of ticket 08 renews when the heartbeat is under 30 s old (`heartbeat_max_age_s`), and the lease is 120 s, so an old supervisor still renews a lease on most passes. Ticket 17 tests this.
- Keep each rule of today's checkin: the thread ends within 1 tick of `stop`; a failed write is recorded by `record` (`supervisor.rs:148`) and does not end the thread.

## Steps

1. Write the tests first, in `supervisor.rs`. Each fails before the code exists:
   - `interval_is_heartbeat_every_s_of_fleet_toml`.
   - `interval_keeps_the_shorter_run_period`.
   - `unreadable_fleet_toml_gives_the_default_period`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. A supervised agent checks in every `heartbeat_every_s` (10 s by default).
