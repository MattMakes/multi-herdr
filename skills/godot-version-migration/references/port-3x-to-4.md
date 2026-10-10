# Porting a Godot 3.x project to 4.7

Read this when `project.godot` has `config_version=4`. It gives the converter
run, what the converter does and does not do, and the manual fixes, each as
the 4.7 name. Source: the official guide "Upgrading from Godot 3 to Godot 4"
(godot-docs branch `4.7` at `9adca4c1c72917bfe1b7be3108abed5ce26696a6`,
`tutorials/migrating/upgrading_to_godot_4.rst`). Every 4.7 name below was
looked up in the 4.7.2 `--doctool` dump. Where the guide and the dump
disagree, the dump wins and the row says so.

## 1. Before the converter

- Commit everything. The converter writes in place and keeps no backup.
- The converter takes Godot 3.0 and later only.
- Rename every `.shader` file to `.gdshader` and fix the paths in `.tscn` and
  `.tres` files. 4.x loads `.gdshader` only.

## 2. Run the converter headless

```bash
G="${GODOT_PATH:-/Applications/Godot.app/Contents/MacOS/Godot}"
"$G" --headless --path . --validate-conversion-3to4 > ai_docs/port-3to4-plan.txt 2>&1
"$G" --headless --path . --convert-3to4
git diff --stat
```

Optional limits: `--convert-3to4 <max_file_kb> <max_line_size>`. The
defaults are 4 MB and 100,000 lines; a larger file is skipped, not
converted. Check the plan file for skipped files.

What one 4.7.2 run did on a small 3.x project (measured):

| 3.x input | converter output |
|---|---|
| `extends KinematicBody2D` | `extends CharacterBody2D` |
| `export var speed = 200` | `@export var speed = 200` |
| `onready var sprite = $Sprite` | `@onready var sprite = $Sprite2D` |
| `var hp = 10 setget set_hp` | `var hp = 10: set = set_hp` |
| `connect("died", self, "_on_died")` | `connect("died", Callable(self, "_on_died"))` |
| `OS.get_ticks_msec()` | `Time.get_ticks_msec()` |
| `yield(get_tree().create_timer(1.0), "timeout")` | `await get_tree().create_timer(1.0).timeout` |
| `[1].empty()` | `[1].is_empty()` |
| `v = move_and_slide(v, Vector2.UP)` | `set_velocity(v)`, `set_up_direction(Vector2.UP)`, `move_and_slide()`, `v = velocity` |
| `get_tree().change_scene(...)` | `get_tree().change_scene_to_file(...)` |
| node `type="Sprite"` named `Sprite` | `type="Sprite2D"` **and the node renamed to `Sprite2D`** |
| node `type="Position2D"` | `type="Marker2D"` |
| `config_version=4` | `config_version=5` |
| `update()` | unchanged: parse error on 4.7 |
| `File.new()` / `f.open(...)` | unchanged: parse error on 4.7 |

Watch the node renames: a node whose name equals its old class name gets the
new class name. The converter fixed `$Sprite` paths in the script it
converted. Check paths in strings, animation tracks and other scripts by hand.

Then fix every parse error, import once, and continue with
`upgrade-4x-to-4.7.md` from the 4.0 → 4.1 hop.

The converted code above still has 3.x habits that work but are not the 4.7
way. Rewrite them when you touch the file:

```gdscript
extends CharacterBody2D

signal died

@export var speed: float = 200.0
@onready var sprite: Sprite2D = $Sprite2D

var hp: int = 10:
	set(value):
		hp = value
		if hp <= 0:
			died.emit()


func _ready() -> void:
	died.connect(_on_died)
	await get_tree().create_timer(1.0).timeout


func _physics_process(_delta: float) -> void:
	velocity = Vector2.ZERO
	move_and_slide()
	queue_redraw()


func _on_died() -> void:
	get_tree().change_scene_to_file("res://main.tscn")
	var file := FileAccess.open("user://x", FileAccess.WRITE)
	if file == null:
		push_error("open failed: %s" % error_string(FileAccess.get_open_error()))
```

## 3. Manual renames (the converter does not do these)

Old 3.x name → 4.7 name. A renamed property also renames its setter and
getter (`set_offset()` → `set_progress()`).

| 3.x | 4.7 |
|---|---|
| `File`, `Directory` | `FileAccess`, `DirAccess` (many methods are static: `FileAccess.open(path, mode)`) |
| `OS.get_screen_size()` and other screen or window calls | `DisplayServer.screen_get_size()` (`DisplayServer.<object>_<get/set>_<property>()`) |
| OS time and date calls | `Time` |
| `instance()` | `instantiate()` |
| `AcceptDialog.set_autowrap()` | `set_autowrap_mode()` |
| `AnimationPlayer.add_animation()` | `add_animation_library()` with an `AnimationLibrary` |
| `AnimationTree.set_process_mode()` | the `callback_mode_process` property |
| `Array.empty()` / `invert()` / `remove()` | `is_empty()` / `reverse()` / `remove_at()` |
| `AStar2D` / `AStar3D` `get_points()` | `get_point_ids()` (the guide says `get_points_id()`; the 4.7.2 dump has `get_point_ids()`) |
| `BaseButton.set_event()` | `shortcut` property |
| `Camera2D` `get/set_h_offset()`, `get/set_v_offset()` | `drag_horizontal_offset`, `drag_vertical_offset` |
| `CanvasItem.raise()` | `move_to_front()` |
| `CanvasItem.update()` | `queue_redraw()` |
| `Control.get_stylebox()` | `get_theme_stylebox()` |
| `Control.set_tooltip()` | `tooltip_text` |
| `ENetMultiplayerPeer.get_peer_port()` | `get_peer()` |
| `FileDialog.get_mode()` / `set_mode()` | `file_mode` |
| `GridMap` / `TileMap` `map_to_world()` / `world_to_map()` | `map_to_local()` / `local_to_map()` |
| `Image.get_rect()` | `get_region()` |
| `ItemList.get_v_scroll()` | `get_v_scroll_bar()` |
| `MultiplayerAPI` `get_network_connected_peers()`, `get_network_unique_id()`, `has_network_peer()` | `get_peers()`, `get_unique_id()`, `has_multiplayer_peer()` |
| `PacketPeerUDP.listen()` / `is_listening()` | `bind()` / `is_bound()` |
| `ParticleProcessMaterial.set_flag()` | `set_particle_flag()` |
| `RenderingServer.get_render_info()` | `get_rendering_info()` |
| `SceneTree.change_scene()` | `change_scene_to_file()` (or `change_scene_to_packed()`) |
| `Shortcut.is_valid()` | `has_valid_event()` |
| `Transform2D.xform(v)` / `xform_inv(v)` | `transform * v` / `v * transform` |
| `XRPositionalTracker.get_name()` / `get_type()` | the `name` / `type` properties of `XRTracker` |
| `AudioServer.device` | `output_device` |
| `BaseButton.group` | `button_group` |
| `Camera3D.znear` / `zfar` | `near` / `far` |
| `Control.margin_*` | `offset_*` |
| `InputEventMouseButton.doubleclick` | `double_click` |
| `InputEventWithModifiers` `alt`, `shift`, `control`, `meta`, `command` | `alt_pressed`, `shift_pressed`, `ctrl_pressed`, `meta_pressed`, `command_or_control_autoremap` (see note) |
| `Label.percent_visible` | `visible_ratio` |
| `Node.filename` | `scene_file_path` |
| `PathFollow2D.rotate` | `rotates` |
| `PathFollow2D/3D.offset` | `progress` |
| `RectangleShape2D.extents` | `size` (full size: double the old value) |
| CSG nodes and `VoxelGI` `extents` | `size` (full size: double the old value) |
| `Engine.editor_hint` | `Engine.is_editor_hint()` |
| `Window.window_title` | `title` |
| Theme item names `on` / `off` / `ofs` | `checked` / `unchecked` / `offset` |
| `CanvasItem` signal `hide` | signal `hidden` (the `hide()` method keeps its name) |
| `Tween` signal `tween_all_completed` | `Tween.loop_finished` (per loop) or `Tween.finished` (all loops done). Tween is no longer a node: see below. |
| colour constants `Color.palegreen` | `Color.PALE_GREEN` |
| `MainLoop.NOTIFICATION_WM_QUIT_REQUEST` | `NOTIFICATION_WM_CLOSE_REQUEST` (also on `Node`) |
| `_get_property_list()` hint strings `or_lesser`, `noslider` | `or_less`, `no_slider` |

Note on `command`: the 3→4 guide lists `command` → `command_pressed`, but
the 4.7.2 dump has no `command_pressed`. Use `meta_pressed` for the macOS
Command key, or `command_or_control_autoremap` for "Command on macOS,
Ctrl elsewhere". Note on `TextureProgressBar.percent_visible`: the guide maps it
to `show_percentage`, which exists on `ProgressBar` only in 4.7.2. A
`TextureProgressBar` has no percentage label; use a `Label`.

## 4. Behaviour changes with no rename

- `_ready()`, `_process()` and the other lifecycle methods no longer call the
  parent class version. Call `super()` first where the parent's code must run.
- `String` and `StringName` are both types. `"a" == &"a"` is true, but
  `is_same("a", &"a")` is false.
- Setters and getters use the property syntax (`var x: int: set = f`, or an
  inline `set(value):` block). The converter converts only part of it.
- Connect signals with the `Signal` object: `died.connect(_on_died)`, and emit
  with `died.emit()`. The converter writes string-based `connect()` calls that
  still work; rewrite them.
- A built-in (in-scene) tool script keeps the old `tool` keyword. Change it to
  `@tool`.
- The `Tween` node is gone. Use `create_tween()` and the `Tweener` chain.
- `randomize()` runs at startup. For a repeatable sequence, set a seed
  (`seed(1234)`) or use your own `RandomNumberGenerator` with a set `seed`.
- `call_group()`, `set_group()` and `notify_group()` run at once. For the old
  deferred behaviour use
  `get_tree().call_group_flags(SceneTree.GROUP_CALL_DEFERRED, "group", "method")`.
- The inspector shows `rotation` in degrees, but the property is radians.
  Animation tracks on `rotation_degrees` do not convert.
- `AABB.has_no_surface()` → `has_surface()`, `has_no_area()` → `has_area()`;
  both are inverted.
- `AnimatedTexture.fps` → `speed_scale`.
- `AnimatedSprite2D/3D.playing` is gone: call `play()` / `stop()`, or set an
  autoplay animation (not both). `speed_scale` can now be negative.
- `Array.slice(begin, end)`: `end` is now exclusive.
- `BaseButton` property `pressed` → `button_pressed`; the `pressed` signal
  stays.
- `Camera2D.rotating` → `ignore_rotation`, inverted. `Camera2D.zoom` is
  inverted: a bigger value zooms in.
- `Node.remove_and_skip()` is gone.
- `ResourceSaver.save(resource, path)`: the arguments swapped.
- `StreamPeerTCP` needs `poll()` each frame; `get_status()` does not poll.
  `is_connected_to_host()` is gone: use `get_status()` on `StreamPeerTCP`,
  `is_socket_connected()` on `PacketPeerUDP`.
- `String.right(n)` returns the last `n` characters. For "from index `i`
  to the end" use `substr(i)`.
- Threads take a `Callable`: `thread.start(_load.bind(path))`.
  `Thread.is_active()` → `is_alive()`.

## 5. Removed nodes: rebuild them

| removed | 4.7 replacement |
|---|---|
| `AnimationTreePlayer` | `AnimationTree` |
| `BakedLightmap` | `LightmapGI` (bake again) |
| `GIProbe` | `VoxelGI` (renamed by the converter; bake again) |
| `BitmapFont`, `DynamicFont`, `DynamicFontData` | `FontFile` |
| `ClippedCamera`, `InterpolatedCamera` | `Camera3D` (+ `SpringArm3D` for collision) |
| `Navigation2D`, `Navigation3D` | `NavigationRegion2D/3D`, `NavigationAgent2D/3D`, `NavigationServer2D/3D` |
| `OpenSimplexNoise` | `FastNoiseLite` (other parameters; no 4D noise) |
| `ToolButton` | `Button` with `flat = true` |
| `YSort` | any `CanvasItem` with `y_sort_enabled = true` |
| `ProximityGroup` | `VisibleOnScreenNotifier3D` |
| `Portal`, `Room`, `RoomManager`, `RoomGroup`, `Occluder` | `OccluderInstance3D` (raster occlusion culling; new setup) |

Godot loads an old scene with the closest node in place of a removed one.
The settings of the old node are lost.

## 6. Other areas

- **Shaders.** `hint_albedo` and `hint_color` → `source_color`. Filter and
  repeat are set per `uniform`, not on the texture. Particle shaders use
  `start()` and `process()`, not `vertex()`. Built-in matrix names changed.
  Forward+ and Mobile use NDC depth `[0, 1]`:
  `vec3 ndc = vec3(SCREEN_UV * 2.0 - 1.0, depth);`. From 4.3 the depth buffer
  is reversed (see the 4.3 hop). Custom `light()` functions need a visual
  check by a human.
- **Rendering settings.** Environment quality settings moved to project
  settings and do not convert. Set them again. A settings menu that changed
  `Environment` quality must call `RenderingServer` instead.
- **2D HDR.** 2D renders without HDR, so overbright `modulate` has no
  effect. Turn on `rendering/viewport/hdr_2d` if the project needs it.
- **Physics.** Bullet is gone. Godot Physics 3D or Jolt (the default for new
  4.6+ projects) behave differently: tune joints and contacts again.
- **ArrayMesh files.** A 3.x `ArrayMesh` saved as `.res` or `.tres` does not
  load. Import the source model again.
- **Project settings.** Some settings were renamed or changed their enum
  values (for example shadow filter quality). Check them in a headless
  script with `ProjectSettings.get_setting()`.
- **C#.** Mono became .NET. Read `godot-csharp-godot`.
- **Version control.** 4.x needs a different ignore list. The official list
  ignores `.godot/` and `*.translation`. Keep the `*.uid` files under version
  control (the rule of `godot-scene-files`).

The full list of automatic renames is the engine source file
`editor/project_upgrade/renames_map_3_to_4.cpp`.
