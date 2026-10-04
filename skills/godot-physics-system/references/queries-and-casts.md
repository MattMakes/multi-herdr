# Shape casts, space queries and ray recipes

Adds `ShapeCast2D/3D`, forced cast updates, complete `intersect_shape` and `intersect_point` code, piercing and reflecting rays, a field-of-view check, stuck detection with `get_rest_info`, and the timing rules for queries. Read it when a thin ray misses, when one ray must hit several things, or when a query returns nothing for no clear reason.

All code targets Godot 4.7. Examples are 2D; the 3D classes have the same names with `3D` and take `Vector3`.

## Choose the cheapest query

| question | API | note |
|---|---|---|
| What does a line hit first? | `intersect_ray` or a `RayCast2D` node | Cheapest. A ray is infinitely thin, so it slips through gaps. |
| What does a volume hit along a path? | `ShapeCast2D` node, or `cast_motion` | For ground checks under a wide body, thick lasers, sweeps. |
| What overlaps a shape right now? | `intersect_shape` | Explosions, melee hit boxes for one frame, spawn checks. |
| What is under a point? | `intersect_point` | Clicks in 2D, "is this cell free". |
| Is this body stuck inside something? | `get_rest_info` | Returns the deepest contact. |

Filter with `collision_mask` in the query. A narrow mask is faster than a wide query whose results you filter in script.

## Timing rules

- Use `direct_space_state` only inside `_physics_process()` or a physics callback. Outside it, the space can be busy, and with physics on a separate thread the call is unsafe.
- A `RayCast2D` or `ShapeCast2D` node updates once per physics tick. If you move it or change `target_position` and read it in the same tick, call `force_raycast_update()` or `force_shapecast_update()` first.
- In `_ready()`, the shapes of new nodes are not in the space yet. A query there finds nothing. Wait one tick: `await get_tree().physics_frame`.
- The `collider` in a result is not always a `PhysicsBody2D`. A `TileMapLayer`, an `Area2D` (with `collide_with_areas`) or a CSG node in 3D can be the collider. Check its type before you call body methods on it.

## ShapeCast2D: a ray with thickness

A `ShapeCast2D` sweeps its `shape` from its position to `target_position` and reports every hit, up to `max_results`. Use it for footing under a wide body, where one center ray falls into a gap between tiles.

```gdscript
extends CharacterBody2D

@onready var _ground_cast: ShapeCast2D = $GroundCast  # small box shape, target_position = (0, 6)


func is_grounded() -> bool:
    _ground_cast.force_shapecast_update()
    return _ground_cast.is_colliding()


func ground_normal() -> Vector2:
    if not _ground_cast.is_colliding():
        return Vector2.UP
    # Average the normals of all contacts for a stable slope.
    var sum: Vector2 = Vector2.ZERO
    for i in _ground_cast.get_collision_count():
        sum += _ground_cast.get_collision_normal(i)
    return sum.normalized()


func _ready() -> void:
    _ground_cast.add_exception(self)  # never report our own body
```

`get_closest_collision_safe_fraction()` returns how far along the sweep the shape can move before it touches something (0 to 1). Use it to place an object flush against a wall.

## Overlap query: hit everything in a radius

An explosion damages everything inside a circle in one tick. A `intersect_shape` query needs no node and no wait for signals.

```gdscript
extends Node2D

@export_flags_2d_physics var damage_mask: int = 0b0110
@export var max_hits: int = 32


func explode(radius: float, damage: int) -> void:
    var circle: CircleShape2D = CircleShape2D.new()
    circle.radius = radius
    var query: PhysicsShapeQueryParameters2D = PhysicsShapeQueryParameters2D.new()
    query.shape = circle
    query.transform = Transform2D(0.0, global_position)
    query.collision_mask = damage_mask
    query.collide_with_areas = true  # hurtboxes are often Area2D

    var space: PhysicsDirectSpaceState2D = get_world_2d().direct_space_state
    var hits: Array[Dictionary] = space.intersect_shape(query, max_hits)
    var done: Dictionary[int, bool] = {}
    for hit: Dictionary in hits:
        var target: Object = hit["collider"]
        var id: int = hit["collider_id"]
        if done.has(id):
            continue  # one result per shape: a body with 3 shapes appears 3 times
        done[id] = true
        if target.has_method(&"take_damage"):
            target.call(&"take_damage", damage)
```

`intersect_shape` returns at most `max_results` entries (default 32). A crowd larger than that is cut off silently; raise the limit for big explosions.

## Point query: what was clicked

```gdscript
extends Node2D


func object_at(world_pos: Vector2) -> Object:
    var query: PhysicsPointQueryParameters2D = PhysicsPointQueryParameters2D.new()
    query.position = world_pos
    query.collide_with_areas = true
    var hits: Array[Dictionary] = get_world_2d().direct_space_state.intersect_point(query, 1)
    return hits[0]["collider"] if not hits.is_empty() else null
```

Pass `get_global_mouse_position()` as `world_pos`, not the event's screen position. With a camera, the two differ.

## Piercing ray: several hits on one line

A ray stops at the first hit. To pierce, cast again from the same start and add each hit to `exclude`. Stop at a wall or after the pierce limit.

```gdscript
extends Node2D

@export_flags_2d_physics var hit_mask: int = 0b0110
@export var max_pierce: int = 4


func pierce(from: Vector2, to: Vector2) -> Array[Dictionary]:
    var space: PhysicsDirectSpaceState2D = get_world_2d().direct_space_state
    var query: PhysicsRayQueryParameters2D = PhysicsRayQueryParameters2D.create(from, to, hit_mask)
    var excluded: Array[RID] = []
    var hits: Array[Dictionary] = []
    while hits.size() < max_pierce:
        query.exclude = excluded  # assign a new array; editing the old one does not update the query
        var hit: Dictionary = space.intersect_ray(query)
        if hit.is_empty():
            break
        hits.append(hit)
        var collider: Object = hit["collider"]
        if collider is StaticBody2D or collider is TileMapLayer:
            break  # walls stop the shot
        excluded.append(hit["rid"])
    return hits
```

The hits come back in order of distance, because each new cast finds the next nearest object.

## Reflecting ray: lasers and ricochets

At each hit, reflect the direction about the surface normal and cast again from just off the surface.

```gdscript
extends Line2D

@export var max_bounces: int = 5
@export var max_length: float = 2000.0
@export_flags_2d_physics var mask: int = 1


func _physics_process(_delta: float) -> void:
    var space: PhysicsDirectSpaceState2D = get_world_2d().direct_space_state
    var origin: Vector2 = global_position
    var dir: Vector2 = global_transform.x.normalized()
    var left: float = max_length
    var pts: PackedVector2Array = PackedVector2Array([to_local(origin)])

    for i in max_bounces + 1:
        var query: PhysicsRayQueryParameters2D = PhysicsRayQueryParameters2D.create(origin, origin + dir * left, mask)
        var hit: Dictionary = space.intersect_ray(query)
        if hit.is_empty():
            pts.append(to_local(origin + dir * left))
            break
        var p: Vector2 = hit["position"]
        var n: Vector2 = hit["normal"]
        pts.append(to_local(p))
        left -= origin.distance_to(p)
        dir = dir.bounce(n)
        origin = p + n * 0.5  # step off the surface so the next ray does not hit it at once
    points = pts
```

`Vector2.bounce(n)` returns the mirror direction off a surface with normal `n`. `reflect(n)` mirrors across a line along `n`; it is not the same. Use `bounce` for a ricochet.

## Field-of-view check

A target is seen when it is within range, within the half-angle of the cone, and no wall blocks the line. Do the cheap tests first; cast the ray last.

```gdscript
extends Node2D

@export var view_range: float = 320.0
@export_range(1.0, 180.0) var half_angle_deg: float = 45.0
@export_flags_2d_physics var blocker_mask: int = 1


func can_see(target: Node2D) -> bool:
    var to_target: Vector2 = target.global_position - global_position
    if to_target.length_squared() > view_range * view_range:
        return false
    var facing: Vector2 = global_transform.x.normalized()
    if absf(facing.angle_to(to_target)) > deg_to_rad(half_angle_deg):
        return false
    var query: PhysicsRayQueryParameters2D = PhysicsRayQueryParameters2D.create(
        global_position, target.global_position, blocker_mask)
    return get_world_2d().direct_space_state.intersect_ray(query).is_empty()
```

The ray mask holds only blockers (walls), so the target itself never blocks the line. Check one ray to the target's center first; for large targets, add rays to its edges.

## Stuck detection

A body pushed into a wall by a teleport or a moving platform can end up inside it. `get_rest_info` with the body's own shape reports the deepest contact; push the body out along the normal.

```gdscript
extends CharacterBody2D

@onready var _shape_node: CollisionShape2D = $CollisionShape2D


func unstick() -> void:
    var query: PhysicsShapeQueryParameters2D = PhysicsShapeQueryParameters2D.new()
    query.shape = _shape_node.shape
    query.transform = _shape_node.global_transform
    query.collision_mask = collision_mask
    query.exclude = [get_rid()]
    var info: Dictionary = get_world_2d().direct_space_state.get_rest_info(query)
    if info.is_empty():
        return
    var normal: Vector2 = info["normal"]
    # Step out in small moves until free; test_move checks without moving.
    for i in 16:
        if not test_move(global_transform, Vector2.ZERO):
            return
        global_position += normal * 2.0
```

## Surface type for footsteps and decals

Do not test the collider's class to pick a footstep sound. Put a metadata value on the static body (`surface` = `"wood"`, `"metal"`) in the Inspector and read it from the hit:

```gdscript
extends Node


func surface_of(hit: Dictionary) -> StringName:
    var collider: Object = hit.get("collider")
    if collider != null and collider.has_meta(&"surface"):
        return StringName(collider.get_meta(&"surface"))
    return &"default"
```

For a `TileMapLayer`, read a custom data layer on the tile instead; the layer is one collider for all its tiles.
