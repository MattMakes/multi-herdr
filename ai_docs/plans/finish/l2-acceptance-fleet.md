# L2 acceptance-fleet: run the fleet local acceptance checks for real

Unit slug: `acceptance-fleet`. Branch: `ds/acceptance-fleet`.

## GOAL

Run LA-3, LA-4 and LA-5 from the dataset design §8 on this Mac, record
evidence, and fix or hand over every defect.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`, design §8, the
  junior handbook (`docs/`), `horch --help`.
- LA-3 (session discovery, resume, `horch cost` totals equal an A0 ledger
  copy): this fleet (`w2F`) already ran dozens of real sonnet, opus and codex
  workers today; use `horch sessions --all` and `horch cost` on its ledger
  first. For opencode and resume, run real workers in a **separate herdr
  workspace you create** (never spawn into `w2F`): at most 3 workers
  (`sonnet`, `codex-terra`, `opencode-pickle`), each with a 1-line task in a
  scratch repo under `/tmp/la-fleet/` (opencode trains on input: the scratch
  repo must hold nothing private). Then `horch spawn --resume` one of them.
  Compare `horch cost` with a copy of the ledger made before.
- LA-4: with one Claude and one Codex worker, verify only the activated
  skills are visible (Claude `/skills` listing or its plugin dir; codex's
  `CODEX_HOME`).
- LA-5: `horch skills install MattMakes/skill-marketplace@<tag>` pins a SHA;
  then spawn works with the network off (`HORCH_GIT_BIN=/nonexistent` as the
  e2e test does, or disconnect if practical).
- Money: keep it to the workers named above. Never a top-tier model.
- NEVER set or export `ANTHROPIC_API_KEY`; run with `env -u ANTHROPIC_API_KEY`.
- Close every workspace and pane you create.

## FILES

own: `ai_docs/reports/finish/acceptance-fleet.md`; fixes (ask first, as L1).

## STEPS

1. LA-3. 2. LA-4. 3. LA-5. 4. Report (PASS/FAIL, evidence, cost).
