# 33: Full gate on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: 30 (Rewrite the briefings and docs for the new behaviour); 31 (Review: delivery, supervision and respawn); 32 (Review: session ledger retirement). Wave 11.

## Goal

Prove the run branch is releasable (D4: this ticket is the run gate until ticket 13's `run gate` exists, then it runs that).

## Steps

1. Run `horch ledger run gate --check "HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate"` on the run-branch tip.
2. Confirm `scripts/check-req-coverage.sh` finds a test for every TRF id.
3. On a failure, send QUESTION with the failing step and the ticket that owns the file. Do not fix it here.
4. Confirm that 31 and 32 have no open Critical finding.

Amendment: drop the scratch worktree `.worktrees/_scratch/t01` with `horch ledger worktree drop`. Its 1 uncommitted test is evidence for ticket 08 only. Name the result in your report.

Amendment (report 34): `utc_offset_date` in `crates/horch-core/tests/common/mod.rs` has no caller (the e2e crate has its own copy). Delete it if `git grep -n -e utc_offset_date -- crates/horch-core` still shows no caller.

Amendment (reports 10, 34, 35): ticket checks do not run `cargo fmt`. Before the gate, run `cargo fmt --all`, commit the changed files by exact path in 1 commit, then run the gate. Known drift: `taskledger/map.rs`, `taskledger/status.rs`, `tests/trf_02_ticket_template.rs`.

Amendment (report 26): `cargo fmt` drift also includes `crates/horch/src/cmd/spawn.rs`.

Amendment (orchestrator): the ledger cannot add blockers to this task after add-map. Before any step, run `horch ledger status` and confirm that every task from T-34 to the last task is `done`. If one is not, send `horch tell orchestrator "BLOCKED: T-33 waits for <tasks>"` and end your turn.

## Done when

The gate passes on the tip and the run gate row is recorded.

## Report

Write `ai_docs/reports/trace-fixes/33.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
