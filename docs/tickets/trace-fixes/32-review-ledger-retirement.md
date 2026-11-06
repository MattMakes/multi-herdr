# 32: Review: session ledger retirement

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** none

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 29 (Stop writing the JSON session ledger). Wave 10.

## Goal

Get a second-vendor review of the widest refactor before the gate.

## Steps

1. Read the diffs of tickets 27, 28 and 29.
2. Look for: a reader still on JSON, lost history after import, a command that changed output without a test.
3. Do not edit code.

Amendment (review 31 did not cover them): also review tickets 42 and 44 (orchestrator launch claims its run; respawn launch-pending) for the C7 risks named in review 31 C-01 and I-02.

## Done when

`ai_docs/reports/trace-fixes/32.md` lists findings as Critical, Important or Minor.

## Report

Write `ai_docs/reports/trace-fixes/32.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
