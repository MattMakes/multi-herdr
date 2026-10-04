# Connection flags, bound arguments and connection lifetime

Adds what the SKILL.md leaves out about `connect()`: the flags, how bound
and unbound arguments line up, when Godot removes a connection for you, and
how to move a connection to a new emitter. Read it before you write a
connection that is not a plain `signal.connect(method)` in `_ready()`.
Every behavior below was measured on Godot 4.7.2.

## Flags

Pass flags as the second argument of `Signal.connect()`. Combine them with
`|`.

| Flag | Effect |
| --- | --- |
| `CONNECT_DEFERRED` | The call runs at idle time, after the current frame's processing, not inside `emit()`. |
| `CONNECT_PERSIST` | The editor saves the connection in the scene file. Code connections rarely need it. |
| `CONNECT_ONE_SHOT` | Godot removes the connection after the first call. |
| `CONNECT_REFERENCE_COUNTED` | The same callable can connect more than once. Each `disconnect()` removes one count. |
| `CONNECT_APPEND_SOURCE_OBJECT` | The emitter is passed as one more argument, after the signal's own arguments. |

Use `CONNECT_DEFERRED` when the handler changes physics state or frees
nodes, for example in a `body_entered` handler. Use `CONNECT_ONE_SHOT` for
"the first time this happens" logic.

## Connecting twice is an error

A second `connect()` of the same signal to the same callable fails. It
prints `Signal '...' is already connected` and returns
`ERR_INVALID_PARAMETER`. Guard code that can run more than once:

```gdscript
extends Node

signal sensor_triggered


func watch(sensor: Node) -> void:
	if not sensor.is_connected(&"ready", _on_sensor_ready):
		sensor.ready.connect(_on_sensor_ready)


func _on_sensor_ready() -> void:
	sensor_triggered.emit()
```

Two callables are "the same" when the object, the method and the bound
arguments match. Two separate `_on_x.unbind(1)` values match too, so the
second connect fails.

`CONNECT_REFERENCE_COUNTED` counts only connections made with the flag.
Measured: two flagged connects need two `disconnect()` calls. A plain
connect followed by a flagged connect is removed by one `disconnect()`.

## Bound and unbound arguments

`bind()` appends values after the signal's arguments. `unbind(n)` drops the
last `n` signal arguments. `CONNECT_APPEND_SOURCE_OBJECT` adds the emitter
after the signal's arguments.

```gdscript
extends Node

signal hit(amount: int)


func _ready() -> void:
	hit.connect(_on_hit_tagged.bind("fire"))
	hit.connect(_on_hit_from, CONNECT_APPEND_SOURCE_OBJECT)
	hit.connect(_on_any_hit.unbind(1))
	hit.emit(7)


func _on_hit_tagged(amount: int, element: String) -> void:
	print("%d %s damage" % [amount, element])


func _on_hit_from(amount: int, source: Object) -> void:
	print("%d damage from %s" % [amount, source])


func _on_any_hit() -> void:
	print("something was hit")
```

One handler for many buttons is the common use of `bind()`:

```gdscript
extends VBoxContainer

signal choice_made(index: int)


func _ready() -> void:
	for i in get_child_count():
		var button := get_child(i) as Button
		if button != null:
			button.pressed.connect(_on_choice.bind(i))


func _on_choice(index: int) -> void:
	choice_made.emit(index)
```

## When Godot removes a connection

Measured on 4.7.2:

- When the listener object is freed, Godot removes its connections. A later
  `emit()` does not call it and prints nothing.
- A lambda counts as a method of the object whose code created it. When
  that object is freed, the lambda's connection goes too, even if the
  lambda captures locals.
- A lambda can capture a *different* object. If that object is freed while
  the lambda's creator lives, the connection stays. The next call prints
  `Lambda capture at index 0 was freed. Passed "null" instead.` and runs
  with `null`.

So the risk is not "lambdas leak". The risk is a lambda that captures an
object with a shorter life than the lambda's creator. Use a method on the
short-lived object, or check `is_instance_valid()` in the lambda, or
disconnect when that object leaves.

A connection to an autoload bus is removed when the listener is freed. The
SKILL.md's `_exit_tree()` disconnect is still right for a node that leaves
the tree and comes back (a pooled enemy, a reparented node): without it,
the node gets bus events while it is out of the tree.

## Moving a connection to a new emitter

A camera that follows "the current target", a HUD that shows "the selected
unit": disconnect the old emitter before you connect the new one.

```gdscript
extends Label

var _target: Node


func track(target: Node) -> void:
	if _target != null and is_instance_valid(_target):
		if _target.tree_exiting.is_connected(_on_target_leaving):
			_target.tree_exiting.disconnect(_on_target_leaving)
	_target = target
	if _target != null:
		_target.tree_exiting.connect(_on_target_leaving)
	text = str(_target.name) if _target != null else ""


func _on_target_leaving() -> void:
	_target = null
	text = ""
```

## Emit, then free

Emit a signal before the emitter frees itself, never after. Listeners often
read the emitter's state in the handler. Use `queue_free()`, not `free()`,
after the emit, so handlers that run in the same frame still see a valid
object.

```gdscript
extends Node2D

signal died(at: Vector2)


func kill() -> void:
	died.emit(global_position)
	queue_free()
```

## Wait for a bus event with `await`

`await` on a bus signal suspends the function until the next emit. The
value is the signal's argument (or an `Array` when the signal has more than
one argument).

```gdscript
extends Node

signal level_completed(level_id: int)


func run_level(level_id: int) -> void:
	var done_id: int = await level_completed
	while done_id != level_id:
		done_id = await level_completed
	print("level %d done" % done_id)
```

If the awaiting node is freed first, the function never resumes. Do not put
cleanup after an `await` that may never return. The `godot-gdscript-advanced`
skill shows a timeout race.

## Do not stream data through signals

A signal call costs more than a direct call: Godot looks up every
connection and packs the arguments. For a value that changes every frame
for many objects (positions of 500 bullets), let the reader poll a shared
array or a direct reference. Keep signals for events.
