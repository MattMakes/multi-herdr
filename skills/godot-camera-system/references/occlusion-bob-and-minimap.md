# 3D occlusion without SpringArm3D, look_at safety, head bob and minimaps

Adds a ray-based occlusion rig for 3D cameras, a guard for `look_at()` when the target is straight above or below, first-person head bob and weapon sway, and a minimap `SubViewport` that does not render every frame. Read it when a `SpringArm3D` does not fit the rig, when the camera flips, when a first-person view needs motion, or when you add a minimap.

All code targets Godot 4.7.

## Occlusion with a ray

A `SpringArm3D` (see `references/camera3d-patterns.md`) is the first choice: set its `collision_mask` to the world layers, or it passes through walls. When the rig is not a straight arm, for example a camera on a rail or a fixed cinematic offset, cast a ray from the target to the ideal camera position. If it hits, put the camera just in front of the hit.

```gdscript
extends Camera3D

@export var target: Node3D
@export var ideal_offset: Vector3 = Vector3(0.0, 2.0, 5.0)  # in the target's space
@export_flags_3d_physics var world_mask: int = 1
@export var wall_padding: float = 0.3
@export var follow_rate: float = 10.0


func _physics_process(delta: float) -> void:
    if target == null:
        return
    var pivot: Vector3 = target.global_position + Vector3.UP * 1.5
    var ideal: Vector3 = target.global_transform * ideal_offset

    var query: PhysicsRayQueryParameters3D = PhysicsRayQueryParameters3D.create(pivot, ideal, world_mask)
    if target is CollisionObject3D:
        query.exclude = [(target as CollisionObject3D).get_rid()]
    var hit: Dictionary = get_world_3d().direct_space_state.intersect_ray(query)

    var wanted: Vector3 = ideal
    if not hit.is_empty():
        var p: Vector3 = hit["position"]
        wanted = p + (pivot - p).normalized() * wall_padding

    var weight: float = 1.0 - exp(-follow_rate * delta)
    global_position = global_position.lerp(wanted, weight)
    _safe_look_at(pivot)


func _safe_look_at(point: Vector3) -> void:
    var dir: Vector3 = point - global_position
    if dir.length_squared() < 0.0001:
        return
    # look_at fails when the direction is parallel to the up vector.
    var up: Vector3 = Vector3.UP
    if absf(dir.normalized().dot(up)) > 0.999:
        up = Vector3.FORWARD
    look_at(point, up)
```

- Move the camera toward a hit fast and away from it slowly. A camera that springs back at full speed when the wall ends feels nervous. Use two rates: a high one when `wanted` is closer to the pivot than the camera, a low one otherwise.
- A thin ray passes through gaps between objects that the camera's near plane still touches. If the near plane clips, sweep a small sphere with `cast_motion` instead (see `references/queries-and-casts.md` in **godot-physics-system**).

### look_at safety

`Node3D.look_at(target, up)` builds a basis from the direction and the up vector. When the two are parallel, the basis is undefined: Godot prints an error and the node does not turn. This happens to a camera directly above a target (top-down) or an orbit camera at the pole. The `_safe_look_at()` helper above picks another up vector in that case. Also clamp an orbit camera's pitch to less than 90 degrees, for example ±85.

## Head bob and weapon sway

Head bob moves the camera on a small sine curve while the player walks. Sway lags a held weapon behind fast turns. Apply both as offsets on top of the real look direction, so they never change where the player aims.

```gdscript
extends Camera3D

@export var body: CharacterBody3D
@export var weapon: Node3D
@export var bob_frequency: float = 2.2  # steps per second at full speed
@export var bob_height: float = 0.05
@export var bob_width: float = 0.03
@export var walk_speed: float = 5.0
@export var sway_amount: float = 0.002
@export var sway_return: float = 8.0

var _bob_time: float = 0.0
var _sway: Vector2 = Vector2.ZERO
var _weapon_rest: Vector3 = Vector3.ZERO


func _ready() -> void:
    if weapon != null:
        _weapon_rest = weapon.position


func _unhandled_input(event: InputEvent) -> void:
    if event is InputEventMouseMotion:
        var motion: InputEventMouseMotion = event
        _sway -= motion.relative * sway_amount


func _process(delta: float) -> void:
    var flat_speed: float = Vector2(body.velocity.x, body.velocity.z).length()
    if body.is_on_floor() and flat_speed > 0.1:
        _bob_time += delta * bob_frequency * (flat_speed / walk_speed) * TAU
        v_offset = sin(_bob_time) * bob_height
        h_offset = cos(_bob_time * 0.5) * bob_width
    else:
        # Settle back to rest without a jump.
        var settle: float = 1.0 - exp(-10.0 * delta)
        v_offset = lerpf(v_offset, 0.0, settle)
        h_offset = lerpf(h_offset, 0.0, settle)

    _sway = _sway.lerp(Vector2.ZERO, 1.0 - exp(-sway_return * delta))
    _sway = _sway.limit_length(0.08)
    if weapon != null:
        weapon.position = _weapon_rest + Vector3(_sway.x, -_sway.y, 0.0)
```

- `h_offset` and `v_offset` shift the view without moving or turning the camera node, so rays cast from the camera's transform still aim true.
- The horizontal bob runs at half the vertical rate: one side-to-side cycle per two steps, like a real walk.
- Bob causes motion sickness for some players. Expose a setting that scales `bob_height` and `bob_width` down to 0.

## Minimap with a SubViewport

A minimap is a second camera that renders into a `SubViewport`. Show it in the HUD with a `TextureRect` whose `texture` is a `ViewportTexture` of that `SubViewport`. Then your script alone controls when the `SubViewport` renders.

1. Add a `SubViewport` with a `Camera2D` child (or a `Camera3D` with orthographic projection, looking down).
2. Share the main world, so the minimap sees the same level. In 2D, set `minimap_viewport.world_2d = get_viewport().world_2d` in `_ready()`. In 3D, a `SubViewport` already shares the parent's world unless `own_world_3d` is on.
3. Hide what the minimap should not draw with visibility layers: put minimap icons on their own layer and set each camera's cull mask (`Camera3D.cull_mask`) or the `SubViewport`'s `canvas_cull_mask` (2D) so the main view does not show them.
4. Set `render_target_update_mode`. The default `UPDATE_WHEN_VISIBLE` renders every frame while shown. A minimap that changes slowly does not need that:

```gdscript
extends SubViewport

@export var map_camera: Camera2D
@export var follow: Node2D
@export var updates_per_second: float = 10.0

var _accum: float = 0.0


func _ready() -> void:
    world_2d = get_tree().root.world_2d
    render_target_update_mode = SubViewport.UPDATE_DISABLED


func _process(delta: float) -> void:
    _accum += delta
    if _accum < 1.0 / updates_per_second:
        return
    _accum = 0.0
    if follow != null:
        map_camera.global_position = follow.global_position
    # Render exactly one frame, then stay idle until the next request.
    render_target_update_mode = SubViewport.UPDATE_ONCE
```

At 10 updates per second instead of 60, the minimap costs about one sixth of the render time. Keep the `SubViewport`'s `size` small (for example 256 × 256); a minimap does not need the full screen resolution.
