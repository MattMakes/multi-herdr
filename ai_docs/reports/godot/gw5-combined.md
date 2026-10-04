# GW5 godot-combined: systems and worlds

Unit GW5 (worker opus-78) built 7 combined skills. Each one is the GodotPrompter v1.14.0 skill (MIT, `3e8d0f005f9604e1dbdad3de693e39555384c5af`), renamed by `scripts/godot/rename.py`, plus new `references/*.md` in own text and own code. The new references hold only what gd-agentic-skills (`4c4d0ff5c4597938cc9257d99d9e35f7692c9c06`, LGPL-3.0) covers and GodotPrompter lacks. Nothing from gd-agentic was copied.

## Commits

| Commit | Skills |
|---|---|
| `2706532` | `godot-inventory-system`, `godot-save-load` |
| `cf1c525` | `godot-dialogue-system`, `godot-procedural-generation`, `godot-2d-essentials` |
| the GW5 commit that adds this report | `godot-3d-essentials`, `godot-animation-system` |

Each `SKILL.md` changed in 2 places only: the `description:` keeps the upstream text and adds an "Also covers ..." sentence (450 to 730 bytes in total), and a "Fleet additions" list at the end names each new reference. Each provenance entry has `vendored: true`, the upstream files and `LICENSE` as `sources`, and the `adaptation` text from the common method.

## How the work was checked

- `scripts/godot/api_check.py` on every skill directory: 0 unknown names in all 7 skills after the API fixes below.
- `scripts/godot/gdscript_blocks_check.py` on every skill directory: every block in the new references parses as a whole script. All failing blocks are upstream fragments (listed below).
- A second checker of my own (`.worktrees/_scratch/godot-opus-78/api.py`, not committed) checked member names on typed local variables and bare calls in `extends <EngineClass>` scripts against the 4.7.2 doctool dump. `load()` does not reject `typed_var.no_such_method()` on 4.7.2, so a parse check alone misses these names. It found 0 problems in the new text.
- **Runtime tests.** I ran the code of every new reference headless on Godot 4.7.2 in scratch projects under `.worktrees/_scratch/godot-opus-78/probe*/`. Each test called the classes with real data and printed the results. Facts in the text marked "tested on 4.7.2" or "measured" come from these runs.
- Shader code (`glsl` blocks in `godot-3d-essentials/references/materials-advanced.md`) is not machine-checked: headless Godot uses the dummy renderer and does not compile shaders. The text tells the agent to check shaders with a scene run.

## Per skill

### godot-inventory-system

Added `references/grid-inventory.md` and `references/weight-loot-crafting.md`.

- Added: shaped-item grid inventory (footprint offsets, quarter-turn rotation with normalization, `can_place` / `move` / first-fit `auto_place`, save format); weight limits checked before the add; loot tables as Resources rolled with a caller-owned `RandomNumberGenerator` and `rand_weighted()`; atomic crafting (check room for the result, counting the freed space, before any removal); world pickups that keep the leftover; per-instance item state (`ItemInstance` with durability, saved as a dictionary); consumables as a virtual `use()`.
- Dropped: gd-agentic's reactive UI that reuses slot widgets (GodotPrompter `ui-binding.md` already reuses them); its "use `duplicate(true)` on slot items" rule (conflicts with GodotPrompter's compare-by-reference design; replaced by the instance pattern, with the 4.5 `duplicate_deep()` note from GW10); its crafting code, which removes the ingredients before it checks room for the result (the bug the new text warns about).
- Fact fix: `references/ui-binding.md` GDScript `_drop_data(_at_position, data: Dictionary)` becomes `data: Variant`. Evidence: 4.7.2 parse error "Parent signature is `_drop_data(Vector2, Variant) -> void`".
- Upstream fragments that do not parse alone (report only): `references/serialization.md:80` (uses `player` and a save dictionary from the caller).

### godot-save-load

Added `references/binary-saves.md` and `references/safe-writes.md`.

- Added: `store_var` / `get_var` with `full_objects` / `allow_objects` false and why (a save file is untrusted input); `bytes_to_var_with_objects` has the same risk; the JSON number trap (measured: every JSON number loads as a float, also in arrays); compressed saves with `open_compressed` (ZSTD); temp file and `DirAccess.rename_absolute()` (measured: replaces an existing target on macOS and returns `OK`); rolling `.bak`; an in-file SHA-256 checksum with fallback to the backup (tested); `open_encrypted_with_pass` (measured: a wrong pass returns `null` and `ERR_FILE_CORRUPT` = 16) and the limit that the key ships in the game; when to save (autosave timer, `NOTIFICATION_WM_CLOSE_REQUEST` with `auto_accept_quit = false`, Android `NOTIFICATION_APPLICATION_PAUSED`); stable ids. GW10 item 2 is in: since 4.4 `FileAccess.store_*` returns `bool` (GH-78289).
- Dropped: gd-agentic's AES-ECB encryption script (ECB leaks patterns and has no integrity check; the text says not to build it); its PERSIST group pattern (GodotPrompter's `SaveableComponent` covers the same need); threaded scene loading (belongs to `godot-scene-organization`).
- Upstream fragments (report only): `references/configfile.md:64` (needs the `SettingsManager` autoload), `references/save-architecture.md:81` (calls a method the snippet does not define), `references/version-migration.md:31` (uses `CURRENT_VERSION` from the manager).

### godot-dialogue-system

Added `references/localization-voice-portraits.md` and `references/events-validation-state.md`.

- Added: translation keys instead of sentences, translate then format, `tr_n` plurals, `tr` with context, `auto_translate_mode` off on labels that get pre-translated text, `NOTIFICATION_TRANSLATION_CHANGED`; voice-over per line by locale and line id with `ResourceLoader.exists()` (works in exports); skip rules with voice; `DisplayServer.tts_speak` with the `audio/general/text_to_speech` setting; portraits by speaker and mood; events on line data fired by the manager through a router; a graph validator for missing targets, unknown events, unreachable lines and no path to an end (tested); `DialogueState` with seen lines and flags, saved through JSON (tested: flag numbers come back as floats); choice analytics with a custom `Logger` and `OS.add_logger` (tested on 4.7.2).
- Dropped: the GraphEdit authoring tool (editor tooling, outside a fleet worker's run); TTS-driven lip sync (no viseme data in the boundary callback; too thin to be useful).
- Upstream fragments (report only): `references/branching-and-conditions.md:55` and `:117`, `references/ui-presentation.md:116` and `:243`, `references/variable-interpolation.md:18` and `:35` (all use `GameState`, `Inventory` or a handler from the surrounding scene).

### godot-procedural-generation

Added `references/placement-and-layout.md` and `references/threaded-chunks.md`.

- Added: drunkard's walk with a step bound; Poisson disk sampling (Bridson, tested: minimum distance holds); jittered grid; graph-first dungeon layout with `AStar2D` reachability, distance ordering and lock-and-key checks (tested), with GW10's 4.6 note on disabled start points; one seed per chunk from `hash()`; chunk data on `WorkerThreadPool`, nodes on the main thread through `call_deferred` (tested: 25 chunks load); `Noise.get_image`; saving the seed plus player deltas with a generator version; height-field mesh terrain with `SurfaceTool` and trimesh collision. The winding sentence is measured: the index order in the text is clockwise seen from above, and `generate_normals()` gives it (0, 1, 0).
- Dropped: marching cubes, marching squares and L-systems (long algorithms with no Godot-specific API beyond `ArrayMesh`, which the mesh section covers); gd-agentic's "Path3D snap-to-colliders" 4.7 claim (no such member on `Path3D` or `Curve3D` in the 4.7.2 doctool dump).
- Upstream fragment (report only): `references/bsp-dungeons.md:69` (uses `rooms` from the generator).

### godot-2d-essentials

Added `references/tilemap-runtime.md`.

- Added: the runtime cell API table (layer-local `local_to_map`, `get_neighbor_cell` for iso and hex), batch terrain writes, `TileMapPattern` capture and stamp (tested: patterns are normalized to (0, 0)), per-cell state outside the shared `TileData` (tested: two cells of one tile type return the same `TileData`), `_use_tile_data_runtime_update` / `_tile_data_runtime_update` overrides, `update_internals()`, `get_coords_for_body_rid` with `physics_quadrant_size = 1` (GW10 item 6), `use_kinematic_bodies`, navigation after edits, a custom-data cache with explicit invalidation, isometric Y-sort.
- Measured: the layer's `changed` signal is not emitted at the `set_cell()` call. 5 edits before the first frame emitted it 5 times at the end of that frame; 1 more edit during a frame did not emit it within the next 2 frames. The text tells agents not to use it for cache invalidation.
- Dropped: legacy `TileMap` to layers migration (decision 3: 4.7 only); chunk streaming of tile maps (covered by `godot-procedural-generation/references/threaded-chunks.md`).
- Fact fix: `references/tilemap.md` said tile colliders merge into physics quadrants by "Rendering Quadrant Size". They merge by `physics_quadrant_size` (4.5+); `rendering_quadrant_size` controls draw batching. Evidence: the 4.7.2 doctool dump has 2 separate `TileMapLayer` members; GW10 confirmed the 4.5 change in upgrading_to_godot_4.5.
- Upstream fragment (report only): `SKILL.md:66` (a `Node2D` snippet; the check wraps it in `extends Node`).

### godot-3d-essentials

Added `references/lighting-budget-and-bakes.md`, `references/materials-advanced.md` and `references/gridmap-and-csg.md`.

- Added: a shadowed-light budget and the shared positional shadow atlas; `distance_fade_*` with a separate shadow distance; cascade splits as **fractions** of `directional_shadow_max_distance` (doctool defaults 0.1, 0.2, 0.5); `light_projector`; VoxelGI size and thin-wall rules; `BAKE_DISABLED` instead of hiding a light; the denoiser; shadowmask (`LightmapGIData.SHADOWMASK_MODE_*`); a day-night cycle (sun rotation math checked by hand: noon points straight down); zone-based environment tweens on a duplicated `Environment`; instance uniforms with `set_instance_shader_parameter`; `material_overlay` and `next_pass`; built-in triplanar (`uv1_triplanar`, `uv1_world_triplanar`); vertex wind; subsurface scattering and its Forward+ limit; dither distance fade; Z-fighting (raise `near` first; default 0.05); MeshLibrary collision check, GridMap cells with orthogonal indices (tested: 4 quarter turns round-trip), marker tiles replaced by scenes (tested), GridMap navigation, CSG greybox rules, CSG bake to mesh and collision (tested).
- Measured: `bake_static_mesh()` called in the same frame as `add_child()` returns `null`; one frame later it returns the mesh. The text says to wait one frame.
- Dropped: gd-agentic's cascade code that sets `directional_shadow_split_1 = 10.0` (meters; the property is a fraction); "emission above 1.0 needs HDR in Project Settings" (I found no such setting for 3D in the 4.7.2 dump or docs; dropped as unproven); world streaming of GridMap chunks (gd-agentic's code is a stub; streaming belongs to `godot-scene-organization`); the material batcher and HLOD scripts (covered by GodotPrompter `lod-and-culling.md`).
- API fixes: `references/environment-and-post.md` GDScript `Environment.TONE_MAP_FILMIC` becomes `Environment.TONE_MAPPER_FILMIC`, and `Environment.TONE_MAP_AGX` becomes `Environment.TONE_MAPPER_AGX` (api_check.py). The C# lines already use `ToneMapper.Filmic` / `ToneMapper.Agx` and are unchanged.
- Upstream fragments (report only): `references/decals.md:31` and `references/lod-and-culling.md:66` (preload project files), `references/fog-recipes.md:51`, `references/materials-and-lighting-recipes.md:76`, `:94` and `:218` (use variables or `$` paths from the scene).

### godot-animation-system

Added `references/player-tracks-and-libraries.md`, `references/tree-layering-and-root-motion.md` and `references/2d-cutout-and-frame-events.md`.

- Added: the 4.2 `AnimationMixer` names and the deprecated `ANIMATION_PROCESS_*` constants (GW10 item 7); RESET and `reset_on_save` (doctool default `true`); building animations, method keys and audio tracks in code; value-track update modes; `ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS` / `_MANUAL`; method keys deferred by default (doctool default 0 = `DEFERRED`); `seek(t, true)`; clock sync; `animation_finished` not emitted by a looping animation (tested); off-screen culling with `VisibleOnScreenNotifier3D`; one-shot requests, `Blend2` filters, `TimeScale`, `Transition`, nested state machines; root motion on `CharacterBody3D` (formula from the 4.7 `AnimationMixer` class reference, godot-docs `9adca4c`); 2D cutout rigs; `AnimatedSprite2D` frame events and `set_frame_and_progress` swaps (tested); `AnimatedTexture` deprecated and 2D skeleton modifications experimental (godot-docs 4.7 class reference).
- Measured: in a state machine named `Locomotion` inside a blend tree, the condition path is `parameters/Locomotion/conditions/<name>`, not `parameters/conditions/<name>`. `get_animation_library()` logs an error for a missing library name, so the code tests `has_animation_library()` first.
- Dropped: gd-agentic's claim that `AnimationPlayer` has an `animation_looped` signal (only `AnimatedSprite2D` has it in the 4.7.2 dump); its root-motion code that both multiplies `global_transform` and sets a velocity (moves the body twice); "call `advance(0)` after `AnimatedSprite2D.play()`" (`AnimatedSprite2D` has no `advance()`); procedural squash and tween lifecycle recipes (covered by `godot-tween-animation`); IK and look-at (GodotPrompter already covers the 4.4+ modifiers).
- Upstream fragments (report only): `SKILL.md:107`, `:134`, `:166`, `:224`, `:241` and `references/bone-constraints.md:66` (use `anim_player`, project scenes or tree nodes from the surrounding scene).

## Upstream claims I could not prove (left as they are)

- `godot-animation-system/SKILL.md` C# playback sample: `_animPlayer.SpeedScale = 2.0;`. If `SpeedScale` is a C# `float`, the `double` literal does not compile. I did not run `dotnet build`, so the line stays.
- `godot-procedural-generation/SKILL.md` pitfalls row: "Different results on different platforms: using global `randf()`". The cause is order of calls, not the platform; I did not test across platforms.

## Not done

- No shader block was compiled (no headless shader compiler).
- No C# block was built. `api_check.py` checked their names.
