# 10: add-map reads the contract, dry-runs, snapshots and refuses duplicates

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- taskledger trf_11 trf_12 trf_13 trf_14 && cargo test -p horch -- taskledger trf_11 trf_12 trf_13 trf_14`
**Own:** `crates/horch-core/src/taskledger/map.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch/src/cmd/taskledger.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 03 (One schema migration for every new field); 04 (Publish the ticket contract and make the writers produce it). Wave 2.

## Goal

Make `add-map` the strict consumer of contract C1 (D2, D14, D16).

## Requirements

- **TRF-11**: `add-map --dry-run` checks every ticket against C1, writes nothing, and names each error with its file and line. Test name starts with `trf_11_`.
- **TRF-12**: `add-map` stores each ticket's snapshot and sha; `claim` prints the snapshot even after the file changed or was deleted. Test name starts with `trf_12_`.
- **TRF-13**: `add-map` refuses a ticket already queued in the run unless `--again` is given. Test name starts with `trf_13_`.
- **TRF-14**: `Kind` sets the default `Review`; an explicit `Review` wins; `**Check:** none` is stored as an explicit no-check. Test name starts with `trf_14_`.

## Steps

1. Parse the C1 block: version comment, Kind, Review, Check none. Fill the ticket defaults from Kind.
2. Add `--dry-run`: validate every ticket and the MAP, print each error as `<file>:<line>: <problem>`, write nothing, exit 1 on any error.
3. Store `ticket_snapshot` and `ticket_sha` at add; `claim` prints the snapshot, then the path.
4. Refuse a ticket whose file is already a task in the run unless `--again`.

## Done when

The 4 tests pass; `horch ledger add-map --dry-run docs/tickets/trace-fixes/MAP.md` exits 0.

## Report

Write `ai_docs/reports/trace-fixes/10.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
