# Static typing, warnings that stop a load, and safe casts

Adds why typed code is faster, which GDScript warnings are errors by
default, typed loop variables and typed math helpers, `as` and `is`, the
constructor rule for scene scripts, `@static_unload`, and the script member
order. Read it when you write or review GDScript that must be typed, or
when a script fails to load with "Warning treated as error". Measured on
Godot 4.7.2 where marked.

## Why types make code faster

An untyped value is a `Variant`. Every operation on it checks the type at
run time. When the compiler knows the types, it emits typed instructions
that skip the check. The script editor marks such lines with a green line
number ("safe line"). Type hot paths first: `_process`, `_physics_process`
and loops.

```gdscript
extends Node2D

var _targets: Array[Node2D] = []


func nearest(from: Vector2) -> Node2D:
	var best: Node2D = null
	var best_dist := INF
	for target: Node2D in _targets:
		var d := from.distance_squared_to(target.global_position)
		if d < best_dist:
			best_dist = d
			best = target
	return best
```

`for target: Node2D in _targets` types the loop variable. `:=` infers the
type from a typed right side. Use the typed math helpers on typed values:
`absf`, `absi`, `clampf`, `clampi`, `minf`, `mini`, `maxf`, `maxi`,
`signf`, `signi`, `floorf`, `floori`, `ceilf`, `ceili`, `roundf`, `roundi`.
The plain `abs()` or `clamp()` returns a `Variant`.

## Warnings that are errors by default

These GDScript warnings default to level `2`. Measured on 4.7.2: each one
makes `load()` print `Parse Error: ... (Warning treated as error.)`, and
the script cannot be instantiated.

| Warning | Trigger | Fix |
| --- | --- | --- |
| `onready_with_export` | `@export @onready var x = $Node` | Pick one. `@export` for Inspector wiring, `@onready` for a fixed child. |
| `native_method_override` | A method with the name of an engine method, such as `get_class()` | Rename it. The engine never calls a script override of a non-virtual method. |
| `inference_on_variant` | `var x := some_variant` | Give the type: `var x: int = some_variant` |
| `get_node_default_without_onready` | `var n = $Child` as a member, without `@onready` | Add `@onready` |

The other warnings default to `1` (warn) or `0` (off). A level `1` warning
prints nothing in a headless run. `godot-project-setup` lists the keys that
make untyped code visible.

## `as`, `is` and `is_instance_of`

`x as T` returns `null` when `x` is an `Object` that is not a `T`. It
never raises. Check the result:

```gdscript
extends Area2D


func _ready() -> void:
	body_entered.connect(_on_body_entered)


func _on_body_entered(body: Node2D) -> void:
	var character := body as CharacterBody2D
	if character == null:
		return
	if character.has_method(&"apply_damage"):
		character.call(&"apply_damage", 10)
```

`x is T` tests the type and, inside an `if`, narrows the type of `x`. Use
`is_instance_of(x, type)` when the type is a variable. Measured:
`RefCounted.new() as Node` gives `null`.

`as` on a value that is not an object (for example a `String` in a
`Variant`) to an object type is a different case. Keep `as` for objects,
and test plain values with `is` or `typeof()`.

## Scene scripts get no constructor arguments

Godot calls `_init()` with no arguments when it instantiates a scene, loads
a resource, or duplicates either. Measured on 4.7.2: a node script whose
`_init()` needs an argument fails with `Method expected 1 argument(s), but
called with 0`. Give every `_init()` parameter a default, and pass setup
data through `@export` properties or a `setup()` method that the creator
calls after `instantiate()`:

```gdscript
class_name Projectile
extends Area2D

var _speed: float = 0.0
var _direction: Vector2 = Vector2.RIGHT


func setup(speed: float, direction: Vector2) -> Projectile:
	_speed = speed
	_direction = direction.normalized()
	return self


func _physics_process(delta: float) -> void:
	position += _direction * _speed * delta
```

`add_child(scene.instantiate().setup(400.0, aim))` reads well and keeps the
object valid before it enters the tree.

## `static var` and `@static_unload`

Static variables live on the script, not on an instance. A script with
static variables stays in memory for the whole run, so the values survive
scene changes. `@static_unload` at the top of the script lets Godot unload
the script, and reset its statics, once nothing references it. `@export`
and `@onready` do not apply to a `static var`.

When a static holds large data (a cache of resources), clear it yourself
when you are done with it. Do not rely on the unload.

## Member order in a script

Follow the order of Godot's style guide. Reviewers and other workers find
things faster.

1. `@tool`, `@icon`, `@static_unload`
2. `class_name`, then `extends`
3. The doc comment
4. Signals
5. Enums, then constants
6. Static variables
7. `@export` variables
8. Public variables, then private (`_`) variables
9. `@onready` variables
10. `_static_init()`, then other static methods
11. Built-in virtual methods (`_init`, `_enter_tree`, `_ready`, `_process`, ...)
12. Public methods, then private methods
13. Inner classes
