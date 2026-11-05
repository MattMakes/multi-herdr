# 03: One schema migration for every new field

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_01`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/model.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Add every column and table this plan needs in 1 migration, so later tickets never race on the schema version.

## Requirements

- **TRF-01**: The schema migration upgrades an existing ledger with no data loss; new fields take their defaults. Test name starts with `trf_01_`.

## Steps

1. Implement spec section 4 exactly. Read the current code for each row first; if a field already exists, reuse it and note it.
2. Add the enum values the plan needs to model.rs: the ticket kind (column `work_kind`; `kind` already means work vs review) and review mode (C1), worker end reason `handed_off`, held reason `unlanded`, the outbox events of C3.
3. Do not add behaviour. Readers and writers of the new fields belong to later tickets.
4. Write `trf_01_*`: open a ledger at the previous version with rows in every table, migrate, and check that no row changed and the new fields hold their defaults.

## Done when

The migration runs forward on a populated ledger; the check passes.

## Report

Write `ai_docs/reports/trace-fixes/03.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
