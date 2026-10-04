# Worked example: one scene, built by script and edited as text

Every file and command here was run on Godot 4.7.2 (macOS, 2026-10-04) in a
scratch project. `gd` and `$LOGS` come from `godot-build-verify`,
`references/commands.md`, "Common setup":

```bash
GH="$PWD/.godot/horch-home"
LOGS="$GH/logs"
mkdir -p "$LOGS"
gd() { HOME="$GH" "$GODOT" --headless --path "$PWD" "$@"; }
```

The project holds `project.godot` and `levels/level.gd`:

```gdscript
extends Node2D


func _on_timer_timeout() -> void:
	print("CONNECTED timeout reached")
```

## 1. Build a new scene by script

Save as `.godot/horch-home/build_level.gd`:

```gdscript
# Builds res://levels/level_1.tscn. Every node below the root gets owner = root.
extends SceneTree


func _init() -> void:
	var root := Node2D.new()
	root.name = "Level1"
	root.set_script(load("res://levels/level.gd"))

	var spawn := Marker2D.new()
	spawn.name = "Spawn"
	spawn.position = Vector2(64, 32)
	root.add_child(spawn)
	spawn.owner = root

	var wall := StaticBody2D.new()
	wall.name = "Wall"
	root.add_child(wall)
	wall.owner = root
	var shape := CollisionShape2D.new()
	shape.name = "Shape"
	var rect := RectangleShape2D.new()
	rect.size = Vector2(32, 128)
	shape.shape = rect
	wall.add_child(shape)
	shape.owner = root

	var scene := PackedScene.new()
	var err := scene.pack(root)
	if err == OK:
		err = ResourceSaver.save(scene, "res://levels/level_1.tscn")
	root.free()
	if err != OK:
		printerr("BUILD_FAIL ", error_string(err))
		quit(1)
		return
	print("BUILD_OK res://levels/level_1.tscn")
	quit(0)
```

```bash
gd -s "$PWD/.godot/horch-home/build_level.gd"   # prints BUILD_OK, exit 0
```

The saved file (ids and `unique_id` values are random; yours differ):

```text
[gd_scene format=3]

[ext_resource type="Script" path="res://levels/level.gd" id="1_vn7o3"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_jirdo"]
size = Vector2(32, 128)

[node name="Level1" type="Node2D" unique_id=1590365356]
script = ExtResource("1_vn7o3")

[node name="Spawn" type="Marker2D" parent="." unique_id=1349167531]
position = Vector2(64, 32)

[node name="Wall" type="StaticBody2D" parent="." unique_id=49164077]

[node name="Shape" type="CollisionShape2D" parent="Wall" unique_id=559493353]
shape = SubResource("RectangleShape2D_jirdo")
```

Measured:

- No `uid=` in the header and none on the `ext_resource`, even though
  `levels/level.gd.uid` exists. `--import` afterwards leaves the file as it is.
- A node added without `owner = root` is not in the file. No error is printed.
- Running the builder again writes new `unique_id` values on every node.

## 2. Edit it as text

Three edits: a property, a new shape with its own `sub_resource`, and a timer
with a signal connection. No `uid`, no `unique_id`, new ids unique in the file.

```diff
 [sub_resource type="RectangleShape2D" id="RectangleShape2D_jirdo"]
 size = Vector2(32, 128)

+[sub_resource type="RectangleShape2D" id="RectangleShape2D_wall2"]
+size = Vector2(128, 16)
+
 [node name="Level1" type="Node2D" unique_id=1590365356]
 script = ExtResource("1_vn7o3")

 [node name="Spawn" type="Marker2D" parent="." unique_id=1349167531]
-position = Vector2(64, 32)
+position = Vector2(96, 32)

 [node name="Wall" type="StaticBody2D" parent="." unique_id=49164077]

 [node name="Shape" type="CollisionShape2D" parent="Wall" unique_id=559493353]
 shape = SubResource("RectangleShape2D_jirdo")
+
+[node name="Floor" type="StaticBody2D" parent="."]
+position = Vector2(0, 160)
+
+[node name="Shape" type="CollisionShape2D" parent="Floor"]
+shape = SubResource("RectangleShape2D_wall2")
+
+[node name="Timer" type="Timer" parent="."]
+wait_time = 0.1
+autostart = true
+one_shot = true
+
+[connection signal="timeout" from="Timer" to="." method="_on_timer_timeout"]
```

## 3. Override a node of an instanced scene

`levels/world.tscn`, written by hand:

```text
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://levels/level_1.tscn" id="1_lvl"]

[node name="World" type="Node"]

[node name="Level" parent="." instance=ExtResource("1_lvl")]

[node name="Spawn" parent="Level"]
position = Vector2(10, 10)
```

The override section names the node and its parent, with no `type`.
Reading `Level/Spawn.position` from `world.tscn` gives `(10.0, 10.0)`.

## 4. The scene check

Save as `.godot/horch-home/scene_check.gd`:

```gdscript
# Loads and instantiates each scene given after "--", and checks every signal
# connection. Prints SCENE_CHECK lines; exits 1 when one scene fails.
extends SceneTree


func _init() -> void:
	var failed := false
	for path in OS.get_cmdline_user_args():
		var scene := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE) as PackedScene
		if scene == null or not scene.can_instantiate():
			print("SCENE_CHECK FAIL ", path, " does not load")
			failed = true
			continue
		var root := scene.instantiate()
		var nodes: Array[Node] = [root]
		nodes.append_array(root.find_children("*", "", true, false))
		for node in nodes:
			for sig in node.get_signal_list():
				for conn in node.get_signal_connection_list(sig["name"]):
					var callable: Callable = conn["callable"]
					if not callable.is_valid():
						print("SCENE_CHECK FAIL ", path, " connection ", root.get_path_to(node),
								".", sig["name"], " -> ", callable.get_method())
						failed = true
		print("SCENE_CHECK ", path, " nodes=", nodes.size())
		root.free()
	quit(1 if failed else 0)
```

```bash
gd -s "$PWD/.godot/horch-home/scene_check.gd" -- res://levels/level_1.tscn res://levels/world.tscn > "$LOGS/scene.log" 2>&1; echo "exit=$?"
grep -E 'SCENE_CHECK|SCRIPT ERROR|^ERROR:|^WARNING:' "$LOGS/scene.log"
```

Output after the edits (exit 0):

```text
SCENE_CHECK res://levels/level_1.tscn nodes=7
SCENE_CHECK res://levels/world.tscn nodes=8
```

Output with `method="_on_timer_typo"` in the connection (exit 1). Without the
check, the scene loads and runs, and Godot prints nothing:

```text
SCENE_CHECK FAIL res://levels/level_1.tscn connection Timer.timeout -> _on_timer_typo
SCENE_CHECK res://levels/level_1.tscn nodes=7
```

Only scenes you name are checked. Name each scene you changed, and each scene
that instances one of them.

## 5. Read the values back

The scene check does not see a misspelled property: `positon = Vector2(96, 32)`
loads with no message, and `position` stays `(0, 0)`. Read the edited values
back. Save as `.godot/horch-home/read_back.gd`:

```gdscript
# Prints property values from an instantiated scene, to prove a text edit.
extends SceneTree


func _init() -> void:
	var root := (load("res://levels/level_1.tscn") as PackedScene).instantiate()
	print("READ Spawn.position=", root.get_node("Spawn").position)
	var floor_shape := root.get_node("Floor/Shape") as CollisionShape2D
	print("READ Floor/Shape.size=", (floor_shape.shape as RectangleShape2D).size)
	root.free()
	quit(0)
```

```bash
gd -s "$PWD/.godot/horch-home/read_back.gd"
```

```text
READ Spawn.position=(96.0, 32.0)
READ Floor/Shape.size=(128.0, 16.0)
```

The smoke run proves the connection works:
`gd --quit-after 30 --scene res://levels/level_1.tscn` prints
`CONNECTED timeout reached`.

## 6. A UID on demand

Use this only when the task needs a `uid://` now (for example, another file
must reference the scene by UID). The engine makes the id; you never type it.
Save as `.godot/horch-home/set_uid.gd`:

```gdscript
# Gives one scene an engine-made UID. Pass the path after "--".
extends SceneTree


func _init() -> void:
	var path := OS.get_cmdline_user_args()[0]
	if ResourceLoader.get_resource_uid(path) != ResourceUID.INVALID_ID:
		print("UID_KEEP ", path)
		quit(0)
		return
	var id := ResourceUID.create_id_for_path(path)
	var err := ResourceSaver.set_uid(path, id)
	print("UID_SET ", path, " ", ResourceUID.id_to_text(id), " ", error_string(err))
	quit(0 if err == OK else 1)
```

```bash
gd -s "$PWD/.godot/horch-home/set_uid.gd" -- res://levels/level_1.tscn
gd --import
```

Measured: `ResourceSaver.set_uid()` rewrites only the header, to
`[gd_scene format=3 uid="uid://..."]`, and after the import
`ResourceLoader.get_resource_uid()` returns that id. A second run prints
`UID_KEEP` and changes nothing.
