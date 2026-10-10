# The orchestrator root and more gameplay components

Adds the orchestrator rule for the entity's root script, input, movement
and visual components, status effects as child nodes, a component registry,
setup validation that also works in release builds, and an isolation test.
Read it when you build an entity from more than the hit, hurt and health
components that the SKILL.md shows.

## The root script only wires

The entity's root script (`player.gd`, `enemy.gd`) is the orchestrator. It
holds typed references to its components, connects their signals and passes
data from one to the next. It holds no game rules: no damage math, no
movement math. A rule in the root cannot be reused by another entity.

| Direction | How |
| --- | --- |
| Root to component | A method call |
| Component to root | A signal |
| Component to component | Never directly. The root listens to one and calls the other. |

## Input, movement and visuals as components

Each one does one job and does not know which entity it is on.

```gdscript
class_name InputComponent
extends Node
## Reads devices. Moves nothing.

var move_direction: Vector2 = Vector2.ZERO
var jump_requested: bool = false


func poll() -> void:
	move_direction = Input.get_vector(&"move_left", &"move_right", &"move_up", &"move_down")
	jump_requested = Input.is_action_just_pressed(&"jump")
```

```gdscript
class_name MoveComponent
extends Node
## Turns a wanted direction into a velocity. The body is passed in.

@export var max_speed: float = 220.0
@export var acceleration: float = 1400.0
@export var friction: float = 1800.0


func step(body: CharacterBody2D, direction: Vector2, delta: float) -> void:
	var rate := acceleration if direction != Vector2.ZERO else friction
	body.velocity = body.velocity.move_toward(direction * max_speed, rate * delta)
	body.move_and_slide()
```

```gdscript
class_name FacingComponent
extends Node
## Flips a sprite to face the way the body moves. Knows nothing of input.

@export var sprite: Sprite2D


func update_from(velocity: Vector2) -> void:
	if sprite != null and not is_zero_approx(velocity.x):
		sprite.flip_h = velocity.x < 0.0
```

The root wires them, once per physics frame:

```gdscript
class_name TopDownPlayer
extends CharacterBody2D

@export var input: InputComponent
@export var mover: MoveComponent
@export var facing: FacingComponent


func _physics_process(delta: float) -> void:
	input.poll()
	mover.step(self, input.move_direction, delta)
	facing.update_from(velocity)
```

An enemy reuses `MoveComponent` and `FacingComponent` with an AI component
in place of `InputComponent`. Nothing else changes.

## Status effects as child nodes

A burn, a slow or a shield is a small node added under a holder node. The
effect owns its timer and removes itself. The holder only adds and lists.

```gdscript
class_name StatusEffect
extends Node
## Base for a timed effect. Subclasses override _on_tick and _on_end.

signal ended(effect: StatusEffect)

@export var duration: float = 3.0
@export var tick_interval: float = 0.5

var target: Node

var _elapsed: float = 0.0
var _since_tick: float = 0.0


func _process(delta: float) -> void:
	_elapsed += delta
	_since_tick += delta
	if _since_tick >= tick_interval:
		_since_tick -= tick_interval
		_on_tick()
	if _elapsed >= duration:
		_on_end()
		ended.emit(self)
		queue_free()


func _on_tick() -> void:
	pass


func _on_end() -> void:
	pass
```

```gdscript
class_name StatusHolder
extends Node

signal effect_added(effect: StatusEffect)


func apply(effect: StatusEffect, target: Node) -> void:
	for existing in get_children():
		if existing.get_script() == effect.get_script():
			existing.queue_free()
	effect.target = target
	add_child(effect)
	effect_added.emit(effect)


func has_effect(script: Script) -> bool:
	for existing in get_children():
		if existing.get_script() == script and not existing.is_queued_for_deletion():
			return true
	return false
```

This holder replaces an effect of the same type. A game that stacks effects
counts them instead.

## A component registry in the root

When a root has many components, it can index them once in `_ready()`.
The registry is private to the root. Components still never call each
other.

```gdscript
extends Node2D

var _by_type: Dictionary[StringName, Node] = {}


func _ready() -> void:
	for child in get_children():
		var script := child.get_script() as Script
		if script != null and script.get_global_name() != &"":
			_by_type[script.get_global_name()] = child


func component(type_name: StringName) -> Node:
	return _by_type.get(type_name)
```

`component(&"MoveComponent")` returns the node or `null`. The SKILL.md's
`ComponentUtils.get_component()` is the same idea without a cache.

## Validate the wiring in every build

A typed `@export` slot left empty is the most common component bug. Check
the slots in `_ready()`. `assert()` runs only in debug builds, so a release
build skips it. Use `push_error()` and a disabled state when an empty slot
must not crash a shipped game.

```gdscript
class_name GuardedPlayer
extends CharacterBody2D

@export var mover: MoveComponent
@export var input: InputComponent


func _ready() -> void:
	var missing: PackedStringArray = []
	if mover == null:
		missing.append("mover")
	if input == null:
		missing.append("input")
	if not missing.is_empty():
		push_error("%s: empty component slots: %s" % [name, ", ".join(missing)])
		set_physics_process(false)
```

## Isolation test

A component passes when it works on a bare node that is not its usual
entity. Build it in a headless check and drive it by its public methods:

```gdscript
extends SceneTree


func _initialize() -> void:
	await process_frame
	var body := CharacterBody2D.new()
	root.add_child(body)
	var mover := MoveComponent.new()
	body.add_child(mover)
	await physics_frame
	mover.step(body, Vector2.RIGHT, 0.1)
	if body.velocity.x <= 0.0:
		push_error("MoveComponent did not accelerate")
		quit(1)
		return
	print("MoveComponent OK")
	quit(0)
```

The two awaits matter. Measured on 4.7.2: in `_initialize()` the root is
not inside the tree yet, so an added node gets no `_enter_tree()` and a body
has no physics space (`move_and_slide()` prints `body->get_space()` is
null). After one frame the tree runs, and after a physics frame the body is
in the physics world.

A component that calls `get_parent()` for its entity type, or reads a
sibling by path, fails this test. Fix the component, not the test.
