# D22 install-checks: what the operator's first install showed

Unit slug: `install-checks`. Branch: `ds/install-checks`.

## GOAL

Two defects found by installing `design-skills` on the operator's Mac
(2026-10-04) are fixed at the root, with tests:

1. `horch smoke messaging` reports `FAIL: SMOKE_TEST_42 not observed on pane
   <id>` every time (3 of 3), while the dumped pane text right after it shows
   `SMOKE_TEST_42`. The check is `Herdr::wait_output(&b, "SMOKE_TEST_42",
   15_000)` (`crates/horch/src/cmd/smoke.rs:104`,
   `crates/horch-core/src/workspace/herdr.rs:396`). herdr is 0.8.2. Find why
   `wait_output` misses text that is on the pane (a herdr CLI change in
   `wait`/`read` arguments or output, a source such as recent vs visible, the
   echoed command line containing the needle, a timing bug). Find every other
   caller of `wait_output` and say whether it was affected (messaging and
   spawn may rely on it). Fix it so it works on herdr 0.8.2; add a test with
   the fake herdr that reproduces the real behaviour you found.
   Then close the leftover smoke workspaces the failing runs left open
   (`w2K`, `w2N`, `w2P`, `w2Q`, and any other `smoke` workspace; list them
   with herdr first, close only smoke workspaces, never the fleet's `w2F`).
2. `horch agent-list` reports `pi` as `available` with version `(no answer)`
   although `pi --version` crashes (`ERR_REQUIRE_ESM` under Node 22.9; the
   pi package needs Node ≥ 22.12). A harness whose version probe exits
   non-zero or crashes must show as `broken` (or similar) with the first
   error line, `horch doctor` must warn about it, and routing must not
   choose it. Check how `quota_probe::harness_version` and the agent-list
   status are computed; fix both with tests (a fake binary that exits 1 with
   a stderr message).

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` (never run the gate
  under `git rebase -x`).
- The real herdr server runs the operator's fleet in workspace `w2F`. You may
  run `horch smoke messaging` against it (it makes its own workspace). Do not
  touch `w2F`.
- Do not upgrade Node or pi; the operator decides that.

## FILES

own: `crates/horch-core/src/workspace/herdr.rs` (wait/read), `crates/horch/src/cmd/smoke.rs`,
`crates/horch-core/src/routing/quota_probe.rs`, the agent-list and doctor
commands, the fake herdr if needed, tests, `ai_docs/reports/design-skills/install-checks.md`.

## STEPS

0. Worktree. 1. Reproduce 1 and find the cause (report the evidence). 2. Fix
+ test. 3. Clean up the smoke workspaces. 4. Item 2 + tests. 5. Gate. Report.
