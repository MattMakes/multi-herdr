# D17 polish: close the small gaps the last units reported

Unit slug: `polish`. Branch: `ds/polish`.

## GOAL

Each gap below is fixed at its cause, with a test that fails on the old code.

## CONTEXT

- Read first: `00-conventions.md` (worktrees now under
  `/Users/mascott/projects/multi-herdr/.worktrees/`), and the reports
  `competition-gaps.md`, `lifecycle-gaps.md`, `followups.md`,
  `gsap-factcheck.md` in `ai_docs/reports/design-skills/`.
- D16 `dead-code` runs in parallel and deletes unused items across the tree.
  Touch only the lines these gaps need; rebase handles the rest.

## THE GAPS

1. **Planner determinism** (`competition/planner.rs`, `diversity.rs`): D13
   saw the third pick change between identical runs, and 1 pick had no
   price. Same inputs must give the same plan (seeded by round id). A pick
   with no price must not be planned silently: price it or exclude it with a
   recorded reason. Test: `cmp_06_plan_is_deterministic_and_priced` (or the
   planner's existing ID; check `check-req-coverage.sh`).
2. **One cost model** (`crates/horch/src/dataset/preflight.rs` `cost_of` vs
   `competition/budget.rs` `UsageMeter::projected`): preflight and the
   running budget must price a candidate the same way. Make one function the
   source; test that both callers agree.
3. **Receipt publish label** (`competition/promotion.rs`): the receipt says
   `merge_ff_only` for the new CAS + `read-tree` publish. Record the real
   mode (a new enum value with serde default; old receipts still parse; no
   golden change, else `QUESTION:`).
4. **Gone pane vs unreachable herdr in `horch done`**
   (`execution/lifecycle.rs`, `workspace/client.rs`): today both exit 0.
   After the summary is recorded, if herdr itself is unreachable, `done` must
   say so on stderr and exit non-zero (the pane may still be open), while a
   gone pane on a reachable herdr stays exit 0. Add a cheap reachability
   check to `WorkspaceClient` (and `FakeWorkspace`). Unit tests for both.
5. **fsx breaker residual** (`fsx.rs`): 2 processes can remove the same
   stale breaker dir at once and re-open the double break. Make breaker
   cleanup itself exclusive (for example rename the stale breaker to a
   unique name first; only the process whose rename succeeds removes it).
   Extend `dirlock_concurrent_breakers_keep_one_holder` with a stale breaker
   present at start.
6. **GSAP `easeReverse`** (`skills/motion-gsap/`): GSAP 3.15 adds it; add it
   where eases are documented, with the gsap.com URL in the report. Keep
   the size budget.

## FILES

own: the files named in each gap and their tests;
`ai_docs/reports/design-skills/polish.md`.

## STEPS

0. Create the worktree (conventions §3 and §8).
1. One commit per gap, each with its test. Gate per commit.
2. Report: per gap the cause, the fix and the test. Follow conventions §7.
