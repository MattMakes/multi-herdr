Adds runtime `TileMapLayer` work the editor guide in tilemap.md does not cover: writing cells from code in batches, terrain painting from code, pattern stamps, per-cell state such as tile health, per-cell visual overrides, collision and navigation after edits, and isometric Y-sort. Read it when code (procgen, digging, building, destruction) changes a tile map while the game runs.

> ← Back to [SKILL.md](../SKILL.md)

# TileMapLayer at Runtime

## Cell API in one table

| Task | Call | Note |
|---|---|---|
| World position → cell | `layer.local_to_map(layer.to_local(global_pos))` | `local_to_map` takes **layer-local** coordinates. Passing a global position works only while the layer sits at the origin. |
| Cell → world position (cell center) | `layer.to_global(layer.map_to_local(cell))` | |
| Place one tile | `layer.set_cell(cell, source_id, atlas_coords, alternative_tile)` | `source_id` is the atlas id in the TileSet, often `0`, but not always. Read it in the TileSet panel. |
| Remove one tile | `layer.erase_cell(cell)` | Same as `set_cell(cell, -1)`. |
| Read a tile | `get_cell_source_id(cell)`, `get_cell_atlas_coords(cell)`, `get_cell_tile_data(cell)` | `-1` / `Vector2i(-1, -1)` / `null` for an empty cell. |
| Neighbors | `get_surrounding_cells(cell)`, `get_neighbor_cell(cell, TileSet.CELL_NEIGHBOR_RIGHT_SIDE)` | Correct for square, isometric and hex shapes. Never add `Vector2i(1, 0)` on hex or iso maps. |
| All placed tiles | `get_used_cells()`, `get_used_cells_by_id(source_id, atlas_coords)` | |

## Write many cells at once

Each `set_cell()` marks the layer dirty, and the layer rebuilds its render and physics quadrants at the end of the frame. Thousands of calls in one frame are still slow in script, so prefer the batch calls:

- **Terrain from code:** `set_cells_terrain_connect(cells, terrain_set, terrain)` paints a whole `Array[Vector2i]` and picks the right edge and corner tiles, the same as the editor's Terrain Connect tool. `set_cells_terrain_path(path, terrain_set, terrain)` connects cells only along the path order (roads, rivers, fences).
- **Prefab stamps:** build a room in the editor once, copy it as a `TileMapPattern`, and stamp it anywhere.

```gdscript
# tile_stamper.gd
class_name TileStamper
extends RefCounted


## Copies the cells inside `rect` from `source` as a pattern.
static func capture(source: TileMapLayer, rect: Rect2i) -> TileMapPattern:
    var cells: Array[Vector2i] = []
    for y in range(rect.position.y, rect.end.y):
        for x in range(rect.position.x, rect.end.x):
            var cell := Vector2i(x, y)
            if source.get_cell_source_id(cell) != -1:
                cells.append(cell)
    return source.get_pattern(cells)


## Stamps the pattern with its top-left at `at`. Keeps source ids, atlas coords and alternatives.
static func stamp(target: TileMapLayer, pattern: TileMapPattern, at: Vector2i) -> void:
    target.set_pattern(at, pattern)
```

- A pattern made with `get_pattern()` is normalized so its top-left used cell is `(0, 0)`.
- Store prefab patterns as resources: `ResourceSaver.save(pattern, "res://rooms/shop.tres")`. A procgen step then loads a list of room patterns and stamps them.
- On hex and iso maps, use `layer.map_pattern(at, coords_in_pattern, pattern)` to find where a pattern cell lands, because offset rows shift.

## Per-cell state: keep it outside `TileData`

`get_cell_tile_data(cell)` returns the **shared** `TileData` of that tile type. Every cell that shows the same atlas tile returns the same object. Writing a value into it (`set_custom_data()`) changes every such cell on the map.

So per-cell state (a wall's remaining health, a crop's growth) lives in a dictionary keyed by cell. The TileSet's custom data holds only the per-type defaults.

```gdscript
# destructible_layer.gd
class_name DestructibleLayer
extends TileMapLayer

signal tile_destroyed(cell: Vector2i)

## Custom data layer in the TileSet, type int. 0 or missing = indestructible.
const HEALTH_KEY := "max_health"

var _health: Dictionary[Vector2i, int] = {}
var _cracked: Dictionary[Vector2i, bool] = {}


## Returns true when the hit destroyed the tile.
func damage_cell(cell: Vector2i, amount: int) -> bool:
    var data := get_cell_tile_data(cell)
    if data == null:
        return false
    var max_health: int = data.get_custom_data(HEALTH_KEY)
    if max_health <= 0:
        return false
    var left: int = _health.get(cell, max_health) - amount
    if left <= 0:
        _health.erase(cell)
        _cracked.erase(cell)
        erase_cell(cell)
        tile_destroyed.emit(cell)
        return true
    _health[cell] = left
    if left * 2 <= max_health and not _cracked.has(cell):
        _cracked[cell] = true
        notify_runtime_tile_data_update()
    return false


# Runtime visual override: only cells that answer true get a private TileData copy.
func _use_tile_data_runtime_update(coords: Vector2i) -> bool:
    return _cracked.has(coords)


func _tile_data_runtime_update(_coords: Vector2i, tile_data: TileData) -> void:
    tile_data.modulate = Color(1.0, 0.6, 0.6)
```

- `_tile_data_runtime_update()` receives a **copy** of the `TileData` for that one cell, so changing `modulate`, `flip_h` or collision polygons there is safe. Call `notify_runtime_tile_data_update()` after the state changes, or the override does not re-run.
- Runtime updates cost time per affected cell. Use them for a few marked cells (cracks, highlights), not for the whole map. For a large state change, swap the tile with `set_cell()` to a "cracked" atlas tile instead.
- Save `_health` (and any other per-cell dictionary) with the level, keyed as `[x, y]` pairs, since JSON keys must be strings.

## Collision and navigation after edits

- **Collision.** The layer rebuilds collision quadrants at the end of the frame after a change. Code that needs the new collision in the same frame (a raycast right after digging) calls `update_internals()` first.
- **Which cell was hit.** `get_coords_for_body_rid(rid)` maps a collision body RID back to a cell (for example from a `KinematicCollision2D.get_collider_rid()`). Since 4.5, colliders merge per physics quadrant (`physics_quadrant_size`, default 16), so the result can be any cell of that quadrant. Set `physics_quadrant_size = 1` on a layer where the exact cell matters (destructible walls); it costs one body per tile (verified by unit GW10).
- **Moving tiles.** `use_kinematic_bodies = true` makes tile bodies kinematic, for a layer that moves (an elevator platform made of tiles), so bodies on it are carried along.
- **Navigation.** A layer with navigation polygons in its TileSet rebuilds its navigation after edits. With many edits per second (a digging game), turn `navigation_enabled` off on the edited layer, and bake one `NavigationRegion2D` from the map in batches instead. See `godot-ai-navigation`.

## Cache hot custom-data reads

Reading `get_cell_tile_data(cell).get_custom_data("friction")` each physics frame for each actor is a lookup chain. Cache the value per cell, and invalidate the entry in the same function that edits the cell:

```gdscript
# tile_property_cache.gd
class_name TilePropertyCache
extends RefCounted

var _layer: TileMapLayer
var _key: String
var _default: Variant
var _cache: Dictionary[Vector2i, Variant] = {}


func _init(layer: TileMapLayer, key: String, default_value: Variant) -> void:
    _layer = layer
    _key = key
    _default = default_value


func get_at(cell: Vector2i) -> Variant:
    if _cache.has(cell):
        return _cache[cell]
    var data := _layer.get_cell_tile_data(cell)
    var value: Variant = _default
    if data != null and data.has_custom_data(_key):
        value = data.get_custom_data(_key)
    _cache[cell] = value
    return value


## Call from the code that runs set_cell() / erase_cell() on this cell.
func invalidate(cell: Vector2i) -> void:
    _cache.erase(cell)


## Call after a batch write (pattern stamp, terrain paint, level load).
func clear() -> void:
    _cache.clear()
```

Do not rely on the layer's `changed` signal for this. On 4.7.2 it is not emitted at the `set_cell()` call: in a test, five edits made before the first frame emitted it five times at the end of that frame, and one more edit made during a frame did not emit it within the next two frames. A cache cleared by `changed` serves stale values in between.

## Isometric and Y-sort

- For an isometric map where actors walk behind tall tiles, set `y_sort_enabled = true` on the `TileMapLayer` **and** on its parent, and put the actors under the same parent. Only nodes under one Y-sorted parent sort against each other.
- A tall tile (a tree, a wall) sorts by its origin. Set `y_sort_origin` on that tile's `TileData` in the TileSet editor to the tile's foot, or the actor appears in front of the tree while standing behind it.
- Keep the floor on a separate layer with Y-sort off. A Y-sorted floor costs sort time and gains nothing.

## Several layers, one map

Keep ground, decoration and collision-only tiles on sibling `TileMapLayer` nodes that share one `TileSet`. Edits then touch only the layer that changes. Note that `enabled = false` turns a layer off completely, collision included. Moving all layers together means moving their parent, never each layer.
