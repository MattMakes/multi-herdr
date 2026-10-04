# L1 acceptance-dataset: run the dataset local acceptance checks for real

Unit slug: `acceptance-dataset`. Branch: `ds/acceptance-dataset` (report and
any fixes).

## GOAL

Run LA-6 to LA-11 from `ai_docs/designs/2026-10-02-dataset-competition-design.md`
§8 on this Mac with real agents, record the evidence, and fix (or hand to the
orchestrator as a unit) every defect found. LA-12 is covered differently (see
CONTEXT).

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`, the dataset design §8,
  `docs/command-flow.md` §3–4 and `multi-herdr-dataset --help`.
- Installed binaries are current (`~/.local/bin/horch`, `multi-herdr-dataset`,
  built from design-skills). Use `HORCH_TEAMMATES_DIR=/Users/mascott/projects/mh-wt/integ-ds/teammates`.
- **Money:** these runs use the operator's subscriptions. Keep them small:
  a scratch git repo under `/tmp/la-dataset/` with a tiny task (for example
  "add a function `slugify` with 3 tests" in a 2-file Python or Rust
  project), 2 candidates (`sonnet` and `codex-terra`, or `codex-luna`),
  `--budget-usd 2` or lower per round, a short deadline. At most 6 rounds in
  total. Never use opus, fable, astra or any top-tier model for a candidate.
- **NEVER export or set `ANTHROPIC_API_KEY`, not even to a sentinel value**
  (project rule, CLAUDE.md). So LA-12 is not run as written: record that the
  hermetic test `sec_08_e2e_no_api_key_in_any_child` covers it, and add one
  real check that needs no key: after a real round, `grep -r ANTHROPIC_API_KEY`
  over the state dir and `ps eww` of a live candidate show the name absent.
  Unset the variable in every shell you use: `env -u ANTHROPIC_API_KEY`.
- LA-7: watch for Claude/Codex trust dialogs stalling a candidate pane in a
  new worktree; test `--worktree-root` under a trusted parent. If a dialog
  stalls, that is a defect: find how to pre-trust (Claude settings, codex
  `projects` trust) and fix it in the product.
- LA-9 needs branches in the scratch repo: unchecked-out target, checked-out
  clean target, dirty target (expect NEEDS_INTERVENTION), moved target with a
  conflict.
- LA-11: `kill -9` the coordinator by exact pid (never by pattern) while
  running, judging and promoting; `resume`; check for duplicates.
- Do not touch the fleet workspace `w2F`. Close every workspace you create.
- Fixes: small fixes in your branch with tests; anything larger, send the
  orchestrator a QUESTION with the evidence and a proposed unit.

## FILES

own: `ai_docs/reports/finish/acceptance-dataset.md`; code you fix (tell the
orchestrator which files before you edit them: other units are active).

## STEPS

1. LA-6, LA-10 (no model calls). 2. LA-7, LA-8 (1–2 rounds). 3. LA-9, LA-11.
4. The key check. 5. Report: per check PASS/FAIL, evidence, cost spent
(`horch cost`). Merge protocol for any fixes (targeted checks; merge train).
