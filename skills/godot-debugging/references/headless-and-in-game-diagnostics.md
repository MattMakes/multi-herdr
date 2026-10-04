Adds diagnostics that work without the editor debugger: making a headless run fail on any error, custom monitors, an in-game debug overlay, 2D and 3D debug drawing, a custom editor debugger tab, and `@tool` code paths; read it when you debug from a terminal or a CI log, on a device without a console, or need to see invisible state (AI targets, ranges, rays).

# Headless and In-Game Diagnostics

> ← Back to [SKILL.md](../SKILL.md). The full headless verify loop (import, parse check, tests, smoke run) is in **godot-build-verify**.

All code targets Godot 4.7. Measured on 4.7.2 headless.

---

## 1. A Headless Run Does Not Fail on Errors

On 4.7.2, a headless run whose script calls `push_error()` still exits 0. A
runtime script error in a function also does not change the exit code. Only
`get_tree().quit(<code>)` sets it. A CI step or a fleet `DONE:` that trusts
the exit code alone misses errors.

Two fixes, best used together:

- Grep the output for `SCRIPT ERROR`, `ERROR:` and `Parse Error`.
- Install a `Logger` that counts errors, and quit with 1 when the count is
  not zero.

```gdscript
extends SceneTree

## Headless smoke run: loads a scene, runs N frames, fails on any error.
## godot --headless --path . --script res://tools/smoke_run.gd
const SCENE := "res://main.tscn"
const FRAMES := 120


class ErrorCounter extends Logger:
	var count: int = 0
	var _m := Mutex.new()

	func _log_error(_function: String, _file: String, _line: int, _code: String, _rationale: String,
			_editor_notify: bool, error_type: int, _script_backtraces: Array[ScriptBacktrace]) -> void:
		if error_type != ERROR_TYPE_WARNING:
			_m.lock()
			count += 1
			_m.unlock()


func _init() -> void:
	var counter := ErrorCounter.new()
	OS.add_logger(counter)
	var packed := load(SCENE) as PackedScene
	if packed == null:
		printerr("smoke: cannot load ", SCENE)
		quit(1)
		return
	root.add_child.call_deferred(packed.instantiate())
	for i in FRAMES:
		await process_frame
	OS.remove_logger(counter)
	print("smoke: %d errors" % counter.count)
	quit(1 if counter.count > 0 else 0)
```

## 2. Custom Monitors

`Performance.add_custom_monitor()` adds a graph to the editor's
**Debugger → Monitors** tab. The callable runs each time the monitor
updates, so keep it cheap.

```gdscript
extends Node

## Registers game metrics as debugger monitors. Autoload, debug builds only.
func _ready() -> void:
	if not OS.is_debug_build():
		return
	Performance.add_custom_monitor(&"game/enemies_alive", _count_group.bind(&"enemies"))
	Performance.add_custom_monitor(&"game/projectiles", _count_group.bind(&"projectiles"))


func _exit_tree() -> void:
	for id: StringName in [&"game/enemies_alive", &"game/projectiles"]:
		if Performance.has_custom_monitor(id):
			Performance.remove_custom_monitor(id)


func _count_group(group: StringName) -> int:
	return get_tree().get_node_count_in_group(group)
```

The part before `/` becomes the category in the Monitors tab. Code can read
a custom monitor back with `Performance.get_custom_monitor()`.

## 3. An In-Game Overlay

On a phone, a console or a test machine there is no editor debugger. A small
overlay on a `CanvasLayer` with a high `layer` shows the numbers you need.
Build it only in debug builds, and refresh it a few times per second, not
every frame.

```gdscript
extends CanvasLayer

## Debug overlay. Toggle with F3. Removed in release builds.
var _label := Label.new()
var _timer := 0.0


func _ready() -> void:
	if not OS.is_debug_build():
		queue_free()
		return
	layer = 128
	process_mode = Node.PROCESS_MODE_ALWAYS
	_label.position = Vector2(8.0, 8.0)
	_label.add_theme_constant_override(&"outline_size", 4)
	add_child(_label)


func _unhandled_input(event: InputEvent) -> void:
	var key := event as InputEventKey
	if key and key.pressed and not key.echo and key.keycode == KEY_F3:
		visible = not visible


func _process(delta: float) -> void:
	_timer -= delta
	if _timer > 0.0 or not visible:
		return
	_timer = 0.25
	_label.text = "FPS %d | frame %.1f ms | phys %.1f ms\nnodes %d | orphans %d | draw calls %d" % [
		Engine.get_frames_per_second(),
		Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0,
		Performance.get_monitor(Performance.TIME_PHYSICS_PROCESS) * 1000.0,
		Performance.get_monitor(Performance.OBJECT_NODE_COUNT),
		Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT),
		Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME),
	]
```

## 4. Draw What Code Cannot Show

Draw invisible state (AI targets, ranges, rays, paths) in debug builds.

**2D:** override `_draw()` on the node and call `queue_redraw()` when the
state changes.

```gdscript
extends Node2D

## Shows an enemy's sight range and current target.
@export var sight_range: float = 200.0
var target: Node2D


func _process(_delta: float) -> void:
	if OS.is_debug_build():
		queue_redraw()


func _draw() -> void:
	if not OS.is_debug_build():
		return
	draw_arc(Vector2.ZERO, sight_range, 0.0, TAU, 48, Color(1.0, 1.0, 0.0, 0.5), 1.0)
	if is_instance_valid(target):
		draw_line(Vector2.ZERO, to_local(target.global_position), Color.RED, 2.0)
```

**3D:** an `ImmediateMesh` on a `MeshInstance3D`, rebuilt each frame.

```gdscript
extends MeshInstance3D

## Draws world-space lines for one frame. Call line() from anywhere, then
## the lines are drawn and cleared every frame.
var _mesh := ImmediateMesh.new()
var _lines: Array[PackedVector3Array] = []
var _material := StandardMaterial3D.new()


func _ready() -> void:
	mesh = _mesh
	_material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_material.vertex_color_use_as_albedo = true
	top_level = true


func line(from: Vector3, to: Vector3) -> void:
	_lines.append(PackedVector3Array([from, to]))


func _process(_delta: float) -> void:
	_mesh.clear_surfaces()
	if _lines.is_empty():
		return
	_mesh.surface_begin(Mesh.PRIMITIVE_LINES, _material)
	for pair in _lines:
		_mesh.surface_set_color(Color.YELLOW)
		_mesh.surface_add_vertex(pair[0])
		_mesh.surface_add_vertex(pair[1])
	_mesh.surface_end()
	_lines.clear()
```

In the editor, **Debug → Visible Collision Shapes** and **Visible
Navigation** show physics and navigation data without code.

## 5. A Custom Tab in the Editor Debugger

For game state the built-in panels do not show (a quest log, an AI blackboard),
send messages from the running game with `EngineDebugger.send_message()`
and receive them in an `EditorDebuggerPlugin` that an editor plugin
registers. The message name needs a prefix and a colon: the prefix selects
the capture.

```gdscript
extends Node

## In the game: sends the quest state to the editor once per second.
func _on_report_timer_timeout() -> void:
	if EngineDebugger.is_active():
		EngineDebugger.send_message("quests:state", [{"active": 3, "done": 12}])
```

```gdscript
@tool
extends EditorDebuggerPlugin

## In the editor: receives "quests:*" messages and shows them in a tab.
var _labels: Dictionary[int, Label] = {}


func _has_capture(capture: String) -> bool:
	return capture == "quests"


func _capture(message: String, data: Array, session_id: int) -> bool:
	if message == "quests:state" and _labels.has(session_id):
		_labels[session_id].text = str(data[0])
		return true
	return false


func _setup_session(session_id: int) -> void:
	var label := Label.new()
	label.name = "Quests"
	get_session(session_id).add_session_tab(label)
	_labels[session_id] = label
```

The editor plugin calls `add_debugger_plugin()` in `_enter_tree()` and
**must** call `remove_debugger_plugin()` with the same instance in
`_exit_tree()`. Plugin structure is in **godot-addon-development**.

## 6. Editor and Game Paths in `@tool` Scripts

A `@tool` script runs in the editor too. Guard game-only code with
`Engine.is_editor_hint()`, or the editor runs your gameplay, and a crash in
it can take the editor down with unsaved work.

```gdscript
@tool
extends Node3D

@export var radius: float = 2.0:
	set(value):
		radius = value
		update_gizmos()


func _process(delta: float) -> void:
	if Engine.is_editor_hint():
		return
	rotate_y(delta)
```
