# Transitions, overlays and state that survives a scene change

Adds fade transitions, overlays that keep the world running, state that
survives a scene change, safe reparenting, quit handling and a leak check.
Read it with `scene-loading.md` when you build menus, level changes or
pause screens. Checked on Godot 4.7.2.

## Pick the right tool

| Goal | Use | Not |
| --- | --- | --- |
| Go to another level | `SceneLoader.change_scene()` (see `scene-loading.md`) | `change_scene_to_file()` inside the level |
| Pause menu, map, inventory over the world | An overlay `CanvasLayer` added to the tree | A scene change |
| Restart after death | Reset state and move the player | `reload_current_scene()` |
| Keep score, inventory, settings across levels | An autoload or a `Resource` it holds | Variables in the level scene |
| Minimap, picture-in-picture | `SubViewport` in a `SubViewportContainer` | A second main scene |

## A fade transition

An autoload that owns a full-screen `ColorRect` on a high `CanvasLayer`.
It builds its nodes in code, so it needs no scene file.

```gdscript
extends CanvasLayer

signal faded_out
signal faded_in

@export var fade_time: float = 0.25

var _rect: ColorRect


func _ready() -> void:
	layer = 100
	process_mode = Node.PROCESS_MODE_ALWAYS
	_rect = ColorRect.new()
	_rect.color = Color.BLACK
	_rect.modulate.a = 0.0
	_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_rect)


func fade_out() -> void:
	_rect.mouse_filter = Control.MOUSE_FILTER_STOP
	var tween := create_tween()
	tween.tween_property(_rect, "modulate:a", 1.0, fade_time)
	await tween.finished
	faded_out.emit()


func fade_in() -> void:
	var tween := create_tween()
	tween.tween_property(_rect, "modulate:a", 0.0, fade_time)
	await tween.finished
	_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	faded_in.emit()
```

The caller is another autoload (or the loader), never the level that is
replaced:

```gdscript
extends Node

@export var fader: CanvasLayer


func go_to(path: String) -> void:
	await fader.fade_out()
	get_tree().change_scene_to_file(path)
	await get_tree().scene_changed
	await fader.fade_in()
```

`SceneTree.scene_changed` fires when the new scene is in the tree. The
rectangle blocks the mouse while the screen is dark, so a click cannot reach
a button of the scene that is about to go.

## Overlays that keep the world

Add the menu to the tree. Pause the tree if the world must stop. The menu
needs `PROCESS_MODE_ALWAYS` (or `WHEN_PAUSED`) to work while paused.

```gdscript
extends Node

@export var pause_menu_scene: PackedScene

var _menu: Node


func open_pause_menu() -> void:
	if _menu != null:
		return
	_menu = pause_menu_scene.instantiate()
	_menu.process_mode = Node.PROCESS_MODE_WHEN_PAUSED
	get_tree().root.add_child(_menu)
	get_tree().paused = true


func close_pause_menu() -> void:
	if _menu == null:
		return
	get_tree().paused = false
	_menu.queue_free()
	_menu = null
```

A `SubViewport` passes input events down to its own scene. If a click in a
minimap must not also reach the world, call
`get_viewport().set_input_as_handled()` in the handler that used it.

## State that survives a change

A scene change frees every node of the old scene, with its variables. Keep
cross-level state outside the scene:

- In an autoload (`GameState.score`), for run state that systems share.
- In a `Resource` that an autoload holds, when the state is also saved.
- In `static var` on a `class_name` script, for plain values that need no
  signals. Static values last for the engine's lifetime, so reset them
  yourself on "new game".

To carry a node itself (a player with all its children) into the next level,
move it out of the scene before the change and back in after:

```gdscript
extends Node

var _carried: Node2D


func carry(node: Node2D) -> void:
	_carried = node
	node.reparent(self)


func drop_into(new_parent: Node) -> void:
	if _carried == null:
		return
	_carried.reparent(new_parent)
	_carried = null
```

`Node.reparent(new_parent, keep_global_transform = true)` keeps the global
transform by default. Do not reparent a physics body inside a physics
callback. Call it deferred (`node.reparent.call_deferred(target)`).

## Quit without losing a save

On desktop, closing the window quits at once. To save first, turn off the
automatic quit and handle the close request in an autoload:

```gdscript
extends Node

signal save_requested


func _ready() -> void:
	get_tree().auto_accept_quit = false


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		save_requested.emit()
		get_tree().quit()
```

The project setting `application/config/auto_accept_quit` sets the same
value for the whole project.

## Leak check after a change

After a scene change, a node that left the tree but was never freed is an
orphan. It holds memory and often keeps signal connections alive. In a debug
build, check the count after each change:

```gdscript
extends Node


func _ready() -> void:
	get_tree().scene_changed.connect(_on_scene_changed)


func _on_scene_changed() -> void:
	if not OS.is_debug_build():
		return
	var orphans := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
	if orphans > 0:
		push_warning("%d orphan nodes after scene change" % orphans)
		Node.print_orphan_nodes()
```

`queue_free()` on a node frees all its children. Do not free children one by
one before you free the parent.
