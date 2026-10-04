# F4 usage-doctor: the judge counted twice, and doctor knows Blender

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

1. `exp_07_candidate_and_judge_in_usage` (`crates/horch-e2e/tests/usage_dataset.rs`)
   failed once under high load and counted the judge 2 times. Find whether
   the product double-counts (two ledger rows, a usage reader reading one
   transcript twice, a retry recorded as two judges) or the test does, fix
   the root cause, and add a deterministic test for it (force the
   interleaving with a hook, not a sleep).
2. `horch doctor` checks Blender when an offered teammate needs it: add
   `blender` to `requires:` (the enum V2 added; it has `xcode` only), give
   `teammates/blender-artist.md` `requires: [blender]`, and make doctor
   check `blender --version` (or `BLENDER_PATH`) and warn with the fix.
   Also note in the blender-artist persona and docs that port 9876 is the
   default of both the Blender Lab MCP add-on and the `ahujasid` add-on:
   running both clashes.

## CONTEXT

- Read `ai_docs/reports/finish/spec-b-events.md` (opus-53 saw the failure),
  `ai_docs/reports/domain-skills/roster-offer.md` (requires/doctor), and
  `ai_docs/reports/finish/blender.md`.

## FILES

own: the usage/ledger code you find at fault, `crates/horch-e2e/tests/usage_dataset.rs`,
`roster/teammate.rs` (the enum), `crates/horch/src/cmd/doctor.rs`,
`teammates/blender-artist.md` (requires + 1 note), tests,
`ai_docs/reports/finish/usage-doctor.md`.

## STEPS

1. Item 1: reproduce, cause, fix, test. 2. Item 2 with tests. 3. Targeted
checks; COMMITTED.
