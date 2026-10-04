# Crowds on NavigationServer, reused path queries and procedural bakes

Adds avoidance agents created directly on `NavigationServer2D` (no node per agent), path queries that reuse their parameter and result objects, a bake from parsed source geometry for procedural levels, and two version notes (async region updates since 4.5, empty AStar paths from a blocked start since 4.6). Read it when hundreds of agents cost too much as nodes, when path queries allocate in a hot loop, or when a generated level needs a navigation mesh without a region bake in the editor.

All code targets Godot 4.7. The 3D server has the same calls with `3D` names and `Vector3`.

## When to leave the nodes

A `NavigationAgent2D` per unit is right for up to a few dozen units. For hundreds of simple units (an RTS army, a zombie crowd), the node work adds up. The server offers the same pathfinding and avoidance through `RID`s. Measure first; move to the server when the profiler shows agent nodes as the cost.

## Avoidance agents on the server

Each server agent has a position, a radius, a maximum speed and a wanted velocity. Each physics tick the server computes a safe velocity that avoids other agents and calls back with it.

```gdscript
extends Node2D

@export var count: int = 200
@export var radius: float = 6.0
@export var max_speed: float = 80.0
@export var goal: Vector2 = Vector2(600.0, 300.0)

var _agents: Array[RID] = []
var _positions: PackedVector2Array = PackedVector2Array()
var _safe: PackedVector2Array = PackedVector2Array()


func _ready() -> void:
    var map: RID = get_world_2d().navigation_map
    for i in count:
        var agent: RID = NavigationServer2D.agent_create()
        NavigationServer2D.agent_set_map(agent, map)
        NavigationServer2D.agent_set_avoidance_enabled(agent, true)
        NavigationServer2D.agent_set_radius(agent, radius)
        NavigationServer2D.agent_set_max_speed(agent, max_speed)
        NavigationServer2D.agent_set_neighbor_distance(agent, radius * 8.0)
        NavigationServer2D.agent_set_max_neighbors(agent, 10)
        NavigationServer2D.agent_set_time_horizon_agents(agent, 1.5)
        NavigationServer2D.agent_set_avoidance_callback(agent, _on_safe_velocity.bind(i))
        _agents.append(agent)
        _positions.append(Vector2(randf_range(0.0, 200.0), randf_range(0.0, 600.0)))
        _safe.append(Vector2.ZERO)


func _physics_process(delta: float) -> void:
    for i in _agents.size():
        _positions[i] += _safe[i] * delta
        var wanted: Vector2 = _positions[i].direction_to(goal) * max_speed
        NavigationServer2D.agent_set_position(_agents[i], _positions[i])
        NavigationServer2D.agent_set_velocity(_agents[i], wanted)
    queue_redraw()


func _on_safe_velocity(safe_velocity: Vector2, index: int) -> void:
    _safe[index] = safe_velocity


func _draw() -> void:
    for p: Vector2 in _positions:
        draw_circle(to_local(p), radius, Color.LIGHT_GREEN)


func _exit_tree() -> void:
    for agent: RID in _agents:
        NavigationServer2D.free_rid(agent)
    _agents.clear()
```

- The callback's first argument is the safe velocity. `bind(i)` appends the agent's index, so one function serves all agents.
- The callback arrives during the physics step after `agent_set_velocity()`. Apply the safe velocity on the next tick, as above. Do not move the agent inside the callback.
- This example steers straight at the goal. For walls, give each agent a path (next section) and steer at its next path point.
- Every `agent_create()` needs a `free_rid()`. Server objects are not freed with the node.

## Reused path queries

`NavigationServer2D.map_get_path()` returns a new array per call. For many units that repath often, build one parameters object and one result object and reuse them with `query_path()`.

```gdscript
extends Node

var _params: NavigationPathQueryParameters2D = NavigationPathQueryParameters2D.new()
var _result: NavigationPathQueryResult2D = NavigationPathQueryResult2D.new()


func _ready() -> void:
    _params.map = get_viewport().world_2d.navigation_map
    _params.navigation_layers = 1
    _params.path_postprocessing = NavigationPathQueryParameters2D.PATH_POSTPROCESSING_CORRIDORFUNNEL
    _params.simplify_path = true


## Path from start to target; the returned array is a copy you may keep.
func find_path(start: Vector2, target: Vector2) -> PackedVector2Array:
    _params.start_position = start
    _params.target_position = target
    NavigationServer2D.query_path(_params, _result)
    return _result.path
```

- `simplify_path` removes points that lie almost on a straight line. It gives fewer waypoints and smoother steering in open areas.
- Spread repaths over ticks: with 200 units, repath 20 per tick, not 200 at once.
- A query on a map that has not synced yet returns an empty path. After you add regions at runtime, wait for `NavigationServer2D.map_changed`, or one physics frame, before you query.

## Bake a generated level

`NavigationRegion2D.bake_navigation_polygon(true)` bakes on a thread (SKILL.md section 1). When the level is built in code, in chunks, or without a region node, use the server in two steps:

1. Parse the scene's collision and visual geometry on the main thread with `parse_source_geometry_data()`. Parsing reads the scene tree, so it must run on the main thread.
2. Bake from the parsed data on a worker thread with `bake_from_source_geometry_data_async()`. The callback runs when the bake ends.

```gdscript
extends Node2D

signal navigation_ready

@export var level_root: Node2D
## The walkable area before obstacles are cut out, in global coordinates.
@export var bounds: Rect2 = Rect2(0.0, 0.0, 1024.0, 768.0)

@onready var _region: NavigationRegion2D = $NavigationRegion2D

var _source: NavigationMeshSourceGeometryData2D = NavigationMeshSourceGeometryData2D.new()


func rebuild() -> void:
    var poly: NavigationPolygon = _region.navigation_polygon
    if poly == null:
        poly = NavigationPolygon.new()
        poly.agent_radius = 10.0
        _region.navigation_polygon = poly
    _source.clear()
    NavigationServer2D.parse_source_geometry_data(poly, _source, level_root)
    # 2D needs a traversable outline; parsed colliders are cut out of it.
    _source.add_traversable_outline(PackedVector2Array([
        bounds.position, Vector2(bounds.end.x, bounds.position.y),
        bounds.end, Vector2(bounds.position.x, bounds.end.y),
    ]))
    NavigationServer2D.bake_from_source_geometry_data_async(poly, _source, _on_baked.bind(poly))


func _on_baked(poly: NavigationPolygon) -> void:
    # Assign again so the region pushes the new polygon to the server.
    _region.navigation_polygon = poly
    navigation_ready.emit()
```

- In 2D, parsed colliders are obstructions. The walkable area comes from traversable outlines: the polygon's own outlines (`add_outline()`) or `add_traversable_outline()` on the source data, as above. With neither, the bake is empty.
- `agent_radius` on the `NavigationPolygon` shrinks the walkable area away from walls by that distance. Match it to the agents' collision radius.
- What the parser reads is set on the `NavigationPolygon`: `parsed_geometry_type` (meshes, static colliders or both), `parsed_collision_mask`, and `source_geometry_mode` (the root node, or nodes in a group).
- Do not start a new bake on the same polygon while one runs. Check `NavigationServer2D.is_baking_navigation_polygon(poly)` first, or queue the request.

## Version notes: async region updates and AStar start points

Sources: godot-docs 4.7 branch (`upgrading_to_godot_4.5`, `upgrading_to_godot_4.6`), Godot GH-113988, checked on 4.7.2 by a headless run.

- Since 4.5, a region change (a new or edited `NavigationPolygon`, a moved region) reaches the map asynchronously. The project settings `navigation/world/region_use_async_iterations` and `navigation/world/map_use_async_iterations` are both `true` by default. A path query in the same tick as the change can still use the old mesh. Wait for `NavigationServer2D.map_changed` (or `NavigationServer3D.map_changed`) before you query, as in the bake example above.
- Since 4.6, `AStar2D`, `AStar3D` and `AStarGrid2D` return an empty path from `get_point_path()` and `get_id_path()` when the start point is disabled (`set_point_disabled()`) or solid (`AStarGrid2D.set_point_solid()`). `allow_partial_path = true` does not change that: the result is still empty. A unit that stands on a cell that just became solid (a placed wall, a closed door) gets no path. Check for an empty result and move the start to the nearest free cell first.

```gdscript
extends Node

var grid: AStarGrid2D = AStarGrid2D.new()


func path_from(start: Vector2i, goal: Vector2i) -> Array[Vector2i]:
    var path: Array[Vector2i] = grid.get_id_path(start, goal, true)
    if path.is_empty() and grid.is_in_boundsv(start) and grid.is_point_solid(start):
        # Since 4.6 a solid start gives no path. Start from a free neighbour instead.
        for offset: Vector2i in [Vector2i.LEFT, Vector2i.RIGHT, Vector2i.UP, Vector2i.DOWN]:
            var next: Vector2i = start + offset
            if grid.is_in_boundsv(next) and not grid.is_point_solid(next):
                return grid.get_id_path(next, goal, true)
    return path
```
