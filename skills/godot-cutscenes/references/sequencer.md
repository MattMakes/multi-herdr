# The sequencer: one clock, one finish path

Every block here is `proof: not run (needs Godot)`: written for Godot 4.7 and not executed in the session that wrote it.

## The base script

One scene per cutscene. The root carries a script that extends this class. The `AnimationPlayer` holds 1 animation, named `cutscene` by default, with every track of the scene.

```gdscript
class_name Cutscene
extends Node
## One cutscene. `player` is the only clock.

signal finished(skipped: bool)

@export var player: AnimationPlayer
@export var timeline: StringName = &"cutscene"
@export var skippable: bool = true

var running: bool = false


func play() -> void:
	if running:
		return
	running = true
	_enter()
	player.animation_finished.connect(_on_animation_finished)
	player.play(timeline)


func skip() -> void:
	if not running or not skippable:
		return
	# Disconnect first: a seek to the end must not report a full play.
	player.animation_finished.disconnect(_on_animation_finished)
	# update = true applies the last key of every value track now.
	# Method keys between here and the end do not fire: they are cosmetic.
	player.seek(player.current_animation_length, true)
	player.stop(true)
	_finish(true)


func position_sec() -> float:
	return player.current_animation_position if running else 0.0


func _on_animation_finished(_name: StringName) -> void:
	_finish(false)


func _finish(skipped: bool) -> void:
	if not running:
		return
	running = false
	if player.animation_finished.is_connected(_on_animation_finished):
		player.animation_finished.disconnect(_on_animation_finished)
	_apply_end_state()
	_exit()
	finished.emit(skipped)


## Lock gameplay and take the camera. Store what you change.
func _enter() -> void:
	pass


## Set every fact the game keeps. Runs on a full play and on a skip.
func _apply_end_state() -> void:
	pass


## Restore exactly what `_enter` changed.
func _exit() -> void:
	pass
```

What belongs where:

| Thing | Place |
|---|---|
| Positions, rotations, colours, shader parameters, camera moves | value and bezier tracks |
| Character clips | animation playback tracks that drive each actor's own `AnimationPlayer` |
| Voice and music | audio tracks, or audio-master sync for long scenes |
| A one-shot sound, a particle burst, a camera shake | method-call tracks |
| A quest flag, a door state, an item, the final actor transform | `_apply_end_state()` |
| Input lock, HUD, camera ownership | `_enter()` and `_exit()` |

A method key that a seek jumps over does not fire. So nothing the game must remember may sit on a method track.

## A concrete cutscene

```gdscript
extends Cutscene
## The gate opens and the guard steps aside.

@export var gate: Node2D
@export var guard: Node2D
@export var guard_end: Marker2D
@export var shot_camera: Camera2D

var _gameplay_camera: Camera2D


func _enter() -> void:
	_gameplay_camera = get_viewport().get_camera_2d()
	get_tree().call_group(&"gameplay", &"set_cutscene_locked", true)
	shot_camera.make_current()


func _apply_end_state() -> void:
	gate.set(&"open", true)
	guard.global_position = guard_end.global_position


func _exit() -> void:
	if is_instance_valid(_gameplay_camera):
		_gameplay_camera.make_current()
		_gameplay_camera.reset_smoothing()
	get_tree().call_group(&"gameplay", &"set_cutscene_locked", false)
```

## Two ways to lock gameplay

| Pattern | How | Use it when | Trap |
|---|---|---|---|
| Group lock | Every gameplay node in group `gameplay` has `set_cutscene_locked(locked)`. It stops its own input, AI and timers. | Actors of the level act in the scene | A new node type that forgets the method keeps running. Test the group. |
| Tree pause | `get_tree().paused = true`. The cutscene root and its actors use `PROCESS_MODE_ALWAYS`. | The cutscene is an overlay: a motion comic, a clip, a dialogue box | Level actors freeze. Physics stops. The pause menu must not unpause the tree under the cutscene. |

With the group lock, a locked player still collides and falls. Zero its velocity on lock, or the last input carries it off its mark.

Do not disable the whole level with `PROCESS_MODE_DISABLED`: animation players and particles in it stop too, and physics bodies leave the simulation.

## Hold to skip

A tap skips by accident. Hold-to-skip reads the same action and fills a ring.

```gdscript
extends Control
## Hold the action for `hold_sec` to skip. Draws its own progress.

signal skip_requested

@export var action: StringName = &"ui_cancel"
@export var hold_sec: float = 0.8

var _held: float = 0.0


func _process(delta: float) -> void:
	if Input.is_action_pressed(action):
		_held += delta
	else:
		_held = maxf(_held - delta * 2.0, 0.0)
	visible = _held > 0.0
	queue_redraw()
	if _held >= hold_sec:
		_held = 0.0
		set_process(false)
		skip_requested.emit()


func _draw() -> void:
	var radius := minf(size.x, size.y) * 0.5
	draw_arc(size * 0.5, radius, -PI * 0.5, -PI * 0.5 + TAU * (_held / hold_sec), 48, Color.WHITE, 3.0, true)
```

Connect `skip_requested` to `Cutscene.skip`. Set the control's `process_mode` to `PROCESS_MODE_ALWAYS` under a tree pause. Show the prompt text ("Hold to skip") on the first press of any key, not all the time.

## Play once, survive a save

The trigger owns the "played" record. The cutscene does not.

```gdscript
extends Area2D
## Starts a cutscene the first time the player enters.

@export var cutscene_id: StringName
@export_file("*.tscn") var cutscene_path: String
@export var played_store: Node


func _ready() -> void:
	ResourceLoader.load_threaded_request(cutscene_path)
	body_entered.connect(_on_body_entered)


func _on_body_entered(body: Node2D) -> void:
	if not body.is_in_group(&"player") or played_store.call(&"has_played", cutscene_id):
		return
	set_deferred(&"monitoring", false)
	var scene := ResourceLoader.load_threaded_get(cutscene_path) as PackedScene
	var cutscene := scene.instantiate() as Cutscene
	get_tree().current_scene.add_child(cutscene)
	cutscene.finished.connect(_on_finished.bind(cutscene))
	cutscene.play()


func _on_finished(_skipped: bool, cutscene: Cutscene) -> void:
	played_store.call(&"mark_played", cutscene_id)
	cutscene.queue_free()
```

Save rules:

- Do not save while a cutscene runs. Block the save action in `_enter` and release it in `_exit`.
- The "played" id is written after `_apply_end_state()`. A crash in the middle then replays the scene from a state the save still describes.
- A save loaded after the scene must not need the cutscene node. Everything it needs is what `_apply_end_state()` wrote into the game's own state. The project's save rules are in **godot-save-load**.

## A scripted beat with one Tween

For a beat with no cut, a `Tween` is the clock. It still has 1 finish path.

```gdscript
extends Node2D

@export var door: Node2D
@export var open_offset: Vector2 = Vector2(0.0, -96.0)

var _tween: Tween
var _closed_position: Vector2


func open() -> void:
	_closed_position = door.position
	if _tween:
		_tween.kill()
	_tween = create_tween()
	# Anticipation: 6 % the wrong way, then the move, then a short settle.
	_tween.tween_property(door, "position", _closed_position - open_offset * 0.06, 0.12) \
		.set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_OUT)
	_tween.tween_property(door, "position", _closed_position + open_offset, 0.45) \
		.set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_IN_OUT)
	_tween.tween_callback(_apply_end_state)


func skip() -> void:
	if _tween and _tween.is_valid():
		_tween.kill()
		_apply_end_state()


func _apply_end_state() -> void:
	door.position = _closed_position + open_offset
```

## Sequence several cutscenes

Chain scenes with `finished`, not with timers. A list of ids and 1 function that plays the next is enough. Do not write a cutscene manager until 3 scenes need the same thing that this base class cannot give them.
