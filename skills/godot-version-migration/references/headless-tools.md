# Headless tools for a migration

Read this when you need to resave every scene, or to turn `TileMap` nodes
into `TileMapLayer` nodes, without the editor GUI. Both scripts are own code,
tested on Godot 4.7.2 (`--headless`). Put a script in the project (for
example `res://tools/`), run it, then delete it or keep it out of exports.

## Resave every scene and resource

The editor's **Project > Tools > Upgrade Project Files** has no command-line
flag. This script does the same work: it loads each `.tscn` and `.tres` and
saves it in the format of the running engine. Measured on 4.7.2: a converted
3.x scene (`format=2`, `ExtResource( 1 )`) was written as `format=3` with
`ExtResource("1")`. It did not add the 4.6 per-node `unique_id` values. A scene packed
from instantiated nodes (as the TileMap converter below does) gets them.

```gdscript
extends SceneTree

## Loads every .tscn and .tres under res:// and saves it again in the
## format of the running engine. Prints each file and quits 1 on a failure.
func _init() -> void:
	var failed := 0
	for path in _find_files("res://", ["tscn", "tres"]):
		var res := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
		if res == null:
			printerr("LOAD FAILED: ", path)
			failed += 1
			continue
		var err := ResourceSaver.save(res, path)
		if err != OK:
			printerr("SAVE FAILED: ", path, " error ", err)
			failed += 1
		else:
			print("RESAVED: ", path)
	quit(1 if failed > 0 else 0)


func _find_files(dir_path: String, extensions: Array[String]) -> PackedStringArray:
	var found := PackedStringArray()
	for sub in DirAccess.get_directories_at(dir_path):
		if sub.begins_with(".") or sub == "addons":
			continue
		found.append_array(_find_files(dir_path.path_join(sub), extensions))
	for file in DirAccess.get_files_at(dir_path):
		if file.get_extension() in extensions:
			found.append(dir_path.path_join(file))
	return found
```

```bash
G="${GODOT_PATH:-/Applications/Godot.app/Contents/MacOS/Godot}"
"$G" --headless --path . -s res://tools/resave.gd
```

Rules:

- Fix every parse error first. A scene whose script fails to load is saved
  without that script's data, or fails to load.
- The script skips `addons/`. An addon is upgraded by its own release.
- Commit the resave alone, so the format diff does not hide real changes.

## TileMap to TileMapLayer

`TileMap` is deprecated since 4.3. The editor's "Extract TileMap layers as
individual TileMapLayer nodes" is a GUI action. This script replaces each
`TileMap` that a scene owns with a `Node2D` of the same name, transform,
`z_index` and visibility. That node holds one `TileMapLayer` per layer, named
after the layer, with the same tile set, cells, `enabled`, `modulate`,
Y-sort settings, `z_index` and `rendering_quadrant_size`. Measured on 4.7.2:
a 2-layer fixture came back with the same cells, atlas coordinates and
position.

```gdscript
extends SceneTree
# api-check: allow TileMap (this converter reads the deprecated node on purpose)

## Usage: Godot --headless --path . -s res://tilemap_to_layers.gd -- res://a.tscn [res://b.tscn ...]
## Replaces each TileMap in each scene with a Node2D of the same name and
## transform that holds one TileMapLayer per TileMap layer. Saves the scene in
## place. Quits 1 if a scene fails or a TileMap has a script (convert those by hand).
func _init() -> void:
	var failed := 0
	for path in OS.get_cmdline_user_args():
		failed += _convert_scene(path)
	quit(1 if failed > 0 else 0)


func _convert_scene(path: String) -> int:
	var packed := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE) as PackedScene
	if packed == null:
		printerr("LOAD FAILED: ", path)
		return 1
	var root := packed.instantiate(PackedScene.GEN_EDIT_STATE_INSTANCE)
	var maps: Array[TileMap] = []
	_collect_tilemaps(root, root, maps)
	var failed := 0
	for map in maps:
		if map.get_script() != null:
			printerr("SKIPPED (has a script): ", path, " ", root.get_path_to(map))
			failed += 1
			continue
		_replace_tilemap(map, root)
		print("CONVERTED: ", path, " ", map.name, " (", map.get_layers_count(), " layers)")
		map.free()
	var out := PackedScene.new()
	var err := out.pack(root)
	if err == OK:
		err = ResourceSaver.save(out, path)
	root.free()
	if err != OK:
		printerr("SAVE FAILED: ", path, " error ", err)
		failed += 1
	return failed


func _collect_tilemaps(node: Node, root: Node, maps: Array[TileMap]) -> void:
	# Only nodes that this scene owns. Nodes inside an instanced sub-scene belong to that scene.
	if node is TileMap and (node == root or node.owner == root):
		maps.append(node)
	for child in node.get_children():
		_collect_tilemaps(child, root, maps)


func _replace_tilemap(map: TileMap, root: Node) -> void:
	var holder := Node2D.new()
	holder.transform = map.transform
	holder.z_index = map.z_index
	holder.visible = map.visible
	for layer in map.get_layers_count():
		var out := TileMapLayer.new()
		out.name = map.get_layer_name(layer) if map.get_layer_name(layer) != "" else "Layer%d" % layer
		out.tile_set = map.tile_set
		out.enabled = map.is_layer_enabled(layer)
		out.modulate = map.get_layer_modulate(layer)
		out.y_sort_enabled = map.is_layer_y_sort_enabled(layer)
		out.y_sort_origin = map.get_layer_y_sort_origin(layer)
		out.z_index = map.get_layer_z_index(layer)
		out.rendering_quadrant_size = map.rendering_quadrant_size
		for cell in map.get_used_cells(layer):
			out.set_cell(cell, map.get_cell_source_id(layer, cell),
				map.get_cell_atlas_coords(layer, cell), map.get_cell_alternative_tile(layer, cell))
		holder.add_child(out)
	var map_name := map.name
	map.replace_by(holder, true)
	holder.name = map_name
	for child in holder.get_children():
		child.owner = root
	if holder != root:
		holder.owner = root
```

```bash
"$G" --headless --path . -s res://tools/tilemap_to_layers.gd -- res://levels/level_1.tscn res://levels/level_2.tscn
```

Limits. Check each one before you report `DONE:`:

- A `TileMap` with a script is skipped (exit 1). Its script calls `TileMap`
  methods with a layer argument; port it by hand to `TileMapLayer` (no layer
  argument) and convert that scene by hand.
- Code elsewhere that does `$World.set_cell(0, ...)` must change to
  `$World/Ground.set_cell(...)`. Search for every `get_node` path to the old
  map.
- Navigation, collision and occlusion settings on the old layers that are not
  listed above keep their `TileMapLayer` defaults. Compare them in the scene
  text.
- A `TileMap` inside an instanced sub-scene belongs to that sub-scene.
  Convert the sub-scene file.
