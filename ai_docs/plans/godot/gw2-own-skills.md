# GW2 godot-own-skills: project-context, build-verify, scene-files

Follow `ai_docs/plans/godot/00-godot-conventions.md`.

## GOAL

`godot-project-context`, `godot-build-verify` and `godot-scene-files` exist
as own text, each proven on Godot 4.7.2 in scratch projects, as described in
`ai_docs/reports/godot-wave.md` "Our own (3)".

## CONTEXT

- Counterparts: `skills/ue-project-context`, `skills/ue-build-verify`
  (read both; match their shape and their fleet rules).
- gd-agentic `godot-builder` may be read for facts only (own-text rule).
- Verify on 4.7.2, by running it, each "to verify" item in the report:
  per-instance `XDG_DATA_HOME`/`XDG_CONFIG_HOME` on macOS (does Godot honour
  them on macOS? where does it write user data and the editor settings
  during `--import`?), `owner` on generated nodes, `ResourceSaver` +
  `--import` writing UIDs, `--quit-after` on the main scene, and the exact
  output strings to grep.
- Test frameworks: download GUT and gdUnit4 at their latest releases that
  support 4.7 into `.worktrees/_scratch/godot-gw2/` (pin the tag and the
  commit in your report; no install outside the scratch dir). Run one
  passing and one failing test with each; record the exit codes (gdUnit4:
  0/1/2 and the 101 orphan case).
- C#: check whether `dotnet` is installed. If not, write the C# step from
  the docs and mark it "not run on this host" in the report.

## STEPS

1. Scratch projects: a blank 4.7.2 project, a GUT project, a gdUnit4 project,
   a project with an autoload, an input map and an addon `plugin.cfg`.
2. `godot-build-verify`: the engine lookup, the version refusal, import,
   the inline parse checker (its code in the SKILL.md or a
   `references/parse_check.gd`; prove it exits 1 on a bad script and 0 on a
   good one), tests, smoke run, the grep, and the fleet rules. Every command
   in it was run by you; the report lists the runs.
3. `godot-project-context`: the scan (fields from the report), the
   `.agents/godot-project-context.md` template with `[unknown]`, one
   `QUESTION:`. Run its scan steps on your scratch projects.
4. `godot-scene-files`: the rules, with a worked example: a property edit as
   text, and a headless `SceneTree` script that builds and saves a scene
   with `ResourceSaver`, both proven with a headless `load()`.
5. Checks: `api_check.py` and `gdscript_blocks_check.py` (GW0) on your 3
   skills. Lock, provenance (`vendored: false`, own text, the consulted
   sources), README rows, `skills_catalog`, commit, release.

## FILES

own: `skills/godot-project-context/`, `skills/godot-build-verify/`,
`skills/godot-scene-files/`, lock-held edits of the 2 index files,
`ai_docs/reports/godot/gw2-own-skills.md`.
