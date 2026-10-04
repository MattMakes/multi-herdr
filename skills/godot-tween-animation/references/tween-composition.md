Adds tween composition and feel control: nested subtweens, custom easing curves, path and physics-synced tweens, data-driven feel presets and the hard limits of the Tween API; read it when one tween sequence grows into a timeline, or when built-in easing or process timing is not enough.

# Tween Composition and Feel

> ← Back to [SKILL.md](../SKILL.md). Kill/replace rules are in [lifecycle.md](lifecycle.md).

All code targets Godot 4.7.

---

## 1. Nested Timelines with `tween_subtween()` (Godot 4.4+)

`tween_subtween(subtween)` runs a whole other `Tween` as one step of the
parent. Build each part of a cutscene as a function that returns a tween,
then join the parts. The parent owns the child: the child stops running on
its own and plays when the parent reaches that step.

The returned `SubtweenTweener` accepts `set_delay()`.

```gdscript
extends Node2D

@onready var door: Node2D = $Door
@onready var hero: Node2D = $Hero
@onready var title: CanvasItem = $Title


func play_intro() -> void:
	var timeline := create_tween()
	timeline.tween_subtween(_open_door())
	timeline.tween_subtween(_walk_in()).set_delay(0.2)
	timeline.tween_subtween(_show_title())
	await timeline.finished


func _open_door() -> Tween:
	var t := create_tween()
	t.tween_property(door, "rotation", deg_to_rad(-90.0), 0.6).set_trans(Tween.TRANS_BACK).set_ease(Tween.EASE_OUT)
	return t


func _walk_in() -> Tween:
	var t := create_tween()
	t.tween_property(hero, "position:x", 320.0, 1.2).set_trans(Tween.TRANS_SINE)
	return t


func _show_title() -> Tween:
	var t := create_tween().set_parallel(true)
	t.tween_property(title, "modulate:a", 1.0, 0.4).from(0.0)
	t.tween_property(title, "scale", Vector2.ONE, 0.4).from(Vector2(0.6, 0.6))
	return t
```

Each part stays testable and reusable. A `set_parallel(true)` inside one part
does not leak into the parent sequence.

## 2. Custom Easing with a `Curve`

The `TRANS_*` and `EASE_*` pairs cover most motion. For a designer-made shape
(an overshoot with two bounces, a slow start and snap), give the tweener a
custom interpolator. `PropertyTweener.set_custom_interpolator()` takes a
`Callable` that maps linear progress (0 to 1) to eased progress. A `Curve`
resource gives you that mapping with `sample_baked()`.

```gdscript
extends Control

## Draw the curve in the inspector: x = time (0..1), y = progress (0..1).
@export var pop_curve: Curve


func pop_in() -> void:
	scale = Vector2.ZERO
	var t := create_tween()
	var tweener := t.tween_property(self, "scale", Vector2.ONE, 0.45)
	if pop_curve:
		tweener.set_custom_interpolator(pop_curve.sample_baked)
```

For a value that is not a property, `tween_method()` plus your own mapping
does the same job. `Tween.interpolate_value()` gives a `TRANS_*` / `EASE_*`
result for any value without creating a tween, which helps when you step
motion yourself.

## 3. Motion Along a Path

To move along a curve, do not write Bézier math in a tween. Put the node under
a `PathFollow2D` (or `PathFollow3D`) on a `Path2D`, and tween
`progress_ratio` from 0 to 1.

```gdscript
extends Path2D

@onready var follower: PathFollow2D = $PathFollow2D


func fly(duration: float) -> void:
	follower.progress_ratio = 0.0
	var t := create_tween()
	t.tween_property(follower, "progress_ratio", 1.0, duration).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)
```

## 4. Tweens That Move Physics Objects

A tween runs in the idle step by default. When it moves a body that physics
also reads, use `set_process_mode(Tween.TWEEN_PROCESS_PHYSICS)`, so each step
happens on a physics tick.

With physics interpolation on, a teleport before the tween shows as a smear
from the old position. Call `reset_physics_interpolation()` right after you
place the node at its start.

```gdscript
extends AnimatableBody3D

## Moves a platform between two points on physics ticks.
func move_platform(from_pos: Vector3, to_pos: Vector3, duration: float) -> void:
	global_position = from_pos
	reset_physics_interpolation()
	var t := create_tween().bind_node(self)
	t.set_process_mode(Tween.TWEEN_PROCESS_PHYSICS)
	t.tween_property(self, "global_position", to_pos, duration)
```

A camera that follows a moving target every frame is not a tween job. Use
`Camera2D.position_smoothing_enabled` or a spring in `_physics_process()`.
A one-shot camera punch can be a tween.

## 5. Feel as Data

When many systems share "juice" (button pops, hit squash, pickup bounce),
keep duration, transition and ease in a `Resource`, not in literals spread
across scripts. Designers then tune one asset.

```gdscript
extends Resource

## Save as res://feel/pop.tres and share it.
@export var duration: float = 0.25
@export var trans_type: Tween.TransitionType = Tween.TRANS_BACK
@export var ease_type: Tween.EaseType = Tween.EASE_OUT
@export var overshoot_scale: float = 1.15


func apply(tween: Tween) -> Tween:
	return tween.set_trans(trans_type).set_ease(ease_type)
```

```gdscript
extends Button

## Assign the preset resource above (its script is the preset class).
@export var feel: Resource


func _ready() -> void:
	pressed.connect(_pop)


func _pop() -> void:
	if feel == null:
		return
	var duration: float = feel.get("duration")
	var peak: float = feel.get("overshoot_scale")
	pivot_offset = size * 0.5
	var t: Tween = feel.call("apply", create_tween())
	t.tween_property(self, "scale", Vector2.ONE * peak, duration * 0.4)
	t.tween_property(self, "scale", Vector2.ONE, duration * 0.6)
```

In a project, give the preset a `class_name` (for example `TweenFeel`) and
type the export as that class instead of `Resource`.

## 6. Hard Limits of the API

| Do not | Because |
|---|---|
| `Tween.new()` | A tween made that way is invalid. Use `create_tween()` on a node or `get_tree().create_tween()`. |
| Reuse a finished tween | A tween is single-use. Make a new one to replay. |
| `PropertyTweener.new()` (or any tweener) | Tweeners come only from the `tween_*()` methods. |
| An infinite loop (`set_loops()`) whose steps all take 0 s | It never yields a frame; the engine stops it with an error. Give at least one step a duration. |
| Create a tween each frame in `_process()` | Hundreds of tweens fight over one property. Keep one reference, and kill and replace it. |
| Use a 0-second tween to set a value | Set the property directly. |
| Forget `bind_node()` for a tween made by `get_tree().create_tween()` | An unbound tree tween follows no node: it ignores that node's pause and process mode, and it is not killed with the node. `create_tween()` on a node binds it already. |
