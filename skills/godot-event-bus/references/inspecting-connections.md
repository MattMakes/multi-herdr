# Inspecting connections and checking emits without a test framework

Adds a way to list who listens to a signal, and a small recorder that
proves a signal fired, with no GUT or gdUnit4. Read it when a handler does
not run, runs twice, or when a project has no test framework and you must
still prove a bus event in a headless check.

## Who is connected

`Signal.get_connections()` returns one `Dictionary` per connection, with
the keys `signal`, `callable` and `flags`. `Object.has_connections(name)`
is a quick yes or no.

```gdscript
class_name SignalReport
extends RefCounted


static func describe(source: Object) -> PackedStringArray:
	var lines := PackedStringArray()
	for info: Dictionary in source.get_signal_list():
		var signal_name: StringName = info["name"]
		for connection: Dictionary in source.get_signal_connection_list(signal_name):
			var callable: Callable = connection["callable"]
			lines.append("%s -> %s.%s (flags %d)" % [
				signal_name, callable.get_object(), callable.get_method(), connection["flags"]])
	return lines
```

Print `SignalReport.describe(EventBus)` when a handler does not run: the
listener is missing from the list (never connected, or freed), or it is
there twice under `CONNECT_REFERENCE_COUNTED`.

## Prove an emit in a headless check

A fleet worker proves a change with a headless run. This recorder connects
to any signal, counts the emits and keeps the last arguments. It works for
signals with 0 to 3 arguments.

```gdscript
class_name SignalRecorder
extends RefCounted

var count: int = 0
var last_args: Array = []


func watch(sig: Signal) -> void:
	var arity := _arity(sig)
	match arity:
		0:
			sig.connect(_record0)
		1:
			sig.connect(_record1)
		2:
			sig.connect(_record2)
		3:
			sig.connect(_record3)
		_:
			push_error("SignalRecorder: %d arguments not supported" % arity)


func _arity(sig: Signal) -> int:
	for info: Dictionary in sig.get_object().get_signal_list():
		if info["name"] == sig.get_name():
			return (info["args"] as Array).size()
	return -1


func _record0() -> void:
	_record([])


func _record1(a: Variant) -> void:
	_record([a])


func _record2(a: Variant, b: Variant) -> void:
	_record([a, b])


func _record3(a: Variant, b: Variant, c: Variant) -> void:
	_record([a, b, c])


func _record(args: Array) -> void:
	count += 1
	last_args = args
```

Use it from a `SceneTree` script and exit non-zero on a failure, because
the exit code is what the build check reads:

```gdscript
extends SceneTree


func _initialize() -> void:
	await process_frame
	var bus: Node = root.get_node_or_null(^"EventBus")
	if bus == null:
		push_error("EventBus autoload missing")
		quit(1)
		return
	var recorder := SignalRecorder.new()
	recorder.watch(bus.score_changed)
	bus.score_changed.emit(50)
	if recorder.count != 1 or recorder.last_args != [50]:
		push_error("score_changed: count %d, args %s" % [recorder.count, recorder.last_args])
		quit(1)
		return
	print("score_changed OK")
	quit(0)
```

Run it with `--headless --path <project> --script res://<file>.gd`.
Measured on 4.7.2: when `_initialize()` starts, the autoload nodes are
children of `root`, but the tree has not started. `root.is_inside_tree()` is
`false` and no `_ready()` has run. The `await process_frame` at the top waits
one frame, so every autoload is in the tree and ready. In a `--script` run there
is no main scene, so `current_scene` is `null`. The recorder is a
`RefCounted`, so the connection ends when the last reference to the
recorder goes.

## Common causes of a handler that does not run

| Symptom | Cause | Check |
| --- | --- | --- |
| Never runs | Connected to another instance (a second bus, a preloaded copy) | Compare `get_instance_id()` of the emitter and of the object you connected to |
| Never runs | The listener was freed | `describe()` no longer lists it |
| Runs one frame late | `CONNECT_DEFERRED` | The `flags` value includes 1 |
| Runs once only | `CONNECT_ONE_SHOT` | The `flags` value includes 4 |
| Runs twice | Connected under two different callables (a bound and an unbound one) | `describe()` lists two lines |
| Error "already connected" | A second plain `connect()` of the same callable | Guard with `is_connected()` |
