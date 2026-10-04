# GW1 report: the 18 GodotPrompter skills with no gd-agentic twin

## What

- Commit 92a3e49: 18 skills in `skills/godot-*`, `skills/provenance.json` entries, `skills/README.md`
  "Godot skills" section (created by this unit).
- 16 renamed with `scripts/godot/rename.py` and not edited: `godot-gdscript-patterns`,
  `godot-localization`, `godot-hud-system`, `godot-responsive-ui`, `godot-multithreading`,
  `godot-gdextension`, `godot-addon-development`, `godot-csharp-godot`, `godot-csharp-signals`,
  `godot-math-essentials`, `godot-assets-pipeline`, `godot-limboai`, `godot-beehave`,
  `godot-popochiu`, `godot-dialogue-manager`, `godot-phantom-camera`.
- 2 adapted (`godot-grill`, `godot-brainstorming`) as `ai_docs/reports/godot-wave.md` says. The
  provenance `adaptation` lists every edit. Extra edits: the `godot-grill` description is rewritten
  and quoted-safe (no colon plus space); the `godot-mentor` cross-reference is dropped.
  `godot-brainstorming` is 10666 bytes (upstream 12950).
- Follow-up: `godot-gdscript-patterns/references/fleet-additions.md` (own text): `@export_file`
  stores `uid://` since 4.4, `@export_file_path` keeps `res://` since 4.5 (item 3 from GW10).

## Not carried

- `using-godot-prompter` and `godot-mentor`: they describe the GodotPrompter plugin and its
  session-start hook, which this catalog does not have.

## Sources consulted

GodotPrompter v1.14.0 at `3e8d0f005f9604e1dbdad3de693e39555384c5af`. The sha256 of every copied
upstream file is in the provenance `sources`. gd-agentic-skills was not consulted.

## Checks

- Addon versions: the upstream text of all 5 addon skills names the pinned version (limboai v1.8.0,
  beehave v2.9.2, popochiu v2.1.1, dialogue-manager v3.10.4, phantom-camera v0.11.0.2). Each
  provenance `adaptation` says so.
- `OS.gc`: not in any of the 18 skills.
- `api_check.py` on the 18: 58 files, 0 unknown names.
- `gdscript_blocks_check.py` on the 18: 108 upstream findings, none edited (upstream blocks are
  fragments or use addon classes the project does not define). Count by skill: gdscript-patterns 19,
  popochiu 19, math-essentials 16, limboai 11, beehave 10, addon-development 8, hud-system 6,
  phantom-camera 6, localization 4, assets-pipeline 3, dialogue-manager 2, brainstorming 1,
  csharp-godot 1, gdextension 1, multithreading 1. Full output: run the script on the 18 dirs.
  The grill and brainstorming text I changed has no new gdscript block.
- `skills_catalog`: 22 of 22 pass.

## Not proved, left as is

- None found.
