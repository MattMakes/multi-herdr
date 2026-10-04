# Loading scenes in the background

Adds threaded scene loading with progress, failure handling and early
staging, plus what happens to the tree during a scene change. Read it when
you build a loading screen, a level switch, or anything that loads a large
scene while the game runs. Checked on Godot 4.7.2.

## Why

`load()` and `change_scene_to_file()` load on the main thread. A large level
freezes the game until it is in memory. `ResourceLoader.load_threaded_request()`
loads on a worker thread. The main thread polls the status every frame and
can draw a progress bar.

## The API

| Call | Result |
| --- | --- |
| `ResourceLoader.load_threaded_request(path, type_hint, use_sub_threads, cache_mode)` | Starts the load. Returns an `Error`. |
| `ResourceLoader.load_threaded_get_status(path, progress)` | `THREAD_LOAD_IN_PROGRESS`, `THREAD_LOAD_LOADED`, `THREAD_LOAD_FAILED` or `THREAD_LOAD_INVALID_RESOURCE`. Writes a 0.0 to 1.0 value into `progress[0]`. |
| `ResourceLoader.load_threaded_get(path)` | The loaded resource. Blocks until the load ends if it has not. |

`THREAD_LOAD_INVALID_RESOURCE` means no request exists for that path. Check
the return value of the request, and check `ResourceLoader.exists(path)`
first: a typo in a path otherwise looks like a load that never ends.

## A scene loader autoload

Register it as an autoload named `SceneLoader`. It owns the poll loop and
reports through signals, so a loading screen only listens.

```gdscript
extends Node

signal progress_changed(path: String, ratio: float)
signal load_failed(path: String)
signal scene_changed(path: String)

var _pending: String = ""


func _ready() -> void:
	set_process(false)


func change_scene(path: String) -> void:
	if not _pending.is_empty():
		push_warning("SceneLoader: already loading %s" % _pending)
		return
	if not ResourceLoader.exists(path):
		load_failed.emit(path)
		return
	var err := ResourceLoader.load_threaded_request(path, "PackedScene", true)
	if err != OK:
		load_failed.emit(path)
		return
	_pending = path
	set_process(true)


func _process(_delta: float) -> void:
	var progress: Array = []
	var status := ResourceLoader.load_threaded_get_status(_pending, progress)
	match status:
		ResourceLoader.THREAD_LOAD_IN_PROGRESS:
			progress_changed.emit(_pending, float(progress[0]))
		ResourceLoader.THREAD_LOAD_LOADED:
			_finish(ResourceLoader.load_threaded_get(_pending) as PackedScene)
		_:
			_fail()


func _finish(scene: PackedScene) -> void:
	var path := _pending
	_pending = ""
	set_process(false)
	if scene == null:
		load_failed.emit(path)
		return
	progress_changed.emit(path, 1.0)
	get_tree().change_scene_to_packed(scene)
	scene_changed.emit(path)


func _fail() -> void:
	var path := _pending
	_pending = ""
	set_process(false)
	load_failed.emit(path)
```

The poll loop lives in an autoload on purpose. See "What the tree does
during a change" below.

## Stage the next level early

A loading screen is not the only option. Start the request while the player
still plays (a victory screen, a dialogue, a long corridor). When the player
reaches the exit, the scene is often already loaded and the switch is
instant. Keep the path, and call `load_threaded_get()` only at the switch.

```gdscript
extends Area2D

@export_file("*.tscn") var next_level: String = ""


func _ready() -> void:
	ResourceLoader.load_threaded_request(next_level, "PackedScene", true)
	body_entered.connect(_on_body_entered)


func _on_body_entered(_body: Node2D) -> void:
	var scene := ResourceLoader.load_threaded_get(next_level) as PackedScene
	if scene != null:
		get_tree().change_scene_to_packed.call_deferred(scene)
```

The deferred call matters here: a physics callback must not remove the
scene that holds the colliding body.

## What the tree does during a change

Measured on 4.7.2 with `change_scene_to_packed()`:

- The call returns `OK`. At once, the old scene is out of the tree
  (`is_inside_tree()` is `false`) but not yet freed.
- `get_tree().current_scene` is `null` until the new scene is added, on the
  next frame. The new scene's `_ready()` then sees itself as
  `current_scene`.
- The old scene is freed. A coroutine in the old scene that awaits a frame
  after the call never resumes. Code after the `await` does not run.

So never put the steps after a scene change (emit a signal, fade in, save)
in the scene that is being replaced. Put them in an autoload, as the
loader above does.

`change_scene_to_node(node)` (4.7) takes a node you have built yourself.
`unload_current_scene()` removes the current scene with no replacement.
`reload_current_scene()` loads the scene file again from disk. For a quick
respawn, reset the state and move the player instead.

## Content packs

`ProjectSettings.load_resource_pack(pck_path)` mounts a `.pck` exported by
Godot. With the default `replace_files = true`, a file in the pack hides the
file of the same `res://` path. Load or change to the path after the mount
returns `true`. Mount a pack before anything loads the paths it replaces, so
no old copy of those resources is in the resource cache.
