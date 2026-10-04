# GW10 godot-port-and-migrate: dimension port and version migration (4.7 only)

Follow `ai_docs/plans/godot/00-godot-conventions.md`. Own text only.

## GOAL

1. `godot-dimension-port`: moving a project from 2D to 3D and from 3D to 2D,
   from gd-agentic adapt-2d-to-3d and adapt-3d-to-2d.
2. `godot-version-migration`, per operator decision 3: support the latest
   only (Godot 4.7; 4.7.2 is the latest stable). Not a per-version table.
   Research gd-agentic's version-migration skill (about 100 reference files,
   460 KB, 1.x to 4.7). From it, keep only what is true and useful on 4.7:
   - porting an older project to 4.7: the 3.x→4.x renames and behaviour
     changes a porter meets, and the 4.x→4.7 changes (each stated as "the
     4.7 way");
   - patterns that replaced an old idiom, stated as the current way, and
     pushed into the right combined skill only if that skill lacks it — send
     those as proposals to the orchestrator (with the target skill), do not
     edit other units' skills.
   Do not act on assumptions: every rename, removal and behaviour change you
   keep is checked against the 4.7.2 `--doctool` dump and the official
   Godot docs or release notes (cite the page or the commit in the
   reference). Drop what you cannot verify, and list it in the report.

## Method

Read all the sources first and write the research notes in your report (what
you kept, where it went, what you dropped and why). Then write the 2 skills
(SKILL.md ≤ 12 KB, references as needed), checks (`api_check.py`,
`gdscript_blocks_check.py`), lock, provenance (`sources: []`,
`vendored: false`, own text, consulted dirs), README rows, commit.

## FILES

own: `skills/godot-dimension-port/`, `skills/godot-version-migration/`,
lock-held edits of the 2 index files, `ai_docs/reports/godot/gw10-port-and-migrate.md`.
