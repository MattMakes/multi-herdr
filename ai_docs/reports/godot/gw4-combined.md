# GW4 godot-combined: gameplay

Unit GW4 of the Godot wave. Seven combined skills: the GodotPrompter v1.14.0 skill is the base (renamed by `scripts/godot/rename.py`, body unchanged), and what gd-agentic-skills covers that GodotPrompter lacks is added as `references/<topic>.md` in own text and own code (operator decision 1). Plan: `ai_docs/plans/godot/gw4-combined.md`, method: `ai_docs/plans/godot/gw-combined-common.md`.

## Result

| skill | GodotPrompter base | gd-agentic consulted (4c4d0ff) | new references | own blocks |
|---|---|---|---|---|
| `godot-player-controller` | player-controller | godot-characterbody-2d | platformer-feel, collision-and-platforms, top-down-pixel-and-crowds | 11 |
| `godot-state-machine` | state-machine | godot-state-machine-advanced | pushdown-and-transitions, observers-debug-and-utility | 12 |
| `godot-input-handling` | input-handling | godot-input-handling | buffers-combos-and-accessibility, injection-replay-conflicts-multitouch | 8 |
| `godot-physics-system` | physics-system | godot-2d-physics, godot-physics-3d, godot-raycasting-queries | queries-and-casts, server-bodies-and-bullets, 3d-stairs-hover-and-vehicles | 13 |
| `godot-camera-system` | camera-system | godot-camera-systems | follow-rigs-and-framing, occlusion-bob-and-minimap | 7 |
| `godot-ai-navigation` | ai-navigation | godot-ai-navigation, godot-navigation-pathfinding | costs-links-obstacles-and-recovery, server-crowds-and-queries | 8 |
| `godot-ability-system` | ability-system | godot-ability-system, godot-rpg-stats | cooldown-rules-charges-and-combos, progression-and-formulas | 8 |

SKILL.md edits per skill: the `description:` is extended (all under 520 bytes) and a "Fleet additions" list is appended. Nothing else in upstream text changed, except the API fix in `godot-ability-system` below.

## Commits

- `be9b0d0` batch 1: godot-player-controller, godot-state-machine, godot-input-handling.
- `1c058cb` batch 2: godot-physics-system, godot-camera-system.
- Batch 3 (godot-ai-navigation, godot-ability-system and this report) is the commit that adds this file.

All three went through `.worktrees/godot-commit.sh`; `skills_catalog` passed 22 of 22 each time.

## Checks

On the 7 skills as committed (upstream text included):

- `scripts/godot/api_check.py`: 55 files, 0 unknown names (after the fix below).
- `scripts/godot/gdscript_blocks_check.py`: 160 blocks; 143 parse, 17 fail. All 17 failures are upstream fragments (see "Upstream findings"). My 67 blocks all parse.
- Runtime tests on Godot 4.7.2 headless, in `.worktrees/_scratch/godot-opus-77/` (not committed), for the code whose behavior a parse check cannot prove:
  - 2D step-up (`collision-and-platforms.md`): climbs a 6 px step with `max_step` 8, stays on the floor; a 10 px step blocks it.
  - CharacterBody3D stairs (`3d-stairs-hover-and-vehicles.md`): climbs a 0.2 m step, stops at a 0.5 m wall.
  - Piercing ray (`queries-and-casts.md`): returns 3 bodies in distance order and stops at a `StaticBody2D`.
  - PhysicsServer2D bodies (`server-bodies-and-bullets.md`): run, and free their RIDs with no leak report at exit.
  - Navigation (`costs-links-obstacles-and-recovery.md`, `server-crowds-and-queries.md`): `link_reached` details hold `position`, `type`, `rid`, `owner`, `link_entry_position`, `link_exit_position`; `is_target_reachable()` is false for an island with no link; the procedural bake with `agent_radius` 10 routes around a 100 px box; `query_path` with reused objects returns the path; server avoidance callbacks deliver safe velocities.
  - Combo matcher (`buffers-combos-and-accessibility.md`): a typed `Array[StringName]` slice compares equal to an untyped `Array`; `keys()` must go into an untyped `Array`.
  - `CooldownBook`, `LevelCurve`, `Progression`, `SkillTree`, `DamageMath` (`godot-ability-system`): GCD, shared group, 2 charges with recharge, level cap, prerequisites, rank cap, respec refund and save/load all behave as written.

## Gap lists

### godot-player-controller

Added: air control apart from ground control; ceiling bonk; wall slide speed cap with a wall-coyote window (GodotPrompter's wall jump has neither); decaying knockback; dash with start and end signals for invulnerability; jump-arc debug view; the slide-collision loop; floor property table; one-way platforms with the 4.7 `one_way_collision_direction` and drop-through by mask bit; moving platforms (`platform_on_leave`, `platform_*_layers`); 2D step-up with `test_move`; tank controls; pixel-art snapping; off-screen disabling with `VisibleOnScreenEnabler2D`.

Dropped:
- "After `move_and_slide()`, add `get_platform_velocity()` to velocity" (gd-agentic movement-recipes): wrong. `move_and_slide()` already carries the body; adding it doubles the motion. The reference says so.
- Decay with `lerp(Vector2.ZERO, 0.2)` per frame: frame-rate dependent; replaced by `move_toward` with a rate.
- Frame-count coyote timers: replaced by seconds (frame counts change with the tick rate).
- 2D root motion from `AnimationTree.get_root_motion_position()`: root motion tracks are 3D tracks; I could not verify a 2D setup on 4.7.2, so it is not shipped.
- Pooling advice and the 4.x migration notes: out of scope or covered elsewhere (decision 3).

### godot-state-machine

Added: pushdown stack with `pause()` and exactly one `exit()`, resume flag; `enter(msg)` payload and context rule; timed states counted in `physics_update`; transition guard table with `StringName` names; re-entrant transition queue; `state_changed` observers for `AnimationTree` travel and sound; transition history ring buffer; on-screen state label; utility scoring with a switch margin.

Dropped: the HSM delegation rules (GodotPrompter's `hierarchical-and-parallel.md` covers them); "never deeper than 3 levels" (opinion, no fact to check); the `_draw()` visualizer (a `Label` does the job).

### godot-input-handling

Added: multi-action buffer with `consume()`; timed combo sequences on the physics clock; echo rules (`is_echo()`, `allow_echo`); hold-or-toggle accessibility; `parse_input_event` versus `action_press`; per-tick record and replay with `process_physics_priority`; rebind conflict checks with `is_match()`; two-finger pinch and pan plus `InputEventMagnifyGesture`.

Dropped: radial deadzone and device-change glyphs (GodotPrompter `gamepad.md` has both); 4.7 device IDs (GodotPrompter SKILL.md has them); multiplayer input RPCs (belongs to `godot-multiplayer-*`); event replay by render frame (drifts; replaced by per-tick action states); `Time.get_ticks_msec` combo timing (wall clock; replaced by the physics clock).

### godot-physics-system

Added: query choice table and timing rules (first-frame queries, forced cast updates, colliders that are not bodies); `ShapeCast2D`; `intersect_shape` and `intersect_point` with code; piercing and reflecting rays (`bounce` versus `reflect`); field-of-view check; stuck recovery with `get_rest_info`; surface metadata; bullets as data with swept rays; `PhysicsServer2D` bodies with `free_rid` and `body_attach_object_instance_id`; CCD guidance; CharacterBody3D stairs; ray-spring hover body; `VehicleBody3D` settings.

Dropped:
- "`CharacterBody2D` ships with `collision_layer = 0`" (gd-agentic 2d-physics): wrong; the 4.7.2 dump gives default 1.
- "An `Area2D` with several shapes fires `body_entered` once per shape": not shipped; per-shape events are `body_shape_entered`.
- Joint breakage by stress: Godot joints have no break threshold; the gd-agentic script measures distance by hand. Not shipped.
- Collision debouncer, batch movers, manual interpolation: covered by GodotPrompter (interpolation) or too thin.

### godot-camera-system

Added: exponential smoothing `1 - exp(-rate * delta)` and log-space zoom (GodotPrompter's `lerp(..., speed * delta)` is frame-rate dependent; the reference tells the reader to swap the weight); `RemoteTransform2D/3D` decoupling; shake on `offset` only; multi-target framing; priority camera director; ray occlusion without `SpringArm3D`; `look_at` up-vector guard; head bob and weapon sway on `h_offset`/`v_offset`; throttled minimap with `UPDATE_ONCE`.

Dropped: lerp follow, look-ahead and drag margins (GodotPrompter has them); trauma debugger (thin); rule-of-thirds framing (no checkable fact).

### godot-ai-navigation

Added: region `enter_cost`/`travel_cost`; `NavigationLink2D/3D` with `link_reached`; `NavigationObstacle` avoidance versus `affect_navigation_mesh`/`carve_navigation_mesh`; unreachable targets; stuck detection; leader-relative formations; server avoidance agents; reused `query_path`; procedural bake with `parse_source_geometry_data` + `bake_from_source_geometry_data_async` and the 2D traversable-outline rule.

Dropped: retarget throttling (GodotPrompter's pitfalls table and checklist have it); "never bake at runtime synchronously" (GodotPrompter covers the async region bake); the gd-agentic chase/patrol recipes (they set `target_position` every frame, which its own rules forbid).

### godot-ability-system

Added: global cooldown, cooldown groups, charges with carry-over recharge; combo finishers; per-caster state in shared Resources (`duplicate(true)`, `resource_local_to_scene`); saving cooldowns in game time or real time; capped experience curve; multi-level `add_xp`; skill tree with prerequisites, ranks, respec, clamped load; derived stats with diminishing returns; one static damage formula with an injected `RandomNumberGenerator`.

Dropped: "save absolute end timestamps to stop reload exploits" as a rule: saving seconds left does not allow a reload exploit; the reference gives both meanings and when each fits. Networked cast validation (belongs to `godot-multiplayer-*`). Equipment tooltips (UI). `StatusEffectData` types (GodotPrompter `stat-modifiers.md` already has ADD/MULTIPLY/OVERRIDE).

## Upstream findings

API fixed in place (orchestrator decision; listed in the skill's provenance `adaptation`):

- `godot-ability-system/references/stat-modifiers.md:64` and `tags-and-conditions.md:323`: C# `StringName.Empty` does not exist. Checked in `modules/mono/glue/GodotSharp/GodotSharp/Core/StringName.cs` on `4.5-stable` and `master`: the class has `public StringName()`, `public StringName(string)`, an implicit conversion from `string` and the instance property `IsEmpty`, and no static `Empty`. Now `new StringName()`.
- `tags-and-conditions.md:347`: `effect.ImmunityTag != StringName.Empty` is now `!effect.ImmunityTag.IsEmpty`.

Fact fixed in place (standing rule; listed in provenance as "Fact fixes on 4.7"):

- `godot-physics-system/SKILL.md:25` and `:140`: Jolt is the default engine for new 3D projects since 4.6, not 4.4 (it is built in since 4.4). Evidence: godot-docs 4.7 `upgrading_to_godot_4.6`, GH-105737, reported by GW10 (opus-83). My own `3d-stairs-hover-and-vehicles.md` had the same error and is corrected.

Added at the orchestrator's request (GW10 item 5), verified by a headless run on 4.7.2, in `godot-ai-navigation/references/server-crowds-and-queries.md`: regions and maps update asynchronously since 4.5 (`navigation/world/region_use_async_iterations` and `map_use_async_iterations` default `true`); since 4.6 `AStar2D` and `AStarGrid2D` return an empty path for a disabled or solid start point, also with `allow_partial_path` (GH-113988).

Block-check failures in upstream text, left as they are (GW0's gate reports them without failing): 17 fragments that use names from outside the block.

- `godot-ability-system/references/stat-modifiers.md:330`, `:374`; `tags-and-conditions.md:428`, `:451`, `:462`
- `godot-ai-navigation/SKILL.md:353`; `references/chase-attack.md:38` (a `State` inner enum used as a type across blocks)
- `godot-camera-system/SKILL.md:193`; `references/transitions.md:127`
- `godot-input-handling/SKILL.md:105`, `:148`; `references/event-propagation.md:15`, `input-buffering.md:28`, `mouse.md:96`
- `godot-physics-system/references/area-recipes.md:62`, `rigidbody-recipes.md:68`
- `godot-state-machine/SKILL.md:42`

Other upstream notes, not changed:

- `godot-ability-system/SKILL.md` ticks cooldowns in `_process`. My reference recommends ticking where gameplay runs.
- `godot-camera-system` follow code uses `lerp(..., speed * delta)`. My reference explains the exponential weight.
- `godot-player-controller/SKILL.md` header says "Godot 4.3+"; all code still parses on 4.7.2.

## Notes for others

- `NavigationPolygon.make_polygons_from_outlines()` prints a deprecation warning on 4.7.2. No GW4 file uses it; other units should not either.
- `godot-combat-system` is named in `progression-and-formulas.md` as the owner of hit resolution. It is a GW9 bundle; if it is renamed, update that line.
