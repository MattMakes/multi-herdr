# Physics at scale: swept bullets and server bodies

Adds two ways to run hundreds or thousands of physics objects without one node each: bullets as data with one ray per tick, and bodies created directly on `PhysicsServer2D` with manual `free_rid()`. Read it for bullet-hell games, swarms, or debris where node count is the cost.

All code targets Godot 4.7.

## When nodes are too many

Each `Area2D` or `RigidBody2D` is a node in the tree plus a body on the server. A few hundred are fine. At thousands, the node work (signals, process callbacks, transforms) costs more than the physics. Profile first (**godot-optimization**); change the design only when the profiler shows it.

| need | design |
|---|---|
| Fast bullets that only need to know what they hit | Bullets as an array of data; one ray per bullet per tick; one draw call for all. |
| Many bodies that must collide and bounce | Bodies on `PhysicsServer2D`, no nodes; draw with `RenderingServer` or one `MultiMeshInstance2D`. |
| A few fast projectiles | Keep nodes; turn on `continuous_cd` on a `RigidBody2D`, or sweep with a ray. |

## Bullets as data

A bullet moves a long way in one tick at high speed. Casting a ray from the old position to the new position finds any hit on the way, so the bullet cannot pass through a thin wall. This is continuous collision detection by hand, and it is cheaper than an `Area2D` per bullet.

```gdscript
extends Node2D

signal bullet_hit(collider: Object, position: Vector2)

@export_flags_2d_physics var hit_mask: int = 0b0110
@export var bullet_radius: float = 3.0
@export var lifetime: float = 3.0

var _pos: PackedVector2Array = PackedVector2Array()
var _vel: PackedVector2Array = PackedVector2Array()
var _age: PackedFloat32Array = PackedFloat32Array()


func spawn(at: Vector2, velocity: Vector2) -> void:
    _pos.append(at)
    _vel.append(velocity)
    _age.append(0.0)


func _physics_process(delta: float) -> void:
    var space: PhysicsDirectSpaceState2D = get_world_2d().direct_space_state
    var query: PhysicsRayQueryParameters2D = PhysicsRayQueryParameters2D.new()
    query.collision_mask = hit_mask
    query.collide_with_areas = true
    var i: int = _pos.size() - 1
    while i >= 0:  # backwards, so removal does not skip an entry
        var from: Vector2 = _pos[i]
        var to: Vector2 = from + _vel[i] * delta
        query.from = from
        query.to = to
        var hit: Dictionary = space.intersect_ray(query)
        _age[i] += delta
        if not hit.is_empty():
            bullet_hit.emit(hit["collider"], hit["position"])
            _remove(i)
        elif _age[i] > lifetime:
            _remove(i)
        else:
            _pos[i] = to
        i -= 1
    queue_redraw()


func _remove(i: int) -> void:
    # Swap with the last entry and shrink: O(1), order does not matter.
    var last: int = _pos.size() - 1
    _pos[i] = _pos[last]
    _vel[i] = _vel[last]
    _age[i] = _age[last]
    _pos.resize(last)
    _vel.resize(last)
    _age.resize(last)


func _draw() -> void:
    for p: Vector2 in _pos:
        draw_circle(to_local(p), bullet_radius, Color.ORANGE)
```

- One reused query object avoids an allocation per bullet.
- The `_draw()` loop is fine for a few thousand circles. For more, or for textured bullets, use one `MultiMeshInstance2D` and write each instance transform.
- A ray treats the bullet as a point. For a thick bullet, use `cast_motion` with a small circle shape, at a higher cost.

## Bodies on PhysicsServer2D

When the objects must collide with each other and bounce, create bodies on the server. You get the solver without the node overhead. The server returns an `RID` for each body and shape.

```gdscript
extends Node2D

@export var count: int = 500
@export var radius: float = 4.0

var _bodies: Array[RID] = []
var _shape: RID
var _canvas_items: Array[RID] = []


func _ready() -> void:
    _shape = PhysicsServer2D.circle_shape_create()
    PhysicsServer2D.shape_set_data(_shape, radius)
    var space: RID = get_world_2d().space
    for i in count:
        var body: RID = PhysicsServer2D.body_create()
        PhysicsServer2D.body_set_mode(body, PhysicsServer2D.BODY_MODE_RIGID)
        PhysicsServer2D.body_add_shape(body, _shape)
        PhysicsServer2D.body_set_collision_layer(body, 0b1000)
        PhysicsServer2D.body_set_collision_mask(body, 0b1001)
        PhysicsServer2D.body_set_space(body, space)
        var start: Vector2 = Vector2(randf_range(0.0, 800.0), randf_range(0.0, 200.0))
        PhysicsServer2D.body_set_state(body, PhysicsServer2D.BODY_STATE_TRANSFORM, Transform2D(0.0, start))
        _bodies.append(body)

        var ci: RID = RenderingServer.canvas_item_create()
        RenderingServer.canvas_item_set_parent(ci, get_canvas_item())
        RenderingServer.canvas_item_add_circle(ci, Vector2.ZERO, radius, Color.WHITE)
        _canvas_items.append(ci)


func _physics_process(_delta: float) -> void:
    for i in _bodies.size():
        var xf: Transform2D = PhysicsServer2D.body_get_state(_bodies[i], PhysicsServer2D.BODY_STATE_TRANSFORM)
        RenderingServer.canvas_item_set_transform(_canvas_items[i], xf)


func _exit_tree() -> void:
    # Server objects are not freed with the node. Free each RID yourself.
    for body: RID in _bodies:
        PhysicsServer2D.free_rid(body)
    PhysicsServer2D.free_rid(_shape)
    for ci: RID in _canvas_items:
        RenderingServer.free_rid(ci)
    _bodies.clear()
    _canvas_items.clear()
```

Rules for server objects:

- An `RID` is not reference counted. Every `*_create()` needs one `free_rid()`. A missing call leaks memory until the program ends, and Godot prints the leak count at exit.
- Free bodies before the shapes they use.
- Hits from these bodies have no node: the `collider` of a query is null. To know which logical object was hit, call `body_attach_object_instance_id(body, some_object.get_instance_id())`, then read `collider_id` from the result.
- The 3D server (`PhysicsServer3D`) follows the same pattern with `sphere_shape_create()` and `Transform3D`.

## Fast single projectiles

For a handful of fast `RigidBody2D` or `RigidBody3D` projectiles, keep the nodes and turn on `continuous_cd`. It costs more per body, so do not turn it on for slow props. If CCD still misses at very high speed, raise `physics/common/physics_ticks_per_second` or sweep with a ray as above.
