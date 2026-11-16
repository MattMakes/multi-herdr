# Navigation, advanced

Target engine: Godot 4.7. How the NavigationServer builds and syncs maps, how to bake at run time without stalls, how to keep path queries and avoidance cheap, and when a grid or graph beats a navigation mesh.

## Use this shelf when

- The level changes at run time (building, destruction, procedural chunks) and paths must follow.
- Many agents ask for paths and the frame time spikes, most of all on unreachable targets.
- Agents of different sizes or movement types share one world.
- Agents jitter, dance between two points, walk through each other or stick in doorways.
- You must choose between a navigation mesh, `AStarGrid2D`, `AStar2D` / `AStar3D` or a flow field.

## How the engine works here

- A map (RID) holds regions, links, agents and obstacles. Each `World2D` and `World3D` owns a default map (`get_world_3d().navigation_map`). Maps are isolated. A map with no changes costs close to nothing.
- A region is one navigation mesh plus a transform. The map joins regions in two passes. First it merges edges whose two vertices fall in the same grid cells (cheap). Then it tests every edge that is still free against the others by distance and angle, inside `edge_connection_margin` (costly). Fewer free edges means a faster sync.
- Every setter is queued. The server applies the queue once per physics frame, in the middle of the tick, after `_physics_process`. A path query in `_ready()` sees an empty map. Wait for `get_tree().physics_frame` or the `map_changed` signal. The map sync runs on a background thread since 4.4 (`navigation/world/map_use_async_iterations`), and region and link updates since 4.5 (`region_use_async_iterations`). The design goal: on a weak machine the map updates less often instead of the whole game slowing down. The price is delay.
- Until 4.5 `NavigationServer2D` was a proxy of the 3D server. It is now its own server, so a 2D-only export template can leave 3D navigation out, and the two are tuned separately.
- A navigation mesh is the area where an agent centre may stand. The agent size is baked in (`agent_radius`, `agent_height`). Pathfinding ignores physics and visuals completely.
- Path search is A* over polygons, then post-processing. Its cost follows the polygon and edge count, not the world size. A search exits early when it reaches the target. An unreachable target makes it visit every connected polygon. This causes most spikes.
- Avoidance (RVO) is a separate simulation of circles or spheres. It knows nothing of the navigation mesh or of physics. It never changes the path, only the velocity.
- 3D baking has three steps: parse source geometry (main thread, reads the scene tree), voxelize and build (thread safe), assign to the region (queued sync).

## Advanced techniques

### Bake at run time without a stall

Parse once with `NavigationServer3D.parse_source_geometry_data()` on the main thread, then call `bake_from_source_geometry_data_async()` and assign the mesh with `region_set_navigation_mesh()` in the callback. The stall people see is nearly always the parse step, not the bake. Parse colliders, not visual meshes (`geometry_parsed_geometry_type = PARSED_GEOMETRY_STATIC_COLLIDERS`): reading a visual mesh pulls arrays back from the GPU and blocks the renderer. Since 4.4 the default parses both meshes and colliders (before: meshes only), so set it yourself. Limit the parse with `geometry_source_geometry_mode` and a group name.

### Skip the scene tree

Fill a `NavigationMeshSourceGeometryData3D` yourself with `add_faces()` or `add_mesh_array()` from the same arrays that build your terrain. Then no parse runs and the whole bake can run on a worker thread. The resource is reusable and can be saved, so a shipped level can load parsed geometry and skip the parse.

### Chunk large worlds

One region per grid chunk. Set `filter_baking_aabb` (2D: `baking_rect`) larger than the chunk, so it sees the neighbours' geometry, and set `border_size` to cut the result back to the chunk size. The border is not moved by `agent_radius`, so the chunk edges line up exactly and merge by vertex. In 3D the border acts on the XZ plane only. Rebake only the chunk that changed. The "Navigation Mesh Chunks" demo projects show the full setup.

### Make the bake cheaper

- Raise `cell_size` and `cell_height` as far as path quality allows (both 0.25 by default). They must match the map cell values, or merges fail with a warning.
- `sample_partition_type`: `SAMPLE_PARTITION_MONOTONE` or `SAMPLE_PARTITION_LAYERS` bake much faster than watershed on flat, blocky levels.
- Never scale source nodes. A mesh scaled to 0.001 still needs the full voxel grid.
- Give the bake boxes, not spheres or dense props.

### Limit and reuse path queries

Keep one parameter object and one result object per agent or per system. Cap the search so an unreachable target cannot scan the whole map. Drop metadata you do not read.

```gdscript
extends Node3D

var _params := NavigationPathQueryParameters3D.new()
var _result := NavigationPathQueryResult3D.new()

func _ready() -> void:
	_params.map = get_world_3d().navigation_map
	_params.metadata_flags = NavigationPathQueryParameters3D.PATH_METADATA_INCLUDE_NONE
	_params.path_search_max_polygons = 2048
	_params.simplify_path = true
	_params.simplify_epsilon = 0.3

func find_path(from: Vector3, to: Vector3) -> PackedVector3Array:
	_params.start_position = from
	_params.target_position = to
	NavigationServer3D.query_path(_params, _result)
	return _result.path
```

`path_search_max_distance` and `path_search_max_polygons` stop the search and return a path to the closest polygon found; set too low they send agents the wrong way. `path_return_max_length` and `path_return_max_radius` only clip the result (tactics games: movement range). `included_regions` and `excluded_regions` skip whole chunks early: include the start chunk and its ring of neighbours and accept a partial path toward a far target. The limits, the clips, the region filters and `path_length` on the result (path distance for AI choices, no loop needed) all arrived in 4.5. `NavigationAgent3D` has the limit and clip properties. `navigation/pathfinding/max_threads` (4) bounds parallel queries on one map.

### Do not query more than needed

- Do not set `target_position` to the player position each frame. Set it when the player has moved more than a threshold.
- Do not test "is it reachable" and then query. Query once and compare the last path point with the target.
- Stagger agents: update groups or random timers, never all in one frame.
- Call `get_next_path_position()` once per physics tick and stop when `is_navigation_finished()`.

### Pick post-processing on purpose

`PATH_POSTPROCESSING_CORRIDORFUNNEL` (default) gives the shortest path inside the corridor. `PATH_POSTPROCESSING_EDGECENTERED` suits grid games that move through cell centres. `PATH_POSTPROCESSING_NONE` shows the raw corridor: use it to debug odd detours. `simplify_path` (Ramer-Douglas-Peucker) removes points that terrain meshes add; it helps steering agents.

### Layers, costs and doors

Change the `navigation_layers` of the query, not the region, for per-actor access (keys, abilities): a query change is free, a region change resyncs the map. `enter_cost` and `travel_cost` on regions and links bias routes (mud, danger) without removing them. None of these update a path that already exists; query again.

### Sizes and movement types

One map per agent size, each with a mesh baked with its own `agent_radius` and `agent_height` from the same source geometry resource. The same pattern covers crouch, swim and fly. Queries may read any map at once; an avoidance agent moves to another map only after the next sync.

### Links need game code

`NavigationLink3D` joins two positions (ladder, jump, teleport) and attaches to the closest polygon within `link_connection_radius`. The agent only gets a path across it. Listen to `link_reached`, or read `path_types` for `PATH_SEGMENT_TYPE_LINK`, and run the jump or teleport yourself, or the agent walks through the air.

### Tune avoidance

- Set `velocity` each tick and move in `velocity_computed`; the callback runs inside the same physics frame, before the physics step, so `move_and_slide()` is safe there.
- Lower `neighbor_distance` (default 50) and `max_neighbors` (10) first; they set the cost.
- `avoidance_layers` and `avoidance_mask` split crowds. `avoidance_priority` lets a boss ignore minions. `use_3d_avoidance` is a separate simulation for flyers.
- Obstacles: `vertices` make a static wall (2D avoidance only, winding decides push out or keep in, costly to move). `radius` makes a soft dynamic obstacle that is free to move. For a moving blocker use the radius, and swap to vertices when it stops.
- An obstacle with `affect_navigation_mesh` removes baked surface; `carve_navigation_mesh` makes it ignore the agent radius. `add_projected_obstruction()` does the same from code.

### Alternatives and when each wins

| Tool | Wins when | Watch for |
| --- | --- | --- |
| Navigation mesh | free movement, large open areas, few big polygons | bake and sync cost |
| `AStarGrid2D` | tile games, per-cell cost, fast edits (`set_point_solid`) | `update()` clears all point data; `jumping_enabled` ignores weights |
| `AStar2D` / `AStar3D` | waypoint graphs, roads, room graphs | script `_compute_cost` is slow; `reserve_space()` first |
| `AStar` + `neighbor_filter_enabled` (since 4.5) | one graph, per-unit rules in `_filter_neighbor` (locked doors, size) | a script call per neighbour |
| Flow field (own code) | hundreds of agents, one target | one field per target, rebuilt on change |

Per-tile navigation polygons on a `TileMapLayer` make a map of tiny polygons that is slow to search and to sync. For free movement bake one `NavigationPolygon` with a `NavigationRegion2D`; for cell movement use `AStarGrid2D`.

## Niche but useful

- `map_get_random_point()` and `region_get_random_point()`: spawn or wander points on the mesh.
- `map_get_closest_point_owner()`: which region (room, biome) an actor stands in, with no areas.
- `map_get_iteration_id()`: cheap check for "did the map change since my path was made".
- `agent_set_paused()`: off-screen agents leave the avoidance simulation.
- `region_get_bounds()` (since 4.4): build the chunk lookup for `included_regions`.
- `region_get_iteration_id()` (since 4.5): the same change check per region.
- `navigation_engine = Dummy` under `navigation/2d/` and `navigation/3d/` (since 4.6): turn a navigation server off without a custom engine build.
- `SAMPLE_PARTITION_TRIANGULATE` for 2D baking (since 4.4): triangles instead of convex pieces.
- `AStarGrid2D.get_point_data_in_region()` (since 4.4): dump a grid area for debug or saves.
- `Performance.NAVIGATION_EDGE_MERGE_COUNT`, `NAVIGATION_EDGE_CONNECTION_COUNT` and `NAVIGATION_EDGE_FREE_COUNT`: merged-by-vertex must be far above the connection count. Since 4.5 `NAVIGATION_2D_*` and `NAVIGATION_3D_*` variants split the numbers per server.
- `Performance.TIME_NAVIGATION_PROCESS`: sync plus avoidance time. It is part of the physics time and does not include path queries.

## Traps

- `map_force_update()` is deprecated; it does not work with async sync. Wait a physics frame.
- Vertex merge needs exact positions. Imported meshes with float noise and overlapping regions fail to merge. Since 4.5 lower `merge_rasterizer_cell_scale` if merge errors remain.
- Region scale is ignored. Rebake when the source scale changes.
- An agent with avoidance but no `target_position` gets a zero safe velocity.
- A fast agent can overshoot `path_desired_distance` and turn back. Raise the distances.
- Debug drawing is node based; server-only regions are not drawn. Debug calls are debug-build only.
- GridMap: bake with the map cell size and lower `agent_radius` and `region_min_size` for small cells.
- Since 4.5 `NavigationServer2D` avoidance callbacks pass a `Vector2`, not a `Vector3`.
- Outdated: the 2020 design post uses a `Navigation` node and `get_next_location()`; neither exists in 4.x. Its goals (region streaming, run-time baking, thread safety) still explain the server design.

## Version notes

- 4.4: async map sync. Path search uses a heap for open polygons (faster queries). `query_path()` takes an optional callback. `navigation/pathfinding/max_threads`. Bakes parse colliders by default. Obstacles follow their node transform. `AStarGrid2D` jumping is faster.
- 4.5: dedicated 2D server. Async region and link sync by default. Regions merge their own polygons first, then the map merges free edges. Path query limits and region filters. Export templates can be built without navigation.
- 4.6: `AStar2D`, `AStar3D` and `AStarGrid2D` return an empty path when the start point is disabled or solid. The navigation engine is selectable, with `Dummy` to disable it.
- 4.7: no navigation entry in the release page or the upgrade guide.

## Related skills

- `godot-ai-navigation`: agent setup, steering, patrol and chase patterns.
- `godot-limboai` and `godot-beehave`: behaviour trees that drive these agents.
- `godot-multithreading`: worker threads for your own bake or flow field jobs.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_optimizing_performance.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationmeshes.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationservers.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationpathqueryobjects.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationagents.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationobstacles.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationlinks.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.5.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.6.html
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-4-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/
- https://godotengine.org/article/navigation-server-godot-4-0/
