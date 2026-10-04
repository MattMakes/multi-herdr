# GW13 godot-integration: one catalog, linked and checked as a whole

Follow `ai_docs/plans/godot/00-godot-conventions.md` (commit skills only with
`.worktrees/godot-commit.sh`; it replaces a tracked skill directory whole, so
copy the committed skill to your unit dir, edit there, and commit it back).

## GOAL

The 64 `godot-*` skills read as one catalog: every cross-reference resolves,
the API check catches what it misses today, and one report lists what was
proved and what was not.

## STEPS

1. `scripts/godot/api_check.py` gaps, with tests (you own `scripts/godot/`):
   - a bare class name that does not exist in 4.7.2 (after `extends`, in a
     type annotation `: Foo`, `-> Foo`, `Foo.new()`, `is Foo`, `as Foo`);
     a project `class_name` defined in the same file or bundle is allowed;
   - members marked deprecated in the doctool XML (`deprecated="..."`):
     report them; strict (fail) in own text, report-only in upstream files
     (same rule as `gdscript_blocks_check --strict-own`). Known case:
     `NavigationPolygon.make_polygons_from_outlines`.
   Run both on all `skills/godot-*` and fix own-text findings; fix upstream
   findings you can prove as API or fact fixes (the standing rule).
2. `scripts/godot/xref_check.py` (+ test, + a line in the gate step in
   `scripts/phase-gate.sh`): every `godot-<name>` mentioned in any
   `skills/godot-*` file names an existing skill directory. Fix every broken
   reference (GW8 wrote genre references before some skills existed).
3. Links that should exist: the genre blueprints name `godot-gameplay-loops`
   where a genre's loop is one of its 6; the combat, economy and quest
   bundles and the combined skills point at each other where the reports say
   so. Read every `ai_docs/reports/godot/*.md` for "outside scope",
   "proposal" and "gotcha" items and close each, or list why not.
4. Wait until GW3 (opus-76) and GW5 (opus-78) finish (ask the orchestrator,
   or check `ls skills | grep -c '^godot-'` = 64 and `git status skills`
   clean) before you edit their skills.
5. `ai_docs/reports/godot/wave-final.md`: the catalog (64 skills, which
   kind), the checks and their final numbers, every claim the units could
   not prove on this host (shader compilation, UPnP, DTLS, WebSocket TLS,
   XR runtime, store SDKs, consoles, release-template behaviour, the gdUnit4
   C# path, print_debug in release, the dedicated-server template claim,
   ...) gathered from the unit reports, and the host facts (macOS Godot
   ignores XDG_*, writes logs and editor settings to the real home).

## FILES

own: `scripts/godot/`, `scripts/phase-gate.sh` (the Godot step only),
`skills/godot-*` (edits through godot-commit.sh, after their unit is done),
`ai_docs/reports/godot/gw13-integration.md`, `ai_docs/reports/godot/wave-final.md`.
do not touch: `teammates/` (GW12), any `.rs` file.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."` per commit, then
`horch done`.
