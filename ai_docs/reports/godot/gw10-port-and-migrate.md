# GW10 godot-port-and-migrate: report

Unit plan: `ai_docs/plans/godot/gw10-port-and-migrate.md`. Worker: opus-83.

## What shipped

Two own-text skills (`vendored: false`, `sources: []`):

| skill | files | SKILL.md |
|---|---|---|
| `godot-version-migration` | `SKILL.md`, `references/port-3x-to-4.md`, `references/upgrade-4x-to-4.7.md`, `references/deprecated-in-4.7.md`, `references/headless-tools.md` | 7.3 KB |
| `godot-dimension-port` | `SKILL.md`, `references/2d-to-3d.md`, `references/3d-to-2d.md` | 8.0 KB |

Target: Godot 4.7 only (operator decision 3). No per-version table for old
engines. Each kept change is stated as "the 4.7 way".

## Sources consulted

- gd-agentic-skills @ `4c4d0ff5c4597938cc9257d99d9e35f7692c9c06` (LGPL-3.0,
  read only, nothing copied): `skills/godot-version-migration/` (SKILL.md,
  `references/` with 95 per-topic mirrors, `era-index.md`, `hop-index.md`,
  `bridges/3-to-4.md`, `legacy/`), `skills/godot-adapt-2d-to-3d/`,
  `skills/godot-adapt-3d-to-2d/` (SKILL.md, references, scripts).
- Official Godot docs, `godotengine/godot-docs` branch `4.7` at
  `9adca4c1c72917bfe1b7be3108abed5ce26696a6`:
  `tutorials/migrating/upgrading_to_godot_4.rst` and `..._4.1.rst` to
  `..._4.7.rst`; `classes/class_*.rst` (the "Deprecated:" notes);
  `tutorials/best_practices/version_control_systems.rst`.
- Godot 4.7.2.stable.official.ed1daf0bf: `--doctool` dump (1,076 XML files),
  `--help`, and headless experiments (below).
- GodotPrompter v1.14.0 @ `3e8d0f0`: read to find what the combined skills
  already cover (for the proposals). Nothing copied.

## Research notes: version migration

The gd-agentic corpus is 352 KB in 98 files. Most of it is the same short
list of official upgrade-guide rows, mirrored into 95 topic files (for
example the 4.0 → 4.1 `Object.get_meta_list` row is repeated in 20 files). I
de-duplicated it per hop (3.x → 4.0: 111 rows; 4.0 → 4.1: 233; 4.1 → 4.2:
119; 4.2 → 4.3: 290; 4.3 → 4.4: 186; 4.4 → 4.5: 220; 4.5 → 4.6: 250; 4.6 →
4.7: 346). Then I checked the remaining unique facts against the official
pages and the dump. I wrote the skill from the official pages, not from the
mirrors.

Kept, and where it went:

| kept | where |
|---|---|
| 3to4 converter run, CLI flags, size limits, 3.0+ only, backup rule | `port-3x-to-4.md` §1-2 (run headless and measured) |
| manual renames (methods, properties, signals, constants) | `port-3x-to-4.md` §3, each new name in the dump |
| behaviour changes with no rename (`super()`, setters, signals, Tween, `randomize`, `call_group`, `slice`, Camera2D zoom, ...) | `port-3x-to-4.md` §4 |
| removed nodes and replacements | `port-3x-to-4.md` §5 |
| shaders, rendering settings, 2D HDR, Bullet, ArrayMesh, VCS | `port-3x-to-4.md` §6 |
| per-hop GDScript breaks, behaviour changes, changed defaults 4.1-4.7 | `upgrade-4x-to-4.7.md` |
| mesh format upgrade (4.2), TileMapLayer (4.3), reverse Z (4.3), multiplayer same-version (4.3), Jolt / D3D12 defaults (4.6), scene unique IDs (4.6) | `upgrade-4x-to-4.7.md`, SKILL.md "cannot do" and "rules" |
| deprecated API and its replacement (not in gd-agentic; taken from the 4.7 class reference) | `deprecated-in-4.7.md` |
| "Upgrade Project Files" and "Extract TileMap layers" have no CLI | `headless-tools.md`: 2 own scripts, tested |

Dropped, and why:

| dropped | why |
|---|---|
| `legacy/pre-2-context.md`, `legacy/2-to-3.md` (1.x / 2.x) | Decision 3. The 4.7.2 converter takes 3.0+ only; the skill sends `BLOCKED:` for older projects. |
| "never skip a 4.x hop: install and open each minor in turn" | Not in the official docs. One 4.7.2 engine reads 4.0+ projects; the pages are cumulative. The skill reads every hop's notes instead. |
| the per-topic mirror structure and `sync_godot_version_migration.py` | Duplicates; the skill has one reference per route. |
| 4.6 → 4.7 "editor features" rows (Asset Store naming, 3D vertex snapping, Path3D snap-to-colliders, collapsed animation tracks, HDR output, Control offset transform, GradientTexture2D conic, TextureRect atlas tiling, AreaLight3D "prefer") | New features, not migration changes. Editor UI rows cannot be checked in the dump. Feature notes belong in the domain skills (GodotPrompter already has AreaLight3D and VirtualJoystick). |
| "Sky roughness_layers default restored toward 8" (4.7) and "default 7 (was 8)" (4.6) | Kept, but corrected: the official tables say 8 → 7 in 4.6 and 7 → 8 in 4.7; the dump default is 8. |
| "CollisionShape2D one-way collision direction is shape-relative" (4.7) | The official page lists only the new optional `direction` argument of `PhysicsServer2D.body_set_shape_as_one_way_collision`. `CollisionShape2D.one_way_collision_direction` exists in the dump, but the behaviour claim has no official source. Dropped. |
| genre and mechanic mirrors (`genre-*`, `game-loop-*`, `mechanic-*`, `theme-easter`, `monte-carlo-balancer`, `agent-vision`) | Generic rows copied from the engine pages; nothing genre-specific. |
| "Retune Environment glow/fog if the genre leans on bloom" (29 copies) | Kept once, as the official 4.6 glow and fog note. |
| `ImageUpdateMask.UPDATE_WIDTH_IN_PERCENT` details beyond the official row | Kept as the official row only. |

Official-doc rows that the 4.7.2 dump contradicts (the dump wins):

| official text | 4.7.2 dump | in the skill |
|---|---|---|
| 3→4: AStar `get_points()` → `get_points_id()` | `get_point_ids()`; no `get_points_id` | `get_point_ids()` |
| 3→4: `TextureProgressBar.percent_visible` → `show_percentage` | `show_percentage` is on `ProgressBar` only | says so; use a `Label` |
| 3→4: `InputEventWithModifiers.command` → `command_pressed` | no `command_pressed`; `meta_pressed` and `command_or_control_autoremap` exist | both named |
| 4.1: `NavigationObstacle2D/3D.get_rid()` → `get_agent_rid()` | `get_rid()` exists; `get_agent_rid()` does not | `get_rid()` |
| 3→4: `XRPositionalTracker.get_name()` → `get_tracker_name()` | the getter of the `XRTracker.name` property | the `name` property |

## Research notes: dimension port

Kept from gd-agentic (as facts, rewritten): separate 2D and 3D layer
tables; 1 unit = 1 m in 3D; a 3D scene needs a light and an environment;
SpringArm3D third-person rig; camera-relative movement; Sprite3D billboard
for 2.5D; 8-direction sprite choice from the camera angle; mouse ray picking;
UI over 3D with `unproject_position` and `is_position_behind`;
`get_global_transform_interpolated` / physics interpolation for jitter;
TileMapLayer → GridMap; simulated height in top-down 2D with a
ground/air hit check; Y-sort on the parent, not `z_index` per frame;
fliers through a second navigation layer; gravity scale by pixels per meter.

Fixed or dropped, and why:

| gd-agentic | problem | in the skill |
|---|---|---|
| "to move forward: `velocity = transform.basis.z * speed`" | forward is `-Z` | the mapping table and code use `-Z` |
| `isometric_math_core.gd` `iso_to_cartesian()` | not the inverse of its `cartesian_to_iso()` (for cart (1, 0): iso (1, 0.5) maps back to (1.25, -0.75)) | hand formulas dropped; use `TileMapLayer.map_to_local()` / `local_to_map()` with an isometric `TileSet` (round trip measured) |
| `light_migration_tool.gd` writes 2D pixel positions into 3D meters | wrong unit | text: `OmniLight3D` at the light's `height`, range in meters |
| `model_to_sprite_bake.gd` (`@tool` SubViewport bake) | cannot run under `--headless` (measured, below) | a plan plus a `QUESTION:` for an operator run |
| "Dimensional patcher" regex `Vector3(a,b,c)` → `Vector2(a,c)` | changes meaning silently (side views keep Y, not Z) | dropped |
| draw-call / vertex budget tables | not measured; gd-agentic itself warns against them | dropped; measure with `Performance` monitors |
| `massive_crowd_manager.gd` | builds an empty `ShaderMaterial` | one sentence: use `MultiMeshInstance3D` with a billboard material |
| `ortho Camera3D as 2D mode` (a NEVER rule) | kept | Step 1 of SKILL.md |

## Measured on Godot 4.7.2 (headless)

| probe | result | used in |
|---|---|---|
| `--validate-conversion-3to4` and `--convert-3to4` on a small 3.x project | both exit 0; renames as in `port-3x-to-4.md` §2; a node named `Sprite` is renamed `Sprite2D`; `update()` and `File.new()` stay and fail to parse; a `PCRE2 Error: unknown substring` line is printed | `port-3x-to-4.md` |
| converted script after the manual fixes | parses (load check) | `port-3x-to-4.md` |
| resave script (load + `ResourceSaver.save` each `.tscn` / `.tres`) | `format=2` scene saved as `format=3`; no `unique_id` added | `headless-tools.md` |
| `ProjectSettings.save()` from a headless script | writes `config/features` `"4.7"`; a `"4.3"` entry becomes `"4.7"`, `"Forward Plus"` stays | SKILL.md step 3 |
| `--import` | writes `.uid` next to `.gd` and `.gdshader` | SKILL.md step 3 |
| TileMap → TileMapLayer script on a 2-layer fixture | same cells, atlas coordinates, position; `unique_id` per node in the saved scene | `headless-tools.md` |
| `SubViewport` + `Camera3D`, 30 frames | `frame_post_draw` never fires; `get_image()` returns null (dummy texture storage) | both SKILL.md files |
| `PlaneMap` yaw mapping, 6 angles | round trip and forward vector match | dimension-port SKILL.md |
| `Dir8.view_index`, 5 camera positions | 0, 4, 6, 2, 1 as expected | `2d-to-3d.md` |
| `TilesToGrid.copy_layer` | mapped cell placed, unmapped cell counted and skipped | `2d-to-3d.md` |
| isometric `TileSet` 64 × 32 round trip, 4 cells | all return to the same cell | `3d-to-2d.md` |
| `HeightBody2D.step_height`, `spans_overlap` | peak 54.25 px, lands in 42 steps; overlap true / false as expected | `3d-to-2d.md` |
| `--help` | no CLI flag for Upgrade Project Files or Upgrade Mesh Surfaces | SKILL.md |

## Checks

- `scripts/godot/api_check.py` (85cc43c) on both skills: 8 files, 0 unknown
  names.
- `scripts/godot/gdscript_blocks_check.py` (ee48348) on both skills: 8 files,
  14 blocks, 14 parse (14 whole scripts), 0 fail.
- Prose check (api_check reads code blocks only): 159 `Class.member` names in
  the text. Every name the text calls removed or renamed is absent from the
  dump; every replacement is present. Enum types `RichTextLabel.ImageUnit`
  and `AccessibilityServer.AccessibilityLiveMode` checked in the XML.
- `cargo test -q -p horch-core --test skills_catalog`: see the commit NOTE.

## Proposals sent to the orchestrator (not edited)

Sent as one NOTE: 8 additions for `godot-resource-pattern`,
`godot-save-load`, `godot-gdscript-patterns`, `godot-multiplayer-basics`,
`godot-ai-navigation`, `godot-2d-essentials`, `godot-animation-system`,
`godot-scene-files`, each with its official source. Each one is missing from
GodotPrompter v1.14.0.

## Outside my scope

- `godot-physics-system` (GodotPrompter text) says Jolt is the default for
  new 3D projects since 4.4. The 4.6 upgrade guide says since 4.6
  (GH-105737). api_check cannot find this; reported to the orchestrator.
- The official 3→4 guide has the 3 wrong rows listed above. They are
  upstream doc bugs; no action in this repository.
