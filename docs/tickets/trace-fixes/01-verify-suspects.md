# 01: Verify the suspected behaviours and measure harness-adapter use

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** none

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Confirm or refute each behaviour the trace only suspected, so the fix tickets act on facts.

## Steps

1. Use `horch ledger worktree scratch t01` for any experiment. Change no tracked file.
2. Check D2: does a pane whose claim found no task stay open? Check L3: does a session still over its threshold after compaction start a new request cycle?
3. Check whether a redelivered or retyped `Spawn:` wake can make the orchestrator spawn twice for one task (connect Q3), and whether the Codex relay path confirms delivery like the Claude MCP path (connect Q4).
4. Check whether reap or stop updates the reaped worker's session-ledger record (connect Q5).
5. Measure which of the 6 harness adapters under `crates/horch-core/src/harness/` a normal run uses (strategize Q4).
6. For each confirmed behaviour, name the ticket of this plan that fixes it. For each refuted one, say so with evidence.

## Done when

`ai_docs/reports/trace-fixes/01.md` answers all 6 items with file:line evidence, each labelled confirmed, refuted or not reproducible.

## Report

Write `ai_docs/reports/trace-fixes/01.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
