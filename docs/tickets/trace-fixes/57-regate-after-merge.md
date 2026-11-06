# 57: Full gate on the run branch after the PR #338 merge

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** none

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator. Prior gate: ticket 33, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/33.md`. Merge: ticket 56, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/56.md`.
Waits for: 56, 58.

## Goal

Prove the run branch is releasable again after PR #338 was merged into it.

## Steps

1. Run `horch ledger status` and confirm every task is `done`. If one is not, send `horch tell orchestrator "BLOCKED: T-57 waits for <tasks>"` and end your turn.
2. In a scratch worktree at the run-branch tip (`git rev-parse fleet/trace-fixes`), run `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` once.
3. Confirm `scripts/check-req-coverage.sh` finds a test for every TRF id.
4. On a failure, send QUESTION with the failing step, the test and the owner (ticket 56 or an earlier ticket). Do not fix it here, and do not rerun the gate hoping it passes.
5. The run gate row cannot be recorded with installed horch 0.4.5 (report 33). Never set `HORCH_ALLOW_LEDGER_MIGRATE=1`. Never run a branch-built horch against the shared ledger.

Amendment (report 58), before step 2: in your task worktree, change `teammates/README.md:337` from `intake-sanitizer | claude, sonnet, medium` to `claude, opus, medium` (ticket 58 moved the role to Opus), run `cargo fmt --all --check`, commit that 1 file by exact path, and run `horch ledger finish` once so it lands. Then gate the new tip. This ticket's Check is none; the file is approved.

## Done when

The gate prints GATE GREEN on the tip, and report 57 names the tip sha.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/57.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
