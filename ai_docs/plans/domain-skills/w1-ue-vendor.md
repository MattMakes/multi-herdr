# W1 ue-vendor: vendor the 31 Unreal Engine skills

Unit slug: `ue-vendor`. Branch: `ds/ue-vendor`.

## GOAL

All 31 upstream `ue-*` skills are bundled: 30 verbatim, and
`ue-project-context` adapted, so its interview becomes one `QUESTION:` to
the orchestrator. Each skill has its LICENSE and a provenance entry, and
`horch skills` lists all 31.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, then
  `ai_docs/reports/unreal-engine-wave.md` "Skills to add" (vendor and adapt).
- Source: `/Users/mascott/projects/multi-herdr/.worktrees/_sources/unreal/quodsoler_unreal-engine-skills/skills/<name>/`.
  Revision and licence: `.../unreal/PINS.txt`. The LICENSE is at the repo root.
- V0 (`ds/vendoring-infra`) adds the `vendored` flag, accepts `LICENSE`
  files, and exempts vendored skills from the size budget. It may land
  after you start. Write your provenance with `vendored: true` from the
  beginning. If the gate fails only on the budget, LICENSE or the
  `vendored` field before V0 merges, say so in your report and send
  READY-TO-MERGE. The orchestrator merges V0 first, then gives you REBASE.
- Do not add the skills to any `Phase`. They attach by name to the UE
  teammates (unit P-UE).

## FILES

own: `skills/ue-*/` (31 dirs), the `ue-*` entries in `skills/provenance.json`,
a "Unreal Engine skills" table in `skills/README.md`,
`ai_docs/reports/domain-skills/ue-vendor.md`.

do not touch: Rust code, teammates, other skills.

## STEPS

0. Create the worktree.
1. Copy the 30 verbatim skills with `rsync -a --exclude='.*'`, then add the
   repo-root `LICENSE` to each. Check: `diff -r` against the source shows
   only the added LICENSE.
2. Adapt `ue-project-context` exactly as the report says: keep step 1 and
   the document template, and replace the interview with `[unknown]` markers
   and one `QUESTION:` that lists unknowns in the skill's order. Keep the
   no-guessing rule. Record the edits in its provenance `adaptation`.
3. Provenance: one entry per skill, keep the file sorted by name, and give
   each copied file its sha256 (a script is fine; commit nothing but data).
4. Add the README table: name, upstream path, verbatim or adapted.
5. `cargo build`, `horch skills` (built binary), and the gate. Commit
   `Skills: Vendor the Unreal Engine skills`. Report: totals, bytes added to
   the binary, and anything odd in upstream.
