Adds three layout and placement methods the base skill lacks: the drunkard's walk, Poisson disk sampling for props, and graph-first dungeon layout with an `AStar2D` reachability check. Read it for tunnels and paths, for spreading trees, rocks or enemies without clumps, or for a dungeon whose rooms must all connect.

> ← Back to [SKILL.md](../SKILL.md)

# Walks, Even Placement and Graph-First Layout

All functions take a `RandomNumberGenerator` from the caller, as SKILL.md section 1 requires. The same seed gives the same result.

## Drunkard's walk

A walker carves floor while it moves at random. It gives winding caves and tunnels with one guarantee BSP and cellular automata do not give: **every floor cell connects**, because the walker visited each one from the last.

```gdscript
# drunkard_walk.gd
class_name DrunkardWalk
extends RefCounted

const DIRS: Array[Vector2i] = [Vector2i.LEFT, Vector2i.RIGHT, Vector2i.UP, Vector2i.DOWN]


## Returns the set of floor cells. `fill` is the target share of the map (0..1).
## `max_steps` stops a walk that can never reach the target.
static func carve(rng: RandomNumberGenerator, size: Vector2i, fill: float,
        max_steps: int = 200000) -> Dictionary[Vector2i, bool]:
    var floor_cells: Dictionary[Vector2i, bool] = {}
    var target := int(size.x * size.y * clampf(fill, 0.0, 1.0))
    var pos := size / 2
    var steps := 0
    while floor_cells.size() < target and steps < max_steps:
        floor_cells[pos] = true
        var next := pos + DIRS[rng.randi_range(0, DIRS.size() - 1)]
        # Stay one cell inside the border so the map keeps an outer wall.
        if next.x >= 1 and next.y >= 1 and next.x < size.x - 1 and next.y < size.y - 1:
            pos = next
        steps += 1
    return floor_cells
```

- **Bias the walk** for long corridors: keep the last direction with a chance of 0.6 to 0.8 instead of a fresh random pick each step.
- **Several short walkers** from random floor cells give branching caves. One long walker gives one blob.
- The target count and `max_steps` both bound the loop. Keep both: a fill above the reachable area otherwise loops forever.
- Write the result with one batch call (`TileMapLayer.set_cells_terrain_connect()` with the floor cells as an `Array[Vector2i]`), not one `set_cell()` per cell.

## Poisson disk sampling (even spacing)

Plain random positions clump: two trees spawn inside each other while a large area stays empty. Poisson disk sampling places points so that no two are closer than a radius `r`, and fills the area evenly. This is Bridson's method: a background grid with cell size `r / sqrt(2)` holds at most one point per cell, so each distance test checks only nearby cells.

```gdscript
# poisson_disk.gd
class_name PoissonDisk
extends RefCounted


## Points in [0, area) with no two closer than `radius`. `tries` per active point: 30 is standard.
static func sample(rng: RandomNumberGenerator, area: Vector2, radius: float,
        tries: int = 30) -> PackedVector2Array:
    var cell := radius / sqrt(2.0)
    var grid_size := Vector2i(ceili(area.x / cell), ceili(area.y / cell))
    var grid := PackedInt32Array()
    grid.resize(grid_size.x * grid_size.y)
    grid.fill(-1)
    var points := PackedVector2Array()
    var active: Array[int] = []

    var first := Vector2(rng.randf() * area.x, rng.randf() * area.y)
    points.append(first)
    active.append(0)
    grid[_index(first, cell, grid_size)] = 0

    while not active.is_empty():
        var pick := rng.randi_range(0, active.size() - 1)
        var center := points[active[pick]]
        var placed := false
        for i in tries:
            var angle := rng.randf() * TAU
            var dist := radius * (1.0 + rng.randf())
            var candidate := center + Vector2.from_angle(angle) * dist
            if candidate.x < 0.0 or candidate.y < 0.0 or candidate.x >= area.x or candidate.y >= area.y:
                continue
            if _is_far(candidate, points, grid, cell, grid_size, radius):
                points.append(candidate)
                active.append(points.size() - 1)
                grid[_index(candidate, cell, grid_size)] = points.size() - 1
                placed = true
                break
        if not placed:
            active.remove_at(pick)
    return points


static func _index(p: Vector2, cell: float, grid_size: Vector2i) -> int:
    var c := Vector2i(int(p.x / cell), int(p.y / cell))
    return c.y * grid_size.x + c.x


static func _is_far(p: Vector2, points: PackedVector2Array, grid: PackedInt32Array,
        cell: float, grid_size: Vector2i, radius: float) -> bool:
    var c := Vector2i(int(p.x / cell), int(p.y / cell))
    for y in range(maxi(c.y - 2, 0), mini(c.y + 3, grid_size.y)):
        for x in range(maxi(c.x - 2, 0), mini(c.x + 3, grid_size.x)):
            var other := grid[y * grid_size.x + x]
            if other != -1 and points[other].distance_squared_to(p) < radius * radius:
                return false
    return true
```

- A ±2 cell search window is enough: with cell size `r / sqrt(2)`, any point closer than `r` lies within 2 cells.
- **Filter after sampling**, not inside it: drop points where the terrain is water, too steep, or a noise mask is below a threshold. That keeps the sampler generic.
- **Varying density** (dense forest core, sparse edge): sample at the smallest radius, then keep each point with a probability from a `FastNoiseLite` value at that point.
- **Jittered grid** is the cheap alternative: one point per grid cell, offset by a random amount up to `0.4 * cell`. It is O(n), never clumps, but shows the grid at low jitter. Use it for grass and small clutter; use Poisson disk for trees and spawns.
- Many instances: feed the points into a `MultiMeshInstance3D` or `MultiMeshInstance2D`, not one node each (see `godot-3d-essentials`).

## Graph first, geometry second

Generate the dungeon as a **graph** of rooms and connections, check it, and only then place tiles or meshes. A check on the graph is cheap; a check on 10,000 tiles is not, and a broken level found after placement means a full rebuild.

```gdscript
# room_graph.gd
class_name RoomGraph
extends RefCounted

var astar := AStar2D.new()


func add_room(id: int, center: Vector2) -> void:
    astar.add_point(id, center)


func connect_rooms(a: int, b: int) -> void:
    astar.connect_points(a, b, true)


## True when every room can reach `start_id`.
func all_reachable(start_id: int) -> bool:
    for id in astar.get_point_ids():
        if id != start_id and astar.get_id_path(start_id, id).is_empty():
            return false
    return true


## Rooms ordered by path length (steps) from the start. Use the last one for the exit or boss.
func by_distance(start_id: int) -> Array[int]:
    var ids: Array[int] = []
    for id in astar.get_point_ids():
        if not astar.get_id_path(start_id, id).is_empty():
            ids.append(id)
    ids.sort_custom(func(a: int, b: int) -> bool:
        return astar.get_id_path(start_id, a).size() < astar.get_id_path(start_id, b).size())
    return ids
```

- **Connect with a spanning tree, then add a few extra edges.** A pure tree has dead ends and one path; 10 to 15 percent extra edges add loops so the player is not forced to backtrack.
- **Place content by graph distance.** The farthest room is the exit; keys go on the path to the locked door, never behind it. A lock-and-key check is a path search with the locked edge disabled (`AStar2D.set_point_disabled()` on the door room, or remove the edge), then re-enabled.
- `AStar2D.get_id_path()` returns an empty array when no path exists. Since 4.6 it also returns an empty path when the start point is disabled (GH-113988, verified by unit GW10), so do not disable the start room during a check.
- For hundreds of rooms, replace the per-room path search in `all_reachable()` with one flood fill over `get_point_connections()`. The `AStar2D` form is shown because the same object then answers distance questions.
