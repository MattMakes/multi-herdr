# D18 flake-and-durable: cmp_05 root cause, replace_durable, doc warnings

Unit slug: `flake-and-durable`. Branch: `ds/flake-and-durable`.

## GOAL

1. `cmp_05_e2e_candidates_in_dataset_workspace` never fails under load, and
   the report names the root cause with evidence.
2. `fsx::replace_durable` and `fsx::write_atomic` stop being two names for
   one idea: the design and the code agree.
3. `cargo doc --workspace --no-deps` prints 0 warnings.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`.
- Flake history: `ai_docs/reports/design-skills/followups.md` rows 3 and 4
  (D10 fixed the fake-herdr lock break and the macOS first-run scan). The new
  symptom after D10, seen twice on 2026-10-03 at load 7–9 with ~10 worktrees
  building: `cmp_05` saw 1 `candidate.completed` instead of 2
  (`ai_docs/reports/design-skills/dead-code.md` "Gotchas"; opus-41 also hit
  it). It passes alone. This is a real defect until proven otherwise: either
  the coordinator loses a completion under slow scheduling (a product bug —
  fix the product) or the test's deadline/poll is too tight (fix the test,
  and justify the number with a measurement).
- Reproduce under load: run the test in a loop (`cargo test -p horch-e2e
  --test dataset cmp_05 -- --exact` 30+ times) while a stress load runs
  (e.g. `yes > /dev/null` on N cores, or a parallel `cargo test` elsewhere).
  Add temporary event/tick logging if needed; remove it before commit.
- `fsx::replace_durable` (design §2.3 MEA-09, test `mea_09_replace_durable`)
  has no caller; the projection, manifest and export writers call
  `fsx::write_atomic`. Read both. If the semantics are identical, keep one
  name, move the callers, update the design text and the test map row. If
  they differ (fsync of the directory?), the writers the design names should
  use the durable one; switch them and say why.
- `cargo doc` warns on bare URLs at `crates/herdr-install/src/main.rs:3` and
  `crates/herdr-docs-sync/src/main.rs:3`.

## FILES

own: `crates/horch-e2e/tests/dataset.rs` and its support code,
`crates/horch-core/src/competition/**` only where the root cause is,
`crates/horch-core/src/fsx.rs` and its callers, the design doc's
replace_durable lines, the two `main.rs` doc lines,
`ai_docs/reports/design-skills/flake-and-durable.md`.

do not touch: skills, teammates, roster, routing. D17 (`ds/polish`) touches
`fsx.rs` (the breaker residual): rebase on design-skills before you start on
fsx, and tell the orchestrator if D17 is not merged yet.

## STEPS

0. Create the worktree.
1. Reproduce cmp_05 under load; record the failure rate before.
2. Find and fix the root cause. Record the rate after (0 of 30+ under the
   same load). Commit.
3. replace_durable / write_atomic. Commit.
4. Doc warnings. Commit. Gate. Report. Follow the merge protocol.
