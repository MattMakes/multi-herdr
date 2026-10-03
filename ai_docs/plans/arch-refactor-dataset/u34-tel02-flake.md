# U34 tel02-flake: make tel_02_fleet_writes_orchestrator_record reliable and leak-free

Unit slug: `tel02-flake`. Branch: `ard/tel02-flake`. Phase: maintenance (no new requirement).

## GOAL

`tel_02_fleet_writes_orchestrator_record` (in `crates/horch-e2e/tests/e2e.rs`,
about line 240) passes 30 of 30 runs under parallel load, and a failing run
never leaves a `horch telemetry --state-dir <tmp>` process alive.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Observed: it fails about once in 6 full e2e runs. When it fails, the
  `horch telemetry` collector that `horch fleet` starts stays alive (2 such
  processes were found from old worktrees). Cause: `stop_collector(&h)` runs
  only at the end of the test; a failing assert skips it.
- Likely flake causes to check (find the real one; do not guess): the
  ledger read races the orchestrator record write (the test reads the
  ledger right after it sees the fake claude call, but the record may be
  written after the pane launch); `wait_for` timeout too short under load;
  the ledger file being replaced mid-read (read must tolerate an atomic
  replace, so retry the parse inside `wait_for`).
- Parallel units: U30 `b3-coordinator`, U31 `b4-judge-job`. They do not
  touch `e2e.rs` or the helpers below.

## FILES

own:
- `crates/horch-e2e/tests/e2e.rs` (only `tel_02_fleet_writes_orchestrator_record`
  and the `stop_collector` helper)
- `crates/horch-e2e/src/lib.rs` (only if a `Drop` guard for the collector
  belongs on `Harness`; additive)
- `ai_docs/reports/arch-refactor-dataset/tel02-flake.md`

do not touch: product code. If the cause is in product code (for example
`horch fleet` writes the record after it starts the agent and that order is
wrong), stop and send `QUESTION:` with the evidence.

## STEPS

1. Create the worktree (conventions §2).
2. Reproduce: run the test 30 times in a loop while
   `cargo test -p horch-e2e` runs in parallel in another shell, and record
   the failure messages. Check: you have at least 1 failure message, or 30
   passes with notes on load.
3. Fix the leak: a guard whose `Drop` stops the collector, so every exit
   path stops it. Check: make the test fail on purpose once (locally, not
   committed); `pgrep -f 'horch telemetry --state-dir'` finds none of its
   processes afterwards.
4. Fix the flake at its cause (most likely: wait for the orchestrator
   record inside `wait_for`, parsing the ledger each poll).
5. Run the loop of step 2 again: 30 of 30 pass. Run the gate.
6. Commit `Tests: Make tel_02 fleet test reliable and stop its collector on failure`.
   Write and commit the report. Follow conventions §6.

## DONE WHEN

- 30 of 30 passes under load; no leaked collector after a forced failure.
- `just gate` is green.

## REPORT

- `horch done` summary: the cause, the fix, the loop results.
