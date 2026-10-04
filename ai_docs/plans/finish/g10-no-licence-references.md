# G10 no-licence-references: enact the operator's licence rule on design-skills

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## THE RULE (operator, 2026-10-04, verbatim)

> As a general rule for this project, since we are creating copies ourselves
> that will grow on their own and this will only be used personally, we need
> not worry ourselves with burdens of NOTICE.md LICENSE.md or holding onto
> references to MIT licensing, APACHE, LGPL or otherwise - these people have
> made these ideas available on the internet. Remove all references to these
> licenses. We do not need to hold onto them or credit them. Go through this
> branch and enact this rule.

## GOAL

No tracked file on design-skills keeps a third-party licence file, a licence
reference (MIT, Apache, LGPL, BSD, "licence"/"license" of an upstream), or
an upstream credit (who a skill was copied or adapted from), and the gate
stays green. The repository's own code has no licence field today; leave
that as it is.

## CONTEXT

- A pi session started this and stopped halfway. Its work is saved in
  `.worktrees/_scratch/pi-licence-rule/` (staged-deletions.patch: the 117
  deletions; unstaged.patch: build.rs and skills_catalog.rs; vendored.json:
  96 names; earlier-aside/: its catalog.rs and skillscmd.rs patches). Use it
  as a draft, not as truth. Its mistakes: it deleted
  `crates/horch-e2e/tests/routing_provenance.rs`, which is the ARC-14 routing
  test ("how routing chose a record") and has nothing to do with licences:
  keep it. "Provenance" also names routing and telemetry provenance in this
  codebase: change only the skill-licence/credit meaning.
- What the skill provenance does today, and must still do without credits:
  (1) the bundled size budget exempts verbatim copies (`vendored: true`);
  (2) `REPO_ORIGINAL` / `skl_01` check own-text skills; (3)
  `gdscript_blocks_check.py --strict-own` uses each skill's `sources` paths
  to know which files are copied (report-only) and which are own text
  (strict); (4) `horch skills show` prints upstream lines; (5) the Godot
  `godot-commit.sh` and `godot-index-merge.py` merge provenance entries.
  Design one neutral replacement with no repository names, revisions,
  licences or credits: for example `skills/copied.json` mapping a skill to
  `{ "copied_files": [...], "verbatim": bool }`. Pick it, use it everywhere
  (Rust, scripts, gate, tests), and write the decision in the report.
- Remove: every `skills/**/LICENSE*` and `NOTICE*`; licence lines and
  "Upstream source" credit columns in `skills/README.md`; credit and licence
  text in teammate files, `docs/`, `README.md`, and `ai_docs/` (plans,
  designs, reports — including the Godot conventions' "LGPL rule" and
  "consulted thedivergentai/..." lines, which become plain facts with no
  credit); `scripts/godot/rename.py` copying LICENSE; credit/licence lines
  inside skill text (for example "MIT licensed", "adapted from X").
- Do not touch `.claude/worktrees/` (another branch's checkout) or
  `.worktrees/` (scratch).
- GW13 (opus-87) edits `scripts/godot/` and `scripts/phase-gate.sh` now.
  Do the Rust, data and docs parts first. Before you edit `scripts/godot/` or
  `phase-gate.sh`, ask the orchestrator; it tells you when GW13 has committed.
- Godot skills are committed with `.worktrees/godot-commit.sh`, which merges
  provenance. Update that script and `godot-index-merge.py` (they are
  untracked helpers in `.worktrees/`) to the new file in the same change.

## STEPS

1. Inventory: `git grep -n -iE "\b(MIT|LGPL|Apache|BSD|licen[cs]e|NOTICE)\b"`
   plus credit phrases ("adapted from", "vendored from", "upstream",
   repository URLs of skill sources). Classify each hit. Put the inventory
   in the report.
2. The replacement file and the Rust changes (catalog, build.rs,
   skillscmd, tests), compiling and green. COMMITTED.
3. Delete the licence files; strip the README credits; skill text. COMMITTED.
4. Docs, teammates, ai_docs. COMMITTED.
5. Scripts and the gate step (after GW13). COMMITTED.
6. A last `git grep` shows no remaining hit except words that are not about
   licences (list them in the report).

## CHECKS

`cargo build --workspace --all-targets`, horch-core lib, `skills_catalog`,
`baseline_oracles`, `arch_scan`, the e2e routing_provenance and
skills_exposure tests (after `cargo build --workspace --bins`), the Godot
script tests, `horch teammates --check`, clippy -D warnings, rustfmt on your
files.

## FILES

own: everything the rule touches, except `scripts/godot/` and
`scripts/phase-gate.sh` until the orchestrator says GW13 has committed.

## REPORT

`ai_docs/reports/finish/no-licence-references.md`. `horch tell orchestrator
"NOTE: COMMITTED <sha>..."` per step, then `horch done`.
