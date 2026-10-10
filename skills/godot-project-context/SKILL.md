---
name: godot-project-context
description: "Use when creating, refreshing or repairing .agents/godot-project-context.md, the project document every other godot-* skill and Godot teammate reads first. Scans project.godot (engine version, C#, renderer, autoloads, input map, physics engine, stretch mode), addons with their versions, the test framework (GUT or gdUnit4), .csproj files, export presets and the godot-grill decision record, writes [unknown] for every field the files cannot prove, and sends one QUESTION to the orchestrator. Targets Godot 4.7."
---

# Godot Project Context

Target engine: **Godot 4.7**. The scan was run on Godot 4.7.2.

This skill writes `.agents/godot-project-context.md`, the one file the other
Godot skills read to learn the engine version, the language, the renderer, the
autoloads, the input map, the physics engine, the addons and the test setup.
It reads project files only, and saves nothing to the project except that
file. In a fleet pane no one answers an interview. Every line it writes comes
from a file or from the orchestrator's reply to one `QUESTION:`, never from a
guess.

The default path is **scan first, then one question**.

## Step 1: Scan and draft

1. If `.agents/godot-project-context.md` exists, read it. Treat it as the
   previous draft; do not start over.
2. Run the scan script in `references/context-scan.md`. It is a headless
   `SceneTree` script that reads `project.godot`, `addons/*/plugin.cfg` and
   `export_presets.cfg` with `ConfigFile`, and prints one `key: value` line per
   fact. It needs no import and does not change the project. If there is no
   engine, read the files by hand with the table below.
3. Do the file checks the script does not do (table below, "by hand" rows).
4. Write `[unknown]` for every field no file proves. Never invent a genre, a
   target platform, a team rule or an architecture.
5. Write the whole draft to `.agents/godot-project-context.md`, then go to
   Step 2.

### What to scan

`project.godot` stores a key `a/b/c` as `b/c=` under the section `[a]`. For
example `physics/3d/physics_engine` is `3d/physics_engine=` under `[physics]`.
A key that is missing has its default value.

| Field | Where | Notes (measured on 4.7.2) |
|---|---|---|
| Engine version | `[application]` `config/features` | The first entry, for example `"4.7"`. It is the version the project was last saved with. |
| C# | `"C#"` in `config/features`, a `*.csproj` in the root, `[dotnet]` `project/assembly_name` | The `.csproj` `Sdk="Godot.NET.Sdk/<version>"` gives the exact engine version. |
| Renderer | `[rendering]` `renderer/rendering_method` | Missing means `forward_plus`. Values: `forward_plus`, `mobile`, `gl_compatibility`. `config/features` also names it (`"Forward Plus"`), but the editor keeps that entry when the setting changes: trust the setting. |
| Main scene | `[application]` `run/main_scene` | A `res://` path or a `uid://`. |
| Autoloads | every key in `[autoload]` | `Name="*res://path.gd"`. The `*` means the autoload is a global singleton. Order matters: Godot loads them top to bottom. |
| Input map | every key in `[input]` | Project actions only. The built-in `ui_*` actions appear here only when the project changed them. |
| Physics 3D | `[physics]` `3d/physics_engine` | Missing or `"DEFAULT"` means GodotPhysics3D on 4.7. Projects created in 4.6 or later write `"Jolt Physics"`. |
| Physics 2D | `[physics]` `2d/physics_engine` | Missing or `"DEFAULT"` means GodotPhysics2D. |
| Stretch | `[display]` `window/stretch/mode` and `window/stretch/aspect` | Missing means `disabled` and `keep`. |
| Addons | `addons/*/plugin.cfg` `[plugin]` `name`, `version` | Enabled when `[editor_plugins]` `enabled` lists its `plugin.cfg` path. A folder without `plugin.cfg` is a library or a GDExtension: record it by folder name. |
| Test framework | `addons/gut/` (GUT), `addons/gdUnit4/` (gdUnit4) | Record the version from its `plugin.cfg`. By hand: the test folder (`test/`, `tests/`) and a `.gutconfig.json`. |
| Export | `export_presets.cfg` `[preset.N]` `name`, `platform` | Godot keeps export passwords in `.godot/export_credentials.cfg` (from the Godot docs; not run). Never read or copy it. |
| GDExtension | by hand: `*.gdextension` files | Native code; `godot-gdextension` applies. |
| Decision record | by hand: the folder that `godot-grill` names, else `docs/decisions/*.md` | Link each record; copy no decision text. |
| Agent rules | by hand: `CLAUDE.md`, `AGENTS.md` in the root | Link them. |

## Step 2: Ask about the unknowns

Send **one** `QUESTION:` to the orchestrator
(`horch tell orchestrator "[<role>] QUESTION: ..."`). Finish the draft first.
List the highest-value `[unknown]` fields, in this order, and skip the fields
the files already proved:

1. **Scope.** Genre, 2D or 3D, target platforms and the minimum hardware.
2. **Architecture.** Who owns game state (an autoload, a scene, a resource),
   the event pattern (signals, an event-bus autoload), the save format.
3. **Conventions.** Folder layout, naming, static typing required or not,
   GDScript or C# for new code.
4. **Tests.** Where tests live and what must pass before `DONE:`.

If the orchestrator answers, fill those fields and keep the rest. If it does
not, `[unknown]` is a valid final answer. A field written as undecided is more
useful than a plausible invention.

## Step 3: Save and confirm

- Write `.agents/godot-project-context.md`; create `.agents/` when needed.
- Stamp the Godot version from the scan and the date at the top.
- On a refresh, keep the sections the scan and the orchestrator did not
  revisit. Update the date.
- Report to the orchestrator: the path, the count of `[unknown]` fields, and
  the version. If `config/features` is newer than 4.7, say that the skills
  target 4.7 and that some advice may not apply.

## Document template

```markdown
# Godot Project Context

*Godot: [4.7 from config/features] · Last updated: [YYYY-MM-DD] · Scanned by: [role]*

## Engine & project
**Name:** [config/name] · **Engine version:** [4.7] · **Language:** [GDScript / C# (assembly [name], Godot.NET.Sdk [version])]
**Type:** [unknown] · **2D/3D:** [unknown] · **Target platforms:** [from export presets, else unknown]
**Main scene:** [res://main.tscn]

## Rendering & display
**Renderer:** [forward_plus / mobile / gl_compatibility]
**Stretch:** mode [disabled / canvas_items / viewport], aspect [keep / expand / ...]

## Physics
**3D:** [Jolt Physics / GodotPhysics3D] · **2D:** [GodotPhysics2D]

## Autoloads
| Name | Path | Singleton |
|---|---|---|
| [Events] | [res://autoload/events.gd] | [yes] |

## Input map
[jump, move_left, ...] (project actions; changed ui_* actions listed separately)

## Addons
| Folder | Name | Version | Enabled |
|---|---|---|---|
| [gut] | [Gut] | [9.7.1] | [yes] |

## Tests
**Framework:** [GUT 9.7.1 / gdUnit4 6.2.1 / none] · **Folder:** [res://test] · **Must pass before DONE:** [unknown]

## Export
| Preset | Platform |
|---|---|
| [macOS] | [macOS] |

## Architecture
**State owner:** [unknown] · **Events:** [unknown] · **Saves:** [unknown]

## Conventions
**Folders:** [unknown] · **Naming:** [unknown] · **Static typing:** [unknown]

## Decisions
- [docs/decisions/YYYY-MM-DD-topic.md]: [title]

## Agent rules
- [CLAUDE.md / AGENTS.md]
```

## Common mistakes

- **Trusting `config/features` for the renderer.** The editor keeps
  `"Forward Plus"` there after the setting changes to `mobile`. Read
  `rendering/renderer/rendering_method`.
- **Assuming Jolt.** Jolt is the default only for projects created in 4.6 or
  later. A missing key means GodotPhysics3D on 4.7.
- **Listing `ui_*` actions as project actions.** They are built in.
- **Reading `project.godot` with a regex across sections.** Keys are relative
  to their section; read by section.
- **Running an import to scan.** The scan needs no `.godot/`; an import from
  this skill wastes minutes and can collide with a builder's import.
- **Sending several questions.** Send one `QUESTION:` that lists the unknowns.
- **Overwriting on a refresh.** Merge into the existing file.

## Related skills

- `godot-build-verify`: reads the engine version, C#, test framework and main
  scene.
- `godot-scene-files`: reads the autoloads and the main scene.
- `godot-grill`: writes the decision records this file links.
- `godot-csharp-godot`: applies when the language is C#.
- `godot-testing`: applies with the framework recorded here.
