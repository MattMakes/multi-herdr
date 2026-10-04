# D13 competition-gaps: report

Unit: `competition-gaps`. Branch: `ds/competition-gaps`. Author: opus-33.

## Summary

- Gap 1: a publish into a checked-out clean branch is now a compare-and-swap.
  A commit that lands between the check and the publish gives
  NEEDS_INTERVENTION, and nothing is half-applied.
- Gap 2: `committed` spend is no longer 0. Each running candidate counts
  max(measured, projected). A round whose projected spend crosses the
  limit stops new launches before its measured spend does.
- 2 new tests. Each fails on the old code and passes on the new code.
- No event format, golden, oracle, lifecycle, skill or teammate changed.

## Gap 1: checked-out publish (`promotion.rs`, `vcs/git.rs`)

The sequence is in `GitPromotionEngine::publish_checked_out`:

1. Check: the checkout is on the target, HEAD is `dest_before`, and
   `status --porcelain` is empty. A mismatch is NEEDS_INTERVENTION.
2. `update-ref <target> <planned_after> <dest_before>` (the existing
   `update_ref_cas`). A lost swap is NEEDS_INTERVENTION. Nothing moved.
3. `read-tree -m -u <dest_before> <planned_after>` in the checkout (new
   `GitClient::read_tree_update`, after `update-index -q --refresh`). This
   moves the index and the files. On a failure, the ref goes back by
   `update-ref <target> <dest_before> <planned_after>`, and the result is
   NEEDS_INTERVENTION. The reason says if the ref is back or not.
4. Verify: the checkout is on the target and HEAD is `planned_after`.

Why this is atomic enough:

- Step 2 is the one atomic step. Git takes the ref lock and compares the
  old value. Any commit that landed after step 1 makes the swap fail.
- A `git commit` in the checkout also swaps HEAD's ref against the value it
  read. A commit that read HEAD before step 2 fails its own swap.
- In step 3, git checks every path before it writes. A local change in a
  path the promotion changes makes git refuse with no file written. Then
  the ref goes back by CAS, so nothing stays half-applied.
- A local change in a path the promotion does not change is kept, and the
  promotion goes through. This is the same as `merge --ff-only`.
- The remaining window is between steps 2 and 3. A `git commit` that starts
  in that window sees HEAD at `planned_after` with the old index. Step 4
  then sees a HEAD that is not `planned_after` and reports
  NEEDS_INTERVENTION. The window is 1 git process long.
- A crash between steps 2 and 3 leaves the ref at `planned_after` and the
  files at `dest_before`. `resume_promotion` sees the target at
  `planned_after` and, for a `merge_ff_only` publish, runs step 3 again
  (`settle_checkout`) before it writes the receipt. `read-tree -m -u` is
  idempotent here: when the index already matches `planned_after`, git
  keeps it, and keeps local edits too. I checked both cases by hand in a
  temp repo. If git refuses, the result is NEEDS_INTERVENTION. The ref does
  not move again (PRO-06).

Rejected alternatives:

- `git reset --keep <new>` after the swap: HEAD already is `<new>`, so
  `reset --keep` moves no files.
- Tree first, then the swap: a lost swap then needs a second tree update to
  undo, and a crash leaves the files ahead of the ref with no record that
  tells the restart to undo them.

Decisions:

- `GitClient::read_tree_update` has a default body that returns an error.
  The 3 `FakeGit` clients in `tests/judging.rs`, `tests/judge_input.rs` and
  `horch-e2e/tests/judge.rs` are outside my scope, and they need no change.
- The receipt and `promotion.started` still say `merge_ff_only` for this
  mode. The format does not change. The doc comment on `PublishMode` says
  that the name is historical.
- `GitClient::merge_ff_only` has no caller in `src/` now. I kept it because
  `tests/vcs.rs` and the 3 fakes use it.

Test: `pro_03_checked_out_publish_is_cas` in `tests/promotion.rs`. A
`Racy` client wraps `GitCli` and runs a closure just before the first CAS
on `refs/heads/main`, which is after the check. It has 4 cases:

- A commit lands on `main`: NEEDS_INTERVENTION, `main` and HEAD hold the
  racer's commit, the files are the racer's, there is no receipt.
- A local edit to `a.txt` (a path the promotion changes): the swap wins,
  `read-tree` refuses, `main` is back at the base, the edit is kept.
- A local edit to `b.txt` (a path the promotion does not change): promoted,
  and the edit is kept.
- A crash between the swap and the tree update (simulated: the
  `abort-after-promotion-started` fault, then a manual `update-ref`):
  `resume_promotion` twice gives Promoted, the files are the candidate's,
  the status is clean, and `main` does not move again.

## Gap 2: committed spend (`budget.rs`, `coordinator.rs`)

Policy (the orchestrator's decision pending the Spec B §budget text; the
`SPEC-RESOLVED(Spec B §budget)` is replaced by a comment that says this): (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3/§4.11, ai_docs/reports/finish/spec-b-preflight.md)

- Each running candidate: committed = max(measured so far, projected). In
  `BudgetPolicy::check` terms, `committed` is the further spend, so the
  coordinator passes `max(projected − measured, 0)` per running candidate.
  `spent + committed` is then Σ terminal measured + Σ running max(measured,
  projected).
- Projected: new `UsageMeter::projected(model)`. It prices
  `DEFAULT_TOKEN_ESTIMATE` at the table price, the same number preflight
  PRE-09 checks. The binary always passes empty `expected_tokens`, so the
  default estimate is the plan's projection. A model with no price projects
  0; its measured spend still counts.
- The judge: its reserve is held until it completes. The policy's limit is
  already `hard − judge_reserve`, and the budget check runs only while
  candidates run. Adding the reserve to `committed` too would count it
  twice.

Coordinator change: measured spend is still read only on check ticks
(every 5 s), and is cached by label. The policy now runs on every tick with
the cached values. Before, it ran only on check ticks, so a launch on any
other tick did not see the budget. A candidate launched since the last
check counts its full projection.

Test: `cmp_10_committed_spend_counts_running_candidates` in
`tests/coordinator.rs`. 3 candidates, `safe_n` 2, no transcripts (measured
spend 0). The limit is the projected cost of A plus B. A and B launch and
run to their own end (exit 2). C is `cancelled{budget}` and never starts.
The outcome is `Rejected { budget: true }`. On the old code C launches and
the outcome is `Rejected { budget: false }`.

## Gotchas

- The planner's third pick varies between runs with the same input. I saw
  `opus`, `sonnet` and `gpt-5.6-sol` for C, and in 1 gate run a pick with
  no price. The test uses only A and B, which `launch_wave` starts first in
  label order. The cause is in `competition/planner.rs` or the roster, not
  in my scope.
- The gate exports `HORCH_TEAMMATES_DIR`, so the roster in the gate can
  differ from a plain `cargo test`.

## Outside my scope (not fixed)

- The planner nondeterminism above. The design says a tie goes to the
  first teammate name, so the pick should be stable.
- `preflight.rs` has its own `cost_of`. `UsageMeter::projected` gives the
  same number through `nano_cost`, but the 2 copies can drift. A follow-up
  can make preflight call `UsageMeter::projected`.
- No e2e test in `horch-e2e/tests/dataset.rs`. The core tests cover both
  gaps with real git and the real store.

## Follow-ups

- Spec B §budget: confirm the policy. One option is to count the next
  candidate's projection before it launches. That stops a launch that by
  itself crosses the limit. Preflight already refuses a plan whose total
  projection crosses it, so I did not add it.
