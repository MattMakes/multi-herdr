# R7: watchlist coverage check and review of each uncovered point

**Persona:** primary `codex-sol`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/watchlist-coverage.md && grep -q '^## Uncovered lines' docs/specs/fleetd/reviews/watchlist-coverage.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/watchlist-coverage.md`

## Goal

The file `ai_docs/plans/fleetd-build/for-operator/end-review-watchlist.md` has 77 lines. Each line is 1 point that an earlier worker marked for the end reviews. 6 end reviews are done. Prove that each of the 77 lines has a review result, and review each line that has none.

## Read first

- `ai_docs/plans/fleetd-build/for-operator/end-review-watchlist.md` in your worktree.
- The 6 reviews in `docs/specs/fleetd/reviews/end-review-*.md`: the section `## Watchlist lines covered` of each.
- `ai_docs/plans/fleetd-build/decisions.md`: search it for the T-number of a watchlist point to get the ruling.
- `docs/specs/fleetd/design.md`: the table "Where each rule becomes green".

## Steps

1. Give each of the 77 lines a number (its line number in the file).
2. Make a table: line number, short text, the review that covers it (R1 to R6), the result that the review gives for it.
3. A line is covered only when a review names it and gives a result. A line that a review only lists is not covered.
4. For each uncovered line: read the code and the test on your worktree and review the point yourself. Give a rank: Critical, Important, Minor, or no fault. Give file:line evidence. Label each claim "verified" or "not verified".
5. Do not change code. Do not run cargo test (the Check needs only `cargo fmt --all --check`). You start no process, so skip the `ps` check. Do not run `npx tsc`.

## Report

Write `docs/specs/fleetd/reviews/watchlist-coverage.md` with the sections: `## Coverage table`, `## Uncovered lines` (your review of each; write "none" if there is none), `## Findings` (ids R7-F1 and up, with rank), `## Operator decisions`. Commit the file.

## Out of scope

Fixes. The orchestrator writes 1 fix ticket for each Critical and Important finding.

## Done when

The report is committed, each of the 77 lines is in the table, and each uncovered line has a rank.

## Rules



1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

