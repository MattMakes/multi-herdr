Adds a shaped-item grid inventory (Diablo / Resident Evil style): footprints, rotation, placement and first-fit search. Read it when items take more than one cell.

> ← Back to [SKILL.md](../SKILL.md)

# Grid Inventory (Shaped Items)

The slot array in SKILL.md is the wrong skeleton when an item covers several cells. A grid inventory stores **which item covers each cell** and **where each item is anchored**. Everything else follows from those two maps.

## Decisions

| Decision | Take | Why |
|---|---|---|
| Shape | `Array[Vector2i]` of cell offsets from the anchor | A rectangle is one case; L and T shapes need offsets. A `Vector2i` size cannot express them. |
| Cell map | `Dictionary[Vector2i, GridEntry]` (typed dictionaries are 4.4+) | O(1) lookup per cell and no nested arrays to resize. Empty cells are absent keys. |
| Placed item record | a `RefCounted` `GridEntry` (item, anchor, rotation) | All cells of one item point at the same entry, so remove and move are one lookup. |
| Rotation | quarter turns, 0 to 3, applied to the offsets | Store the turn count, not rotated offsets, so the save file stays small and the item data stays shared. |
| Stacking | only for 1-cell items with `max_stack_size > 1` | A stack of shaped items has no clear footprint. |

Normalize the offsets after each rotation so the smallest x and y are 0. Otherwise a rotated item hangs off the top or left edge of its anchor.

## Item data

Extend the item Resource with a footprint. The default single cell keeps every 1x1 item valid.

```gdscript
# grid_item_data.gd
class_name GridItemData
extends Resource

@export var id: StringName = &""
@export var display_name: String = ""
@export var icon: Texture2D
## Cell offsets from the anchor. (0, 0) must be one of them.
@export var footprint: Array[Vector2i] = [Vector2i.ZERO]
@export var can_rotate: bool = true


## Offsets after `turns` clockwise quarter turns, shifted so min x and min y are 0.
func get_cells(turns: int) -> Array[Vector2i]:
    var out: Array[Vector2i] = []
    var min_corner := Vector2i(1 << 30, 1 << 30)
    for offset in footprint:
        var c := offset
        for i in posmod(turns, 4):
            c = Vector2i(-c.y, c.x)
        out.append(c)
        min_corner = Vector2i(mini(min_corner.x, c.x), mini(min_corner.y, c.y))
    for i in out.size():
        out[i] -= min_corner
    return out
```

## The grid

```gdscript
# grid_inventory.gd
class_name GridInventory
extends RefCounted

signal changed

class GridEntry:
    extends RefCounted
    var item: GridItemData
    var anchor: Vector2i
    var turns: int

var size: Vector2i
var _cells: Dictionary[Vector2i, GridEntry] = {}
var _entries: Array[GridEntry] = []


func _init(grid_size: Vector2i) -> void:
    size = grid_size


func can_place(item: GridItemData, anchor: Vector2i, turns: int, ignore: GridEntry = null) -> bool:
    for offset in item.get_cells(turns):
        var cell := anchor + offset
        if cell.x < 0 or cell.y < 0 or cell.x >= size.x or cell.y >= size.y:
            return false
        var other: GridEntry = _cells.get(cell)
        if other != null and other != ignore:
            return false
    return true


## Places the item and returns its entry, or null when it does not fit.
func place(item: GridItemData, anchor: Vector2i, turns: int = 0) -> GridEntry:
    if not can_place(item, anchor, turns):
        return null
    var entry := GridEntry.new()
    entry.item = item
    entry.anchor = anchor
    entry.turns = turns
    _occupy(entry)
    _entries.append(entry)
    changed.emit()
    return entry


func remove(entry: GridEntry) -> void:
    _vacate(entry)
    _entries.erase(entry)
    changed.emit()


## Moves or rotates an entry in one step. The item may overlap its own old cells.
func move(entry: GridEntry, anchor: Vector2i, turns: int) -> bool:
    if not can_place(entry.item, anchor, turns, entry):
        return false
    _vacate(entry)
    entry.anchor = anchor
    entry.turns = turns
    _occupy(entry)
    changed.emit()
    return true


## First fit, row by row, trying every allowed rotation. Returns null when full.
func auto_place(item: GridItemData) -> GridEntry:
    var turn_options: Array[int] = [0, 1, 2, 3] if item.can_rotate else [0]
    for y in size.y:
        for x in size.x:
            for turns in turn_options:
                if can_place(item, Vector2i(x, y), turns):
                    return place(item, Vector2i(x, y), turns)
    return null


func entry_at(cell: Vector2i) -> GridEntry:
    return _cells.get(cell)


func get_entries() -> Array[GridEntry]:
    return _entries.duplicate()


func _occupy(entry: GridEntry) -> void:
    for offset in entry.item.get_cells(entry.turns):
        _cells[entry.anchor + offset] = entry


func _vacate(entry: GridEntry) -> void:
    for offset in entry.item.get_cells(entry.turns):
        _cells.erase(entry.anchor + offset)
```

## Notes

- **Emit one `changed` per operation**, the same rule as the slot inventory.
- **Drag preview.** While the player drags, call `can_place(item, hovered_cell, turns, dragged_entry)` each time the hovered cell changes, and tint the target cells. Pass the dragged entry as `ignore`, or the item blocks its own move.
- **Rotate input during a drag** changes `turns` only. Commit with `move()` on drop.
- **First fit is greedy.** It can report "full" while a different arrangement fits. That matches player expectations in most games. Do not add a packing solver unless the design asks for one.
- **Save** each entry as `{ "id": item.id, "x": anchor.x, "y": anchor.y, "turns": turns }`. On load, call `place()` for each entry and log any entry that no longer fits (a footprint changed between versions).
- **UI.** Draw the grid as one `Control` with `_draw()` for the cell backgrounds, and one `TextureRect` per entry, sized `cell_px * footprint bounds` and rotated with `rotation` and `pivot_offset`. A per-cell `Button` grid does not show shaped icons well.
