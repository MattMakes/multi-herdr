# 04: Publish the ticket contract and make the writers produce it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core -- trf_02`
**Own:** `teammates/ticket-writer.md`, `teammates/spec-writer.md`, `docs/specs/ticket-format.md`, `docs/specs/trace-fixes.md`, `docs/tickets/trace-fixes/`, `crates/horch-core/tests/trf_02_ticket_template.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Make the ticket-writer's output queue as written (C1), and version this plan in git (D14).

## Requirements

- **TRF-02**: The ticket-writer template's example ticket and MAP parse under the C1 rules with no error. Test name starts with `trf_02_`.

## Steps

1. Write `docs/specs/ticket-format.md`: contract C1 with 1 full example ticket and 1 example MAP. No requirement table.
2. Rewrite the ticket-writer template and MAP format to C1: the field block, Own paths instead of Areas, a Check per ticket, no 'avoid file paths' rule. Output goes to `docs/tickets/<slug>/`.
3. Change spec-writer's output path to `docs/specs/<slug>.md`.
4. Write `trf_02_*`: extract the example ticket and MAP from `teammates/ticket-writer.md`, run them through `taskledger::map` parsing, and assert a primary persona, Own paths and a Check.
5. Commit `docs/specs/trace-fixes.md` and `docs/tickets/trace-fixes/` as they are on disk.

## Done when

The template's example parses with no error; the plan files are on the run branch.

## Report

Write `ai_docs/reports/trace-fixes/04.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
