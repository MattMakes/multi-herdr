# 01: The module files of Phase 2 in task_execution, ideas and worker_profiles

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/application/mod.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/mod.rs`, `crates/horch-core/src/fleetd/ideas/domain/mod.rs`, `crates/horch-core/src/fleetd/ideas/application/mod.rs`, `crates/horch-core/src/fleetd/ideas/adapters/mod.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/domain/lease.rs`, `crates/horch-core/src/fleetd/task_execution/domain/stall.rs`, `crates/horch-core/src/fleetd/task_execution/domain/retry.rs`, `crates/horch-core/src/fleetd/task_execution/domain/quota.rs`, `crates/horch-core/src/fleetd/task_execution/domain/cancel.rs`, `crates/horch-core/src/fleetd/task_execution/application/lease.rs`, `crates/horch-core/src/fleetd/task_execution/application/stall.rs`, `crates/horch-core/src/fleetd/task_execution/application/retry.rs`, `crates/horch-core/src/fleetd/task_execution/application/quota.rs`, `crates/horch-core/src/fleetd/task_execution/application/cancel.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/leases_store.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/waits_store.rs`, `crates/horch-core/src/fleetd/task_execution/domain/waits.rs`, `crates/horch-core/src/fleetd/ideas/domain/questions.rs`, `crates/horch-core/src/fleetd/ideas/application/questions.rs`, `crates/horch-core/src/fleetd/ideas/adapters/questions_store.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 4 (Phase 2); `ddd/08-code/implementation-plan.md`, "Slice 2".

## Goal

Each new module file of Phase 2 exists, with its `mod` line and a module doc that names its ticket, so that the later tickets of this map own different files and run at the same time with no conflict.

## Requirements

None directly. This ticket prepares the files of the tickets that make RR-1, RR-2, RR-6, RR-11, FM-3, FM-5 and FM-14 green.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first. It records each choice of this phase, and each ticket follows it.
- Phase 1 (`docs/tickets/fleetd-p1/`, ticket 01) made the folders `crates/horch-core/src/fleetd/<context>/{domain,application,adapters}/` and the fitness tests `fitness_domain_imports_nothing_outward` and `fitness_no_context_touches_foreign_tables` in `crates/horch-core/tests/arch_scan.rs`. The stub files of Phase 1 (for example `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` before ticket 21) show the form: 1 module doc of 2 or 3 lines that names the ticket that fills the file.
- The files of this ticket and the ticket that fills each one:

| File (under `crates/horch-core/src/fleetd/`) | Filled by |
| --- | --- |
| `task_execution/domain/lease.rs`, `application/lease.rs`, `adapters/leases_store.rs` | 08 |
| `task_execution/domain/stall.rs`, `application/stall.rs` | 09 |
| `task_execution/domain/retry.rs`, `application/retry.rs` | 10 |
| `task_execution/domain/quota.rs`, `application/quota.rs` | 11 |
| `task_execution/domain/waits.rs`, `task_execution/adapters/waits_store.rs`, `ideas/domain/questions.rs`, `ideas/application/questions.rs`, `ideas/adapters/questions_store.rs` | 12 |
| `task_execution/domain/cancel.rs`, `application/cancel.rs` | 13 |
| `worker_profiles/domain/profile.rs` | 06 |

- `worker_profiles/domain/mod.rs` has 1 line today. Its `application/` and `adapters/` folders keep their files.
- Use the visibility of the `mod` lines of Phase 1 in each `mod.rs` (`pub mod`).

## Steps

1. Write each stub file with its module doc. Add each `mod` line, in sorted order.
2. Run `cargo fmt` on the changed files.
3. Run the Check, and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. Each file of the table exists with its module doc and its `mod` line, and no other file changed.
