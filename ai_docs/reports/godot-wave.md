# A Godot wave for the fleet

Which skills and teammates to add so a horch fleet can staff Godot 4 work.
Two sources are merged into one catalog. Where both repos cover a topic, the
fleet gets **one** combined skill, not two. It follows the shape of
[`unreal-engine-wave.md`](unreal-engine-wave.md).

## Operator decisions (2026-10-04)

These override any text below that says otherwise.

1. **LGPL: rewrite.** No text, code or script from gd-agentic-skills is
   copied into this repository. Workers read it as a source of facts and
   techniques and write every reference and every code block in their own
   words. Mark such content "own text, consulted
   thedivergentai/gd-agentic-skills@4c4d0ff". Every bundle stays MIT (the
   GodotPrompter base) or own text. Where the text below says "folded in",
   "inlined" or "copied" for gd-agentic content, read "rewritten".
2. **Genres: ship all 27** in `godot-genre-blueprints`.
3. **Version migration: support the latest only.** Target Godot 4.7 (4.7.2
   is the latest stable). Do not ship per-version migration tables for old
   engines. Research the 460 KB of migration notes, and carry forward into the
   4.7 skills only what is still true and useful on 4.7 (for example a
   pattern that replaced a 3.x idiom, stated as the 4.7 way). Verify every
   claim against the 4.7.2 `--doctool` dump and the official docs. Do not act
   on assumptions.
4. **Re-vendoring: freeze** at GodotPrompter v1.14.0. We copy to extend it
   ourselves. The rename script still exists, so the rename is reviewable
   and repeatable, but no re-vendor process is planned.

Sources, read-only clones at the pinned revisions:
`.worktrees/_scratch/godot-src/GodotPrompter` and
`.worktrees/_scratch/godot-src/gd-agentic-skills` (git-ignored).

| source | revision | licence | content |
|---|---|---|---|
| [jame581/GodotPrompter](https://github.com/jame581/GodotPrompter) | `3e8d0f005f9604e1dbdad3de693e39555384c5af` (v1.14.0, 2026-09-20) | MIT | 56 skills (2.0 MB, `.md` only), 9 Claude agents, a SessionStart hook |
| [thedivergentai/gd-agentic-skills](https://github.com/thedivergentai/gd-agentic-skills) | `4c4d0ff5c4597938cc9257d99d9e35f7692c9c06` (2026-09-09) | **LGPL-3.0** | 99 skills (17 MB: 2,418 `.gd`, 1,100 `.md`, 70 `.py`), 3 persona skills |

## Summary

- **64 `godot-*` skills** in one catalog:
  - **36 combined skills.** GodotPrompter's skill is the base. The gd-agentic
    skills on the same topic are rewritten as references (53 of its 99
    skills).
  - **18 GodotPrompter skills** with no gd-agentic counterpart.
  - **7 new bundles** made only from gd-agentic facts, in own text. They
    cover genres, gameplay loops, combat, economy, quests, porting and
    version migration (42 skills).
  - **3 skills of our own:** project context, build-verify and scene files.
  - **Dropped:** 2 GodotPrompter skills and 4 gd-agentic skills. See
    [Excluded](#excluded).
- **19 focused teammates in three waves.**
  - Each teammate loads 5 to 10 expected skills and names 3 more as available.
  - The groups overlap on purpose. For example, `godot-physics-system`
    belongs to 5 seats, and `godot-state-machine` to 3.
  - No teammate carries the whole catalog. GodotPrompter's own
    `godot-game-dev` agent points at 39 skills, and that breadth is the
    problem these seats solve.
  - Every teammate sets `offer_when: ["project.godot"]`, so a non-Godot
    project pays nothing.
- **Three facts about headless Godot are new.** Measured on the operator's
  Godot 4.7.2; they change how a fleet worker proves `DONE:`. See
  [Measured](#measured-on-godot-472).

## Measured on Godot 4.7.2

The operator's Mac has `/Applications/Godot.app` 4.7.2.stable.official, and
`godot` is not on PATH. 4.7.2 (2026-08-18) is the latest stable release.
4.8 is at dev7, with no beta.

| probe | result | consequence |
|---|---|---|
| `--headless --path . --check-only --script res://bad.gd` on a parse error | prints `SCRIPT ERROR: Parse Error`, **exits 0** | A worker cannot gate on the exit code of `--check-only`. |
| a `SceneTree` script that `load()`s each `.gd` and calls `quit(1)` on a null or uninstantiable script | **exits 1** and names the file | This is the project-wide parse check. `godot-build-verify` ships it inline. |
| `--headless --path . --import` on a fresh tree | exits 0; creates `.godot/`; **writes a `.uid` sidecar next to every script** | A fresh worktree must import first. A commit must include the new `.uid` files (pathspec commits in the shared checkout). |
| `--doctool <dir>` | dumps 1,079 class XML files for the exact engine build | This is the Godot counterpart of "grep the engine headers". It gives a machine check for API names. |

**API spot check.** Every `Class.method(` call on a real engine class from
both repos, looked up in the 4.7.2 `--doctool` dump (with inheritance,
setters and getters; constructors and `godot-master` left out):

| repo | distinct engine calls | not in 4.7.2 | examples |
|---|---|---|---|
| GodotPrompter | 143 | 2 (1.4%) | `OS.gc` (1 reference file); `Signal.any` is correctly described as non-existent |
| gd-agentic-skills | 376 | 20 (5.3%) | `OS.get_ticks_msec`, `Array.join`, `ThemeDB.set_project_theme`, `NavigationServer3D.agent_set_navigation_layers`, `Vector3.random_on_unit_sphere` |

GodotPrompter is the more reliable base. Every fact taken from gd-agentic must
be checked against `--doctool` before it lands. Do the same check for the
merged result. It belongs in the gate.

## Skills

### Naming

Every bundle gets the `godot-` prefix. The bundles that already have it keep
their names, for example `godot-testing` and `godot-ui`. Plain upstream names
such as `state-machine`, `event-bus` and `save-load` are too generic for a
flat catalog that already holds `ue-*`. The rename changes the `name:` field,
the folder, and the inline cross-references (`godot-prompter:<x>` and bold
`**<x>**` in each "Related skills" line). Do the rename with a script.
Record it in `provenance.json` as `adaptation: "rename to godot-* prefix"`.

### Combined skills (36)

The SKILL.md is GodotPrompter's file, renamed and otherwise unchanged. The
description is extended (≤1024 B) to name the added coverage. The gd-agentic
topics become `references/<topic>.md` in own text (decision 1), with own code
blocks, because a bundle holds text only (precedent: `app-store-changelog`).
Only what GodotPrompter lacks is added. When the two repos disagree, the
`--doctool` check decides.

| combined skill | GodotPrompter base | gd-agentic consulted |
|---|---|---|
| `godot-2d-essentials` | 2d-essentials | tilemap-mastery |
| `godot-3d-essentials` | 3d-essentials | 3d-lighting, 3d-materials, 3d-world-building (GridMap, CSG) |
| `godot-ability-system` | ability-system | ability-system, rpg-stats |
| `godot-ai-navigation` | ai-navigation | ai-navigation, navigation-pathfinding |
| `godot-animation-system` | animation-system | 2d-animation, animation-player, animation-tree-mastery |
| `godot-audio-system` | audio-system | audio-systems |
| `godot-camera-system` | camera-system | camera-systems |
| `godot-component-system` | component-system | composition, composition-apps |
| `godot-debugging` | godot-debugging | debugging-profiling (debugging half) |
| `godot-optimization` | godot-optimization | performance-optimization, debugging-profiling (profiling half) |
| `godot-dialogue-system` | dialogue-system | dialogue-system |
| `godot-event-bus` | event-bus | signal-architecture |
| `godot-dependency-injection` | dependency-injection | autoload-architecture |
| `godot-export-pipeline` | export-pipeline | export-builds, platform-desktop, platform-web, platform-console, adapt-mobile-to-desktop |
| `godot-mobile-development` | mobile-development | platform-mobile, adapt-desktop-to-mobile |
| `godot-xr-development` | xr-development | platform-vr |
| `godot-gdscript-advanced` | gdscript-advanced | gdscript-mastery |
| `godot-input-handling` | input-handling | input-handling |
| `godot-inventory-system` | inventory-system | inventory-system |
| `godot-multiplayer-basics` | multiplayer-basics | adapt-single-to-multiplayer |
| `godot-multiplayer-sync` | multiplayer-sync | multiplayer-networking (rollback, interest culling) |
| `godot-dedicated-server` | dedicated-server | server-architecture |
| `godot-particles-vfx` | particles-vfx | particles |
| `godot-physics-system` | physics-system | 2d-physics, physics-3d, raycasting-queries |
| `godot-player-controller` | player-controller | characterbody-2d |
| `godot-procedural-generation` | procedural-generation | procedural-generation |
| `godot-project-setup` | godot-project-setup | project-foundations |
| `godot-resource-pattern` | resource-pattern | resource-data-patterns |
| `godot-save-load` | save-load | save-load-systems |
| `godot-scene-organization` | scene-organization | scene-management (async loading, transitions) |
| `godot-shader-basics` | shader-basics | shaders-basics |
| `godot-state-machine` | state-machine | state-machine-advanced (HSM, pushdown) |
| `godot-testing` | godot-testing | testing-patterns |
| `godot-tween-animation` | tween-animation | tweening |
| `godot-ui` | godot-ui | ui-theming, ui-rich-text, ui-containers |
| `godot-code-review` | godot-code-review | auditor (never-lists), analyst (project scoring). The "Aurelius" and "Anara" voices are dropped. |

### GodotPrompter only (18)

Vendored and renamed, with no other change:

`godot-gdscript-patterns`, `godot-localization`, `godot-hud-system`,
`godot-responsive-ui`, `godot-multithreading`, `godot-gdextension`,
`godot-addon-development`, `godot-csharp-godot`, `godot-csharp-signals`,
`godot-math-essentials` and `godot-assets-pipeline`.

The five addon skills are pinned to their addon versions: `godot-limboai`
(v1.8.0), `godot-beehave` (v2.9.2), `godot-popochiu` (v2.1.1),
`godot-dialogue-manager` (v3.10.4) and `godot-phantom-camera` (v0.11.0.2).

Two more are **adapted**, because they ask a human:

- **`godot-grill`.** It asks "the user" for decisions in rounds, with a
  recommended answer for each question. Keep the seeded dependency tree and
  the decision-versus-fact rule. Replace the rounds with this: write the
  decision record with each recommendation filled in as `[proposed]`, then
  send **one** `QUESTION:` to the orchestrator that lists the open decisions,
  scope first. A `[proposed]` answer is valid until the orchestrator
  overrides it.
- **`godot-brainstorming`.**
  - Drop the step that offers to inject an agent-instructions section.
  - Drop the `~/.godot-prompter/state` file.
  - Replace the `superpowers:writing-plans` reference with `create-plan`.
  - Replace "does this look right?" after each section with this: write the
    whole design, then send one `QUESTION:` that lists the open items.

### gd-agentic only: 7 new bundles (42 skills consulted, own text)

| bundle | built from | shape |
|---|---|---|
| `godot-genre-blueprints` | the 27 `genre-*` skills plus project-templates | A router SKILL.md (pick a genre, then read one reference) with one `references/<genre>.md` each. All 27 genres (decision 2). |
| `godot-gameplay-loops` | game-loop-collection, -harvest, -time-trial, -waves; mechanic-revival, mechanic-secrets | A router and 6 references |
| `godot-combat-system` | combat-system, turn-system | Real-time and turn-based. Hitbox/hurtbox points to `godot-component-system` and is not repeated. |
| `godot-economy-system` | economy-system | |
| `godot-quest-system` | quest-system | |
| `godot-dimension-port` | adapt-2d-to-3d, adapt-3d-to-2d | |
| `godot-version-migration` | version-migration | Decision 3: 4.7 only; carry forward what is still true. |

### Our own (3)

Neither repo has the loop a fleet worker needs: change the project, prove the
change, report.

1. **`godot-project-context`**, the counterpart of `ue-project-context`.
   - It scans `project.godot` for: the version in `config/features`, and C#
     when it is there; the rendering method; the autoloads and the input map;
     the physics engine (Jolt or Godot Physics); the stretch mode.
   - It also records: each addon in `addons/*/plugin.cfg`, with its version;
     the test framework (GUT or gdUnit4); `*.csproj` and
     `export_presets.cfg`; the `godot-grill` decision record, when one exists.
   - It writes `.agents/godot-project-context.md`, with `[unknown]` for each
     field it cannot fill, and sends one `QUESTION:`.
2. **`godot-build-verify`.** Every builder and QA seat carries it.
   - It finds the engine: `GODOT_PATH`, then `godot` on PATH, then the macOS
     app bundle.
   - It refuses to run when the project's `config/features` version is newer
     than the engine.
   - It runs `--headless --import` when `.godot/` is missing.
   - It runs the inline project-wide parse checker. `--check-only` exits 0 on
     errors.
   - It runs `dotnet build` on a C# project.
   - It runs the matching tests with GUT (`gut_cmdln.gd ... -gexit`) or
     gdUnit4 (`GdUnitCmdTool.gd ... --ignoreHeadlessMode`).
   - It smoke-runs the main scene with `--quit-after`.
   - It greps the output for `SCRIPT ERROR`, `Parse Error` and `ERROR:`.

   Fleet rules: run long imports in the background; one Godot import at a
   time per working copy (`.godot/` is shared); never launch the editor GUI;
   if the operator's editor has the project open, report `BLOCKED:`, because
   the editor rewrites scenes it has open.

   To verify on 4.7.2 before shipping (from gd-agentic `godot-builder`):
   per-instance `XDG_DATA_HOME` and `XDG_CONFIG_HOME` for concurrent
   instances (Linux; unverified on macOS); set `owner` on generated nodes, or
   they are not saved; a `ResourceSaver` save in headless mode is followed by
   `--import`, so that UIDs are written.

   The gdUnit4 exit codes are 0 (pass), 1 (fail) and 2 (tool error). Watch
   for exit 101 on orphan nodes (gdUnit4 issue #1332).
3. **`godot-scene-files`.** Editing `.tscn` and `.tres` as text. Rules: keep
   `ext_resource` and `sub_resource` ids consistent; never invent a `uid://`
   (omit it and let the import write it); prefer a headless `SceneTree` script
   that builds the scene and saves it with `ResourceSaver` for anything bigger
   than a property change; prove each changed scene with a headless `load()`;
   never edit `.godot/`, `.import` files, or binary `.res` and `.scn` files;
   commit the `.uid` sidecars.

### Excluded

| skill | why |
|---|---|
| GodotPrompter `using-godot-prompter` | The bootstrap and per-harness tool mapping. The horch briefing does this job. |
| GodotPrompter `godot-mentor` | A teaching mode for a human learner. No human reads a fleet pane. |
| gd-agentic `godot-master` | An 8.4 MB hub that repeats the other 92 skills. |
| gd-agentic `godot-agent-vision` | A Python screenshot CLI and a temporary editor-viewport bridge. Reconsider with the MCP decision below. |
| gd-agentic `godot-monte-carlo-balancer` | Builds a Rust and rayon lab; needs a toolchain and crates from the network. |
| gd-agentic `godot-theme-easter` | A seasonal overlay; too narrow. |

Also not carried: GodotPrompter's SessionStart hook and its 9 agent files
(the personas below replace the agents, and their domain boundaries are
kept); gd-agentic's `AGENT.md`, `SOUL.md` and `IDENTITY.md`.

## Teammates

Every teammate uses `base: fleet-worker`, `agent: claude`,
`permission_mode: auto`, `inherit_plugins: false`, `mcp_servers: {}`,
`disallowed_tools: [Agent]`,
`disabled_skills: [herdr-orchestrator, herdr-worker]`,
`offer_when: ["project.godot"]` and `requires: [godot]`. Model and effort
follow the role table in `teammates/README.md`, the same as the UE wave.

Fallbacks: builders and QA have none (Godot import writes outside the
project, to the user data directory, and Codex runs with the network off and
`workspace-write`; untried). The plan and review seats fall back to
`codex-sol`.

### Wave 1 (7)

| teammate | phase | model / effort | expected skills | available |
|---|---|---|---|---|
| `godot-tech-lead` | plan | opus / high | project-context, grill, brainstorming, scene-organization, project-setup, genre-blueprints, event-bus, dependency-injection | component-system, resource-pattern, version-migration |
| `godot-gameplay-programmer` | implementation | opus / medium | gdscript-patterns, player-controller, state-machine, component-system, combat-system, gameplay-loops, input-handling, physics-system, build-verify, scene-files | animation-system, camera-system, math-essentials |
| `godot-systems-programmer` | implementation | opus / medium | gdscript-patterns, resource-pattern, ability-system, inventory-system, economy-system, quest-system, save-load, event-bus, build-verify, scene-files | component-system, hud-system, dialogue-system |
| `godot-ui-developer` | implementation | opus / medium | ui, responsive-ui, hud-system, tween-animation, localization, input-handling, build-verify, scene-files | inventory-system, ability-system, dialogue-system |
| `godot-technical-artist` | implementation | opus / medium | shader-basics, particles-vfx, 2d-essentials, 3d-essentials, assets-pipeline, audio-system, build-verify, scene-files | animation-system, tween-animation, optimization |
| `godot-qa-engineer` | validation | sonnet / high | testing, debugging, build-verify (+ `check`, `debug`, `tdd`) | dependency-injection, gdscript-advanced, optimization |
| `godot-code-reviewer` | validation | opus / high | code-review (godot), gdscript-patterns, gdscript-advanced, scene-organization, multithreading (+ `code-review`) | csharp-godot, multiplayer-sync, optimization |

Names drop the `godot-` prefix for width. `godot-code-reviewer` copies
`ue-code-reviewer`'s read-only setup:
`disallowed_tools: [Agent, Edit, Write, NotebookEdit]`.

### Wave 2 (7)

| teammate | phase | model / effort | expected skills | available |
|---|---|---|---|---|
| `godot-animator` | implementation | opus / medium | animation-system, tween-animation, state-machine, 2d-essentials, 3d-essentials, build-verify, scene-files | player-controller, shader-basics, camera-system |
| `godot-ai-programmer` | implementation | opus / medium | ai-navigation, state-machine, limboai, beehave, math-essentials, build-verify, scene-files | physics-system, component-system, multithreading |
| `godot-network-engineer` | implementation | opus / medium | multiplayer-basics, multiplayer-sync, dedicated-server, physics-system, input-handling, build-verify, scene-files | state-machine, export-pipeline, event-bus |
| `godot-world-builder` | implementation | opus / medium | 2d-essentials, 3d-essentials, procedural-generation, physics-system, camera-system, phantom-camera, build-verify, scene-files | math-essentials, optimization, multithreading |
| `godot-narrative-programmer` | implementation | opus / medium | dialogue-system, dialogue-manager, popochiu, localization, quest-system, save-load, inventory-system, build-verify, scene-files | ui, state-machine, resource-pattern |
| `godot-performance-engineer` | implementation | opus / high | optimization, multithreading, gdscript-advanced, debugging, physics-system, assets-pipeline, build-verify | gdextension, shader-basics, particles-vfx |
| `godot-release-engineer` | implementation | sonnet / medium | export-pipeline, mobile-development, assets-pipeline, responsive-ui, build-verify | dedicated-server, optimization, testing |

`godot-release-engineer` builds export artifacts and never uploads them (no
`butler push`, no Steam or store upload). Like `app-release-preparer`, it ends
at a plan for the orchestrator.

### Wave 3 (5)

| teammate | phase | model / effort | expected skills | available |
|---|---|---|---|---|
| `godot-csharp-engineer` | implementation | opus / medium | csharp-godot, csharp-signals, testing, multithreading, build-verify, scene-files | gdextension, event-bus, save-load |
| `godot-tools-engineer` | implementation | opus / medium | addon-development, gdscript-advanced, ui, resource-pattern, testing, build-verify, scene-files | csharp-godot, gdextension, export-pipeline |
| `godot-native-engineer` | implementation | opus / medium | gdextension, multithreading, export-pipeline, optimization, build-verify | csharp-godot, addon-development, math-essentials |
| `godot-xr-developer` | implementation | opus / medium | xr-development, input-handling, physics-system, 3d-essentials, optimization, build-verify, scene-files | mobile-development, export-pipeline, animation-system |
| `godot-porting-engineer` | implementation | opus / medium | version-migration, dimension-port, multiplayer-basics, mobile-development, gdscript-advanced, build-verify, scene-files | export-pipeline, testing, input-handling |

### C# is a language, not a domain

Every GodotPrompter skill has GDScript and C# examples. A C# project needs
`godot-csharp-godot` on every builder. Add a `skills_when:` field, so that a
`*.csproj` adds `godot-csharp-godot` and `godot-csharp-signals` to each
builder. Until it exists, each builder's persona says "if the project context
says C#, read `godot-csharp-godot` first". `godot-csharp-engineer` remains the
seat for interop, .NET builds and source-generator problems.

### What every persona must say

These are role rules, so they go in each persona body, not in
`_base/fleet-worker.md`:

1. **Read `.agents/godot-project-context.md` first.** If it is missing, send
   `QUESTION:` asking for `godot-tech-lead`. Do not write it from a guess.
2. **Scene and resource files are text, but treat them with care.** Follow
   `godot-scene-files`. Never touch `.godot/` or `.import` files. Commit the
   `.uid` sidecars with their scripts. List binary assets (art, audio) that a
   human must change in `DONE:`.
3. **Check APIs against the project's engine, not from memory.** Use
   `godot --doctool` for the exact build. The skills target 4.7. If the
   project is older, say which advice may not apply.
4. **Headless only, and one import at a time per working copy.** Never open
   the editor GUI. If the operator's editor has the project open, report
   `BLOCKED:`.
5. **"It parses" is not done.** A `DONE:` names the Godot version, the parse
   check, and the tests run with their result. A C# project also needs the
   `dotnet build` result.
6. **Addons are pinned.** Use an addon skill only when that addon is in
   `addons/` at a matching version. Otherwise say which advice may not apply.

## Code changes

1. **`Requirement::Godot`** in `roster/offer.rs`, plus a `horch doctor`
   check. Mirror `Blender`: `GODOT_PATH`, then `godot` on PATH, then
   `/Applications/Godot.app/Contents/MacOS/Godot`; `--version` must be 4.3
   or later.
2. **Size budget for combined skills.** An adapted skill must keep SKILL.md
   at or under 12 KB, but 31 GodotPrompter SKILL.md files are 12 to 15.7 KB.
   Extend the exemption to a skill whose SKILL.md is an upstream copy changed
   only by the rename and the description, and whose additions are only under
   `references/`.
3. **`skills_when: {"<glob>": [skills]}`.** Adds skills from project facts,
   the same facts that `offer_when` reads. Needed for C#, and later for addon
   skills (`addons/limboai`).
4. **Gate: the API name check** against a `--doctool` dump, for every
   `godot-*` bundle. Skip it on a host without Godot.
5. **No change** to `offer_when`.

## Harness notes

- **No MCP server in wave 1.** The shell gives a worker everything `DONE:`
  needs.
- Later, for visual QA: [Erodenn/godot-mcp-runtime](https://github.com/Erodenn/godot-mcp-runtime)
  (MIT, no editor or addon; young, pin it and read its source first);
  fallback [Coding-Solo/godot-mcp](https://github.com/Coding-Solo/godot-mcp)
  0.1.1 (deny `launch_editor`). Avoid editor-addon servers: `godot-ai`
  (opt-out telemetry), GDAI, `ee0pdt`.
- **Codex.** Try one Codex fallback on a real project before adding any to a
  builder.
