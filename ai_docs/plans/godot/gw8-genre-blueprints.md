# GW8 godot-genre-blueprints: one router bundle for all 27 genres

Follow `ai_docs/plans/godot/00-godot-conventions.md`. Own text only: every
source here is gd-agentic, read for facts only, so nothing is copied.

## GOAL

`skills/godot-genre-blueprints/` holds a router `SKILL.md` (how to pick a
genre, then read one reference; ≤ 12 KB) and one `references/<genre>.md` for
each of the 27 `godot-genre-*` skills in gd-agentic (operator decision 2:
all 27), plus `references/project-templates.md` from its project-templates
skill.

## Method, per genre

1. Read the gd-agentic genre skill and its scripts. Extract the facts: core
   loop, the systems the genre needs, the scene/node layout, the pitfalls.
2. Write the reference in your own words: core loop; required systems, each
   pointing at the combined `godot-*` skill that covers it (names are in
   `ai_docs/reports/godot-wave.md`; do not repeat what those skills teach);
   a 4.7 scene tree sketch; 1 or 2 short own GDScript blocks only where they
   carry something no other skill has; pitfalls.
3. Keep each reference under about 8 KB. The router lists all 27 with a
   one-line "pick this when".
4. Checks: `api_check.py` and `gdscript_blocks_check.py` (GW0) on the whole
   bundle.

Commit in batches (router + 6 to 9 genres per commit) with the lock.
Provenance: `sources: []`, `vendored: false`, adaptation "own text, consulted
gd-agentic-skills@4c4d0ff: <the 28 dirs>".

## FILES

own: `skills/godot-genre-blueprints/`, lock-held edits of the 2 index files,
`ai_docs/reports/godot/gw8-genre-blueprints.md`.
