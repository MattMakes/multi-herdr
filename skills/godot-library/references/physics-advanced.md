# Physics, advanced

Target engine: Godot 4.7. The fixed tick, the two 3D physics engines, physics interpolation,
direct space queries, shape cost, and the body types that need special care.

## Use this shelf when

- You pick or switch the 3D physics engine, or a Jolt setting changes behaviour.
- Motion stutters, streaks after a teleport, or the camera jitters with interpolation on.
- You run many ray or shape queries per tick and want them cheap.
- Bodies tunnel, stacks wobble, or the frame rate collapses when many bodies touch.
- You build a ragdoll, cloth, moving platforms, or a world larger than a few kilometres.

## How the engine works here

- Each rendered frame runs zero or more physics ticks. A tick runs `_physics_process`, then the
  navigation sync, then the physics step. `physics/common/physics_ticks_per_second` (60) sets
  the tick length. CPU use scales about linearly with it. Below 30 behaviour can break down.
- `physics/common/max_physics_steps_per_frame` (8) caps the ticks per frame. It stops the
  "spiral of death" (slow physics causes more ticks, which is slower again). The price: if
  rendering falls under ticks / 8 FPS, the game runs in slow motion even with correct `delta`.
- `physics/common/physics_jitter_fix` (0.5) lets tick timing drift from the real clock to hide
  frame jitter. It is turned off while physics interpolation is on. Set it to 0 for network
  games and for a custom interpolation scheme.
- 3D has two engines. Jolt Physics is built in since 4.4 and is the engine of projects created
  in 4.6 or later. The value `DEFAULT` of `physics/3d/physics_engine` still means
  GodotPhysics3D, so an older project stays on it until you switch. 2D has GodotPhysics2D only.
- Physics is not deterministic in either engine. Do not build lockstep or replays on it; see
  `determinism-and-simulation`.
- The space is locked outside the tick. `get_world_3d().direct_space_state` is safe only in
  `_physics_process`, also for a query that starts from `_input`.
- Physics interpolation renders each node between its previous and current tick transform, so
  the picture is one to two ticks old. 2D does this in the server (since 4.3). 3D arrived in
  4.4 inside the RenderingServer and moved to the scene tree in 4.5, because nodes need the
  interpolated transform and the server could not be asked for it. So 3D bodies made with
  `PhysicsServer3D` alone are not interpolated.
- Godot Physics detects collisions mostly with the separating axis test and uses a dynamic
  BVH as broad phase. Cylinders do not fit that test and fall back to a generic GJK-EPA path;
  this is why they are its weakest shape. A height map finds its triangles by grid lookup, and
  rays walk the grid like a Bresenham line, so no triangle tree is searched.

## Advanced techniques

### Know what Jolt changes

| Topic | Godot Physics 3D | Jolt Physics |
| --- | --- | --- |
| Joint soft limits (`bias`, `softness`, `relaxation`, `damping`) | used | ignored, warns |
| One-body joint | body acts as `node_a` | acts as `node_b`; `joints/world_node` flips it |
| `Shape3D.margin` | unused | upper bound; real margin is `collision_margin_fraction` |
| Penetration fix | spring, can overshoot | position only, slower, no overshoot |
| Ray `face_index` | filled | -1 until `queries/enable_ray_cast_face_index` (+25% trimesh RAM) |
| Frozen kinematic body vs static contacts | reported | off until `generate_all_kinematic_contacts` |
| Cylinders, soft bodies, stacks | weak | more reliable |

Settings live under `physics/jolt_physics_3d/`. Jolt has hard limits: `limits/max_bodies`
(10240, awake or asleep; past it behaviour is undefined) and `limits/max_body_pairs` (65536;
past it collisions are dropped at random).

### Tune the tick, not the frame

Raise the tick rate for fast vehicles and tall stacks (120, 180, 240 keep a clean ratio to 60).
A car at 360 km/h moves about 1.67 m per 60 Hz tick. With interpolation on, any rate works
visually, so 30 can halve physics cost in a slow game. `Engine.physics_ticks_per_second` and
`Engine.max_physics_steps_per_frame` change them at run time.

### Make interpolation correct

Turn on `physics/common/physics_interpolation`. Move every interpolated node only in the tick:
scripts in `_physics_process`, `Tween.set_process_mode()`, `AnimationMixer.callback_mode_process`
and parents too. After a teleport or spawn call `reset_physics_interpolation()`. For a bullet:
set the start transform, reset, then set the transform one tick ahead, so it moves at once.
Test at 10 ticks per second; every missed case becomes obvious.

### Interpolate the camera by hand

Set the camera to `PHYSICS_INTERPOLATION_MODE_OFF`, make it `top_level`, and in `_process`
follow `target.get_global_transform_interpolated()`. Apply mouse look directly, without
interpolation. Use that method only for a few nodes; game logic must read tick transforms.
An "off" child of an "on" parent still moves smoothly with the parent.

### Query the space without garbage

Create the query object once and reuse it. One `intersect_shape` replaces many overlap `Area3D`
nodes that would track pairs all the time.

```gdscript
extends Node3D

var _shape := SphereShape3D.new()
var _query := PhysicsShapeQueryParameters3D.new()

func _ready() -> void:
	_shape.radius = 4.0
	_query.shape = _shape
	_query.collision_mask = 1 << 2
	_query.collide_with_areas = false

func _physics_process(_delta: float) -> void:
	_query.transform = global_transform
	var space := get_world_3d().direct_space_state
	var hits: Array[Dictionary] = space.intersect_shape(_query, 16)
	for hit: Dictionary in hits:
		var body := hit["collider"] as Node3D
		if body != null:
			body.add_to_group(&"alerted")
```

`cast_motion` returns the safe and unsafe fraction of a sweep (a cheap "how far can I move").
`get_rest_info` returns the nearest contact with its normal. `collide_shape` returns contact
point pairs. Lower `max_results`.

### Pay for shapes in the right order

Cheapest to dearest: sphere, box, capsule, cylinder, one convex hull, several convex hulls,
trimesh. Trimesh (`ConcavePolygonShape3D`) works for static bodies only. Keep shape nodes
unscaled and, if possible, untransformed. A static body with many shapes defeats the broad
phase. Convex decomposition can make dozens of hulls for one prop; two boxes often do. Build
level collision from a simplified mesh. For terrain use `HeightMapShape3D`, not a trimesh.

### Merge tile and grid colliders

A body bumps on the seams between adjacent shapes ("ghost collisions"). Before 4.5 every tile
was its own body, which the 4.5 release notes call a likely cause of slow 2D physics. Since 4.5
`TileMapLayer.physics_quadrant_size` (16) merges tile shapes per chunk; larger is smoother but
slower to edit at run time, 1 disables it. One body now covers many cells, so
`get_coords_for_body_rid()` no longer identifies a single cell. Jolt reduces seams inside one body with
`collisions/active_edge_threshold` and enhanced internal edge removal, not between two bodies.

### Stop tunneling in layers

First thicken static colliders. Then `continuous_cd` (2D: `CCD_MODE_CAST_RAY` is cheaper than
`CCD_MODE_CAST_SHAPE`). In Jolt, CCD starts only when a body moves more than
`simulation/continuous_cd_movement_threshold` (0.75) of its inner radius per step. Last, raise
the tick rate. For bullets a ray or `cast_motion` per tick beats a tiny rigid body.

### Keep areas and contacts cheap

Give each area the smallest mask.
Set `monitoring` false on areas that are only detected, `monitorable` false on areas that only
detect. `contact_monitor` needs `max_contacts_reported` above 0 and costs per contact. With
Jolt, areas report static bodies always since 4.5 and soft bodies since 4.7; filter by mask.

### Let bodies sleep; integrate by hand when needed

Sleeping bodies cost almost nothing and skip `_integrate_forces`. Avoid `can_sleep = false` and
per-tick transform writes on rigid bodies. To steer a body, set velocities in
`_integrate_forces(state)` and divide by `state.step`. `custom_integrator = true` removes
gravity and damping so the callback owns the motion. Server-only bodies use
`PhysicsServer3D.body_set_force_integration_callback`.

### Use the CharacterBody options

`floor_snap_length` keeps the body on downward slopes; `floor_constant_speed` needs it.
`platform_on_leave` chooses whether platform velocity is kept on a jump. `platform_floor_layers`
and `platform_wall_layers` choose which bodies carry the character. `MOTION_MODE_FLOATING` is for
top-down and flying. Raise `safe_margin` for jitter against walls. Animate platforms with
`AnimatableBody3D` and `sync_to_physics`, with the animation on the physics callback.

### Ragdolls and cloth

Delete finger, helper and root bones from the generated `PhysicalBone3D` set. Replace the
default pin joints (limbs crumple) with hinge joints for knees and elbows and cone joints for
shoulders and hips (swing 20 to 90 degrees, twist 20 to 45). `physical_bones_start_simulation()`
takes bone names for a partial ragdoll; a wrong name fails silently. Blend with the simulator's
`influence`. Put bones on a layer the character capsule does not hit.
`SoftBody3D` is more robust in Jolt. The 4.5 beta notes list physics interpolation support for
it, while the 4.7 manual page still says it is not interpolated: test at a low tick rate. Keep
`simulation_precision` above 5. Pin points to a `BoneAttachment3D` instead of parenting. Turn
off import LOD for the mesh. Since 4.5 `apply_central_impulse()` and the other force calls
work on soft bodies (wind gusts, hits).

### Large worlds

A 32-bit float steps 0.0005 m between 4096 and 8192 m. The docs give +-4096 m as the comfortable
range for first person, 8192 m third person, 32768 m top-down. Past that, shift the origin, or
build editor and templates with `precision=double` (slower, heavy on mobile, extensions must be
rebuilt, server and clients must match). See `worlds-and-streaming`.

## Niche but useful

- `one_way_collision_direction` on `CollisionShape2D` (since 4.7): one-way walls and ceilings.
- `SeparationRayShape3D`: a ray shape that holds a body above ground; steps without a capsule.
- `collision_priority`: the higher body is pushed less in a penetration (player versus crates).
- `physics_engine = Dummy` (since 4.4): a server build that needs no physics pays nothing.
- `debug_color` and `debug_fill` on `CollisionShape3D` (since 4.4): colour hitboxes, hurtboxes
  and triggers differently under Debug > Visible Collision Shapes, also at run time.
- The editor's game speed controls (since 4.6) slow a running game to inspect physics.
- `MultiMesh.physics_interpolation_quality`: the high setting helps under about 20 ticks per
  second or with fast rotation.
- `MultiMesh.set_buffer_interpolated()` and `reset_instance_physics_interpolation()`: smooth
  instanced crowds at a low tick rate.
- `Performance.PHYSICS_3D_ACTIVE_OBJECTS`, `PHYSICS_3D_COLLISION_PAIRS`, `PHYSICS_3D_ISLAND_COUNT`:
  watch them; rising pairs with flat bodies means bad masks.

## Traps

- Scaling a body or shape node. Resize the shape resource (and `duplicate()` it first).
- `get_global_transform_interpolated()` in game logic, or moving interpolated nodes in `_process`.
- Multiplying `velocity` by `delta` before `move_and_slide()`.
- `physics/3d/run_on_separate_thread`: API access is limited to the tick; with Jolt it is
  experimental and errors name nodes `<unknown>`.
- Only `CPUParticles2D` is interpolated in 2D; GPU 2D particles are not.
- Outdated: "Jolt is an add-on" or "experimental" (built in since 4.4, label dropped in 4.6)
  and the extension's `physics/jolt_3d/` setting names (now `physics/jolt_physics_3d/`).
- The extension's own joint nodes are not in the built-in module.

## Version notes

- 4.3: 2D physics interpolation.
- 4.4: Jolt built in (experimental). 3D physics interpolation, with MultiMesh support and
  `get_global_transform_interpolated()`. Dummy physics servers. Collision shape debug colours.
- 4.5: 3D interpolation moved to the scene tree (same API), with custom interpolation for
  vehicle wheels. TileMapLayer collision chunks. Jolt updated to 5.3.0, cheaper non-monitoring
  areas, setting `areas_detect_static_bodies` removed (always on). Forces on soft bodies.
- 4.6: Jolt is the engine of new 3D projects and no longer experimental. MultiMesh
  interpolation for 2D transforms. `reset_instances_physics_interpolation()`.
- 4.7: Jolt `WorldBoundaryShape3D` plane distance sign flipped to match Godot Physics. Jolt
  `SoftBody3D` mass defaults to 1 kg in total and `linear_stiffness` changed: retune cloth.
  `Area3D` reports soft bodies. One-way collision direction in 2D (before, always "up").
- Direction: the physics team plans to reshape the node API to mirror Jolt (per-shape
  filtering and materials are proposed) and expects compatibility breaks. Keep physics setup
  behind your own small helper functions.

## Related skills

- `godot-physics-system`: body types, layers, basic queries and setup steps.
- `godot-player-controller`: character movement recipes on `CharacterBody2D` and `CharacterBody3D`.
- `godot-perf-measure`: measure the physics time before tuning.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/physics/using_jolt_physics.html
- https://docs.godotengine.org/en/stable/tutorials/physics/troubleshooting_physics_issues.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/using_physics_interpolation.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/advanced_physics_interpolation.html
- https://docs.godotengine.org/en/stable/tutorials/physics/ray-casting.html
- https://docs.godotengine.org/en/stable/tutorials/physics/collision_shapes_3d.html
- https://docs.godotengine.org/en/stable/tutorials/physics/ragdoll_system.html
- https://docs.godotengine.org/en/stable/tutorials/physics/soft_body.html
- https://docs.godotengine.org/en/stable/tutorials/physics/large_world_coordinates.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.5.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/article/dev-snapshot-godot-4-4-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-4/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-5/
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-7-beta-1/
- https://godotengine.org/article/physics-progress-report-1/
- https://godotengine.org/priorities/
