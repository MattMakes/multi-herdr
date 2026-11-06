# 31: Review: delivery, supervision and respawn

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** none

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 17 (Answers close 1 question by id); 24 (Respawn a dead orchestrator, rarely). Wave 8.

## Goal

Get a second-vendor review of the riskiest concurrency changes before the gate.

## Steps

1. Read the diffs of tickets 05, 14, 15, 16, 17 and 24 on the run branch.
2. Look for: lost or duplicated wakes, a transaction that sends, a respawn that can run twice, a lock that a live holder can lose.
3. Do not edit code. Use a scratch worktree for experiments.

Amendment (orchestrator, decision R10): ticket 14 writes the outbox row right after the ledger commit, not inside it, because the call sites belong to other tickets. Judge this window: which wake events can a crash between commit and outbox write lose, and does ticket 15's 30 s reconcile recover each one? List each unrecovered event as a finding.

Amendment: also review ticket 42 if it has landed when you start (orchestrator launch updates the run's workspace and clears the stop).

## Done when

`ai_docs/reports/trace-fixes/31.md` lists findings as Critical, Important or Minor, each with file:line and a reproduction.

## Report

Write `ai_docs/reports/trace-fixes/31.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
