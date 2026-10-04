# GW1 godot-prompter-only: the 18 GodotPrompter skills with no gd-agentic twin

Follow `ai_docs/plans/godot/00-godot-conventions.md`.

## GOAL

These 18 skills are in `skills/` with provenance and README rows:

- Renamed only (16): `gdscript-patterns`, `localization`, `hud-system`,
  `responsive-ui`, `multithreading`, `gdextension`, `addon-development`,
  `csharp-godot`, `csharp-signals`, `math-essentials`, `assets-pipeline`,
  `limboai`, `beehave`, `popochiu`, `dialogue-manager`, `phantom-camera`.
  The 5 addon skills name their pinned addon versions (limboai v1.8.0,
  beehave v2.9.2, popochiu v2.1.1, dialogue-manager v3.10.4, phantom-camera
  v0.11.0.2); check the upstream text says them, and if not, say so in the
  provenance `adaptation` rather than editing the text.
- Adapted (2): `godot-grill` and `godot-brainstorming`, exactly as
  `ai_docs/reports/godot-wave.md` "GodotPrompter only" says.
- Not carried: `using-godot-prompter`, `godot-mentor` (record in the report).

## STEPS

1. Wait for GW0's "rename.py ready" NOTE (the orchestrator relays it). Read
   the upstream skills meanwhile, and plan the 2 adaptations.
2. Run `scripts/godot/rename.py` for the 18. Do not edit the 16.
3. Adapt grill and brainstorming. Their provenance `adaptation` lists every
   edit, like `ue-project-context`'s.
4. Run `api_check.py` and `gdscript_blocks_check.py` on your 18 when GW0 has
   them. Do not edit upstream text to fix a finding: record each finding in
   the report (the API spot check found `OS.gc` in 1 reference file — if it is
   in your set, add a one-line note in the provenance `adaptation` and in the
   report; the orchestrator decides on an errata file).
5. Lock, provenance (`vendored: true`, `adaptation: "rename to godot-*
   prefix"` for the 16), README rows (create the "Godot skills" section with a
   short intro modelled on the Unreal one), `skills_catalog`, commit, release.

## FILES

own: `skills/godot-{the 18}/`, your lock-held edits of
`skills/provenance.json` and `skills/README.md`, `ai_docs/reports/godot/gw1-prompter-only.md`.
