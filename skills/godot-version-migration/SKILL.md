---
name: godot-version-migration
description: "Use when a Godot project must move to Godot 4.7 (4.7.2 is the latest stable): a Godot 3.x project through the built-in 3to4 converter, or a 4.0-4.6 project opened in 4.7. Covers version detection from project.godot, the headless converter run, the manual fixes the converter leaves, the 4.x changes that break GDScript or change behaviour up to 4.7, the deprecated APIs and their 4.7 replacements, a headless TileMap to TileMapLayer converter and a headless resave of every scene. Targets 4.7 only; it does not support stepping an old project through each older engine. Every claim is checked against the 4.7.2 --doctool dump and the official upgrade guides."
---

# Godot version migration (target: 4.7)

This skill moves a project to **Godot 4.7** and nothing else. The engine on
the fleet host is 4.7.2. The skill does not keep per-version tables for old
engines: every change is stated as "the 4.7 way".

Read first: `godot-build-verify` (finding the engine, the parse check) and
`godot-scene-files` (text edits of `.tscn` / `.tres`).

## References

| file | read when |
|---|---|
| [references/port-3x-to-4.md](references/port-3x-to-4.md) | The project is Godot 3.x (`config_version=4`). |
| [references/upgrade-4x-to-4.7.md](references/upgrade-4x-to-4.7.md) | The project is 4.0 to 4.6, and after a 3.x port. |
| [references/deprecated-in-4.7.md](references/deprecated-in-4.7.md) | Always, as the last scan: old API that still runs on 4.7 but is deprecated. |
| [references/headless-tools.md](references/headless-tools.md) | You need the resave script or the TileMap converter. |

## Step 1: detect the version

Read `project.godot` at the project root.

| `project.godot` says | project is | route |
|---|---|---|
| `config_version=5` and `config/features=PackedStringArray("4.N", ...)` | Godot 4.N | Step 3 |
| `config_version=5`, no `config/features` | Godot 4.x, minor unknown | Step 3; read every hop in `upgrade-4x-to-4.7.md` |
| `config_version=4` | Godot 3.x | Step 2, then Step 3 from 4.0 |
| no `project.godot`, or `config_version` below 4 | older than 3.0 | Stop. Send `BLOCKED:` (the converter takes 3.0 and later only). |

If `config/features` names a version **newer** than the engine (4.8 or later),
stop and send `BLOCKED:`. Never open a project with an older engine than the
one that saved it.

## Before you change anything

- The tree must be committed. The 3to4 converter makes no backup.
- The operator's editor must not have the project open. The editor rewrites
  open scenes. If it is open, send `BLOCKED:`.
- Headless only. Never start the editor GUI.
- One Godot import at a time in one working copy.

## Step 2: Godot 3.x to 4.x (converter)

Follow [references/port-3x-to-4.md](references/port-3x-to-4.md). In short:

```bash
G="${GODOT_PATH:-/Applications/Godot.app/Contents/MacOS/Godot}"
"$G" --headless --path . --validate-conversion-3to4 > ai_docs/port-3to4-plan.txt 2>&1
"$G" --headless --path . --convert-3to4
```

Measured on 4.7.2 with a small 3.x project: both commands exit 0; the
validate run prints every rename it plans; the convert run sets
`config_version=5` and changes `.gd`, `.tscn` and `project.godot` in place.
A `PCRE2 Error: unknown substring` line can appear and does not stop the run.

The converter leaves work for you. Each script that still fails the parse
check needs the manual fixes in the reference (examples measured on 4.7.2:
`update()` and `File.new()` stay as they were and fail to parse).

## Step 3: open in 4.7 and apply the 4.x changes

1. Import once: `"$G" --headless --path . --import`. This creates `.godot/`
   and writes a `.uid` file next to each script and shader (measured on 4.7.2). Commit the `.uid`
   files with the scripts.
2. Read [references/upgrade-4x-to-4.7.md](references/upgrade-4x-to-4.7.md)
   from the hop after the project's version up to 4.6 → 4.7. The changes add
   up, so read every hop in between. One engine (4.7.2) is enough; you do not
   install 4.1, 4.2 and so on.
3. Scan the project for the old names. The scan is a list of hits for you to
   read, not a fix:

```bash
rg -n -g '*.gd' -e '\bupdate\(\)' -e 'File\.new|Directory\.new' \
   -e '\byield\(' -e 'setget' -e '^tool$' -e '\.instance\(\)' -e 'OS\.get_ticks' \
   -e 'change_scene\(' -e 'connect\("[^"]+", *self' -e 'get_rpc_config' \
   -e 'type_exists\(' -e 'TileMap\b' -e 'duplicate\(true\)' -e 'device == 0' .
```

4. Run the parse check from `godot-build-verify` on every script. Fix until it
   passes. Never trust the exit code of `--check-only`; it exits 0 on errors.
5. Resave every scene and resource in the 4.7 format with the headless resave
   script in [references/headless-tools.md](references/headless-tools.md).
   This is the headless form of the editor's **Project > Tools > Upgrade
   Project Files**, which has no command-line flag. Commit the resave alone,
   so its diff does not hide real changes.
6. Save the project settings once so `config/features` says `4.7`
   (`ProjectSettings.save()` from a headless script writes it; measured on
   4.7.2, a `"4.3"` entry became `"4.7"` and other features stayed).
7. Run the tests and a smoke run of the main scene (`godot-build-verify`).

## Things a headless worker cannot do

Send each of these as one `QUESTION:` or `BLOCKED:` line. Do not guess.

- **Mesh format upgrade (projects from 4.0 or 4.1).** 4.2 changed the
  `Mesh` format. The editor offers "Restart & Upgrade" or "Upgrade Only", or
  **Project > Tools > Upgrade Mesh Surfaces**. All are GUI. Ask the
  orchestrator for an operator step.
- **Visual checks.** Glow, fog, decals, reverse-Z shaders and 2D line widths
  changed (see the 4.x reference). Under `--headless` the renderer is a dummy:
  no frame is drawn (measured: `RenderingServer.frame_post_draw` never fires,
  `get_image()` on a viewport texture returns null). List the scenes that need
  a human look in `DONE:`.
- **Pre-3.0 projects.** No tool exists. Send `BLOCKED:`.

## Rules that stay true on 4.7

- High-level multiplayer works only between peers on the same Godot version.
  Upgrade server and clients together (stated in the 4.3 upgrade guide).
- A mesh saved by 4.2+ and a resource with a large `PackedByteArray` saved
  by 4.3+ may not open in an older editor. After the upgrade, version control
  is the only way back. (4.6 scene files stay readable by 4.5.)
- C# (.NET) projects: the official upgrade pages mark each change "C# source
  compatible" or not. Run `dotnet build` after the upgrade and fix each error
  against those columns. Read `godot-csharp-godot`.
- Save files: an engine upgrade is not a save-format change. Keep the
  project's own save version separate (`godot-save-load`).

## DONE

A `DONE:` for a migration names: the source version, `config/features` now,
the parse check result (files, failures), the tests run and their result,
the scenes resaved, and the human checks still open (mesh upgrade, visuals).

## Related skills

`godot-build-verify`, `godot-scene-files`, `godot-project-context`,
`godot-gdscript-patterns`, `godot-csharp-godot`, `godot-2d-essentials`
(TileMapLayer), `godot-dimension-port`.
