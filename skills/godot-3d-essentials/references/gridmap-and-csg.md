Adds level building with `GridMap` (MeshLibrary setup, cells and rotations from code, logic marker tiles, navigation) and with CSG (greybox rules, baking CSG to a static mesh and collision). Read it when a 3D level is built from modular tiles, or when a CSG greybox must become shipping geometry. The 4.7 octant queries and CSG smoothing are in [godot-4.7-additions.md](godot-4.7-additions.md).

> ← Back to [SKILL.md](../SKILL.md)

# GridMap and CSG Level Building

## MeshLibrary: the tile set of a GridMap

A `GridMap` places items from a `MeshLibrary`. Build the library from a source scene:

1. Make a scene with one child per tile: a `MeshInstance3D` named after the tile, with a `StaticBody3D` + `CollisionShape3D` child for collision, and optionally a `NavigationRegion3D` child for navigation.
2. Scene > Export As... > MeshLibrary. Save it as `.tres` (text, reviewable) and commit the source scene too.
3. Re-export after you change the source scene. The library is a copy, not a link.

Check that each item has collision before you ship. An item without shapes is visible but the player falls through it:

```gdscript
# mesh_library_check.gd
class_name MeshLibraryCheck
extends RefCounted


## Returns the names of items without collision shapes. Run it in a test.
static func items_without_collision(library: MeshLibrary) -> PackedStringArray:
    var missing := PackedStringArray()
    for id in library.get_item_list():
        if library.get_item_shapes(id).is_empty():
            missing.append(library.get_item_name(id))
    return missing
```

Decorative items (grass tufts, wall posters) have no collision on purpose. Keep their names in an allow list in the test.

## Cells from code

```gdscript
# grid_builder.gd
class_name GridBuilder
extends GridMap


## Places `item` at `cell`, turned `quarter_turns` times around +Y.
func place(cell: Vector3i, item: int, quarter_turns: int = 0) -> void:
    var basis := Basis(Vector3.UP, quarter_turns * PI / 2.0)
    set_cell_item(cell, item, get_orthogonal_index_from_basis(basis))


func clear_cell(cell: Vector3i) -> void:
    set_cell_item(cell, GridMap.INVALID_CELL_ITEM)


## The cell under a world position, for example a raycast hit.
func cell_at(world_pos: Vector3) -> Vector3i:
    return local_to_map(to_local(world_pos))


## World position of a cell's center.
func cell_center(cell: Vector3i) -> Vector3:
    return to_global(map_to_local(cell))


## Quarter turns of a placed cell around +Y (0..3), or -1 when empty.
func turns_at(cell: Vector3i) -> int:
    if get_cell_item(cell) == GridMap.INVALID_CELL_ITEM:
        return -1
    var basis := get_basis_with_orthogonal_index(get_cell_item_orientation(cell))
    var angle := atan2(basis.x.z, basis.x.x)
    return posmod(roundi(-angle / (PI / 2.0)), 4)
```

- The orientation argument of `set_cell_item()` is an **orthogonal index** (0 to 23), not an angle. Always convert from a `Basis` with `get_orthogonal_index_from_basis()`; never hard-code the index numbers.
- Set `cell_size` once, before you place tiles, to match the library's tile size. Changing it later does not move existing tiles to match: the level looks broken.
- `cell_center_x/y/z` decide whether a cell's origin is its center or its corner. Keep the defaults unless your tiles were modeled with the origin at a corner.
- Batch writes are fine: each `set_cell_item()` marks an octant dirty, and the GridMap rebuilds dirty octants once.

## Marker tiles for gameplay objects

A GridMap holds meshes, collision and navigation, not scripts. To place spawn points, chests or triggers in the same grid editor, add invisible **marker items** to the library (a small colored cube named `SPAWN_ENEMY`), paint them, and replace them with scenes at start-up:

```gdscript
# grid_marker_spawner.gd
class_name GridMarkerSpawner
extends Node3D

@export var grid: GridMap
## MeshLibrary item name -> scene to spawn there.
@export var spawns: Dictionary[String, PackedScene] = {}


func _ready() -> void:
    var library := grid.mesh_library
    for id in library.get_item_list():
        var scene: PackedScene = spawns.get(library.get_item_name(id))
        if scene == null:
            continue
        for cell in grid.get_used_cells_by_item(id):
            var node := scene.instantiate() as Node3D
            add_child(node)
            node.global_position = grid.to_global(grid.map_to_local(cell))
            grid.set_cell_item(cell, GridMap.INVALID_CELL_ITEM)
```

Look items up by **name**, not by id. Ids change when the library is re-exported with items in a new order.

## Navigation on a GridMap

A GridMap does not make a navigation mesh by itself. Two working setups:

- **Bake.** Put the GridMap under a `NavigationRegion3D` whose `NavigationMesh` parses static colliders or meshes (`geometry_parsed_geometry_type`), and bake after the level is built: `bake_navigation_mesh()` (runs on a thread by default). Re-bake after large runtime edits, not after each cell.
- **Per item.** Give library items a navigation mesh (the `NavigationRegion3D` child in the source scene), and keep `bake_navigation` on the GridMap. Each placed cell then brings its own navigation, with no bake step. Edges between cells must line up exactly, or agents cannot cross.

## CSG: greybox only

CSG nodes (`CSGBox3D`, `CSGCylinder3D`, `CSGPolygon3D`, `CSGMesh3D` under a `CSGCombiner3D`) are fast to edit and slow to run. Every change to a CSG node in the tree rebuilds the combined mesh on the CPU.

- Do not move, rotate or scale CSG nodes during gameplay. A sliding door made of CSG rebuilds the whole combiner every frame.
- A `CSGMesh3D` with a custom mesh needs a closed (manifold) mesh. Holes or self-intersections make the boolean result wrong or empty.
- Ship baked geometry, not live CSG.

## Bake CSG to a static mesh

The editor has a CSG menu that bakes the selected root to a mesh instance and a collision shape. In code (an `EditorScript`, or a headless tool script that saves the scene), use the CSG bake methods on the **root** CSG node:

```gdscript
# csg_baker.gd
class_name CsgBaker
extends RefCounted


## Replaces a CSG root with a MeshInstance3D + StaticBody3D. Call on the main thread
## after the CSG tree has updated at least once (one frame after it entered the tree).
static func bake(csg_root: CSGShape3D) -> MeshInstance3D:
    var mesh := csg_root.bake_static_mesh()
    if mesh == null:
        push_error("CsgBaker: %s produced no mesh; is it a root CSG node with geometry?" % csg_root.name)
        return null
    var mesh_node := MeshInstance3D.new()
    mesh_node.name = csg_root.name + "_Baked"
    mesh_node.mesh = mesh
    mesh_node.transform = csg_root.transform
    var shape := csg_root.bake_collision_shape()
    if shape != null:
        var body := StaticBody3D.new()
        var collider := CollisionShape3D.new()
        collider.shape = shape
        body.add_child(collider)
        mesh_node.add_child(body)
    var parent := csg_root.get_parent()
    parent.add_child(mesh_node)
    # Saving with PackedScene needs owners set on new nodes (see godot-scene-files).
    if csg_root.owner != null:
        mesh_node.owner = csg_root.owner
        for child in mesh_node.find_children("*", "", true, false):
            child.owner = csg_root.owner
    csg_root.queue_free()
    return mesh_node
```

- Bake only after the CSG has computed its mesh. Wait one frame first (`await get_tree().process_frame`). On 4.7.2, in a test, `bake_static_mesh()` called in the same frame as `add_child()` returned `null`; one frame later it returned the mesh. The same applies after you change a CSG property.
- `bake_collision_shape()` returns a `ConcavePolygonShape3D`: correct for static level geometry, not for moving bodies.
- Materials carry over per surface. Check the baked mesh's surface count against the CSG's material count if a surface turns up untextured.
- The baked mesh has no lightmap UV2. Before you bake lightmaps, unwrap it (`ArrayMesh.lightmap_unwrap()`) or re-import the geometry from a modeling tool.
