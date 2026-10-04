# Combined Godot skills: the common method (units GW3 to GW7)

Follow `ai_docs/plans/godot/00-godot-conventions.md`. Your unit plan names
your skills and their sources (also in `ai_docs/reports/godot-wave.md`,
"Combined skills (36)").

## Method, per combined skill

1. Base: run `scripts/godot/rename.py` on the GodotPrompter skill (wait for
   GW0's "rename.py ready" NOTE; do step 2 first while you wait). The
   renamed SKILL.md body stays unchanged.
2. Gap analysis: read the GodotPrompter skill (SKILL.md and references) and
   the gd-agentic skills listed for it. List what gd-agentic covers that
   GodotPrompter lacks. Only that list is added. Drop anything that is wrong
   on 4.7.2 (check with the doctool dump), deprecated, or a duplicate.
3. Write `references/<topic>.md` in your own words and your own code (the
   LGPL rule). Each reference starts with one line saying what it adds and
   when to read it. Code blocks are complete, idiomatic Godot 4.7 GDScript
   (typed), and pass `gdscript_blocks_check.py` and `api_check.py`.
4. Extend the `description:` (≤ 1024 bytes in total) to name the added
   coverage, and add one line per new reference in the SKILL.md's existing
   reference list or at its end, under a heading "Fleet additions". These two
   are the only SKILL.md edits.
5. Run both checks on the whole skill, including the upstream text. Record
   upstream findings in your report; do not edit upstream text.
6. Provenance: `sources` = the GodotPrompter SKILL.md, every other copied
   upstream file and the LICENSE (sha256 each, from rename.py's output);
   `vendored: true`; `adaptation`: "rename to godot-* prefix; description
   extended; references/<files> are own text, consulted
   thedivergentai/gd-agentic-skills@4c4d0ff: <skill dirs>".

## Commit

Commit in batches of 2 or 3 skills (lock, index files, `skills_catalog`,
commit, release), so other units can see your names early.

## Report

`ai_docs/reports/godot/<unit>.md`: per skill, the gap list (added / dropped
and why), check results, upstream findings.
