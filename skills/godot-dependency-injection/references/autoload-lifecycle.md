# Autoload boot order, two-phase start and non-node services

Adds the measured start-up order of autoloads, a two-phase start for
autoloads that depend on each other, `static var` state, engine singletons
for non-node services, thread access and a headless health check. Read it
when you add an autoload, when one autoload calls another at start-up, or
when an autoload is `null` or half set up in some code path. Every order
below was measured on Godot 4.7.2.

## Start-up order

For autoloads `First` then `Second` (their order in Project Settings >
Globals > Autoload) and a main scene `Main`, Godot runs:

1. `First._init()`, then `Second._init()`. The nodes are not in the tree
   yet: `is_inside_tree()` is `false` and `get_tree()` is not usable.
2. Both autoloads and the main scene enter the tree. In
   `First._enter_tree()`, `Second` is already in the tree.
3. `First._ready()`, then `Second._ready()`, then `Main._ready()`. In
   `First._ready()`, `get_tree().current_scene` is already `Main`, but
   `Second._ready()` has not run.

So the rules are:

- In `_init()`, touch no other autoload and no tree. Set plain fields only.
- In `_ready()`, an autoload can *get* a reference to an autoload below it
  in the list, but must not *use* state that the lower one sets up in its
  own `_ready()`. It can use everything of an autoload above it.
- Gameplay scenes can use every autoload in `_ready()`: all of them are
  ready before the main scene.

Put an autoload that others need (config, a service registry) at the top
of the list. If two autoloads need each other at start-up, use the
two-phase start below, not list order.

## Two-phase start

Each autoload sets itself up alone in `_ready()`. A bootstrap step then
connects them, after every autoload is ready. The last autoload in the list
runs it, because its `_ready()` runs after all others.

```gdscript
extends Node
## Autoload "Boot", the last entry in the autoload list.

signal services_started

var started: bool = false


func _ready() -> void:
	for child in get_tree().root.get_children():
		if child != self and child.has_method(&"start_service"):
			child.call(&"start_service")
	started = true
	services_started.emit()
```

An autoload that needs another one does that work in `start_service()`,
not in `_ready()`. Code that can run before the boot step (rare: only other
autoloads) checks `Boot.started` or awaits `Boot.services_started`.

## Plain shared state without a node

When global state needs no `_process()`, no signals and no tree, a
`class_name` script with `static var` is simpler than an autoload. It needs
no registration and costs no node.

```gdscript
class_name RunStats
extends RefCounted

static var enemies_defeated: int = 0
static var best_time: float = INF


static func reset() -> void:
	enemies_defeated = 0
	best_time = INF
```

Static values live as long as the engine. A scene change or
`reload_current_scene()` does not reset them, so call `reset()` on "new
game" and in each test's setup. `@export` and `@onready` do not work on a
`static var`.

## Non-node services with `Engine.register_singleton`

`Engine.register_singleton(name, object)` makes an object reachable through
`Engine.get_singleton(name)`. It is not a GDScript global name: code must
call `Engine.get_singleton()`. Measured on 4.7.2: registering a
`RefCounted` prints `RefCounted singleton '...' will be disallowed soon;
raw pointer will dangle when last Ref is released. Use Object singleton.`
So register an `Object` subclass, keep it alive, and free it yourself.

```gdscript
extends Node
## Autoload "Services", near the top of the autoload list.

class Analytics extends Object:
	var events: PackedStringArray = []

	func track(event_name: String) -> void:
		events.append(event_name)


var _analytics: Analytics


func _enter_tree() -> void:
	_analytics = Analytics.new()
	Engine.register_singleton(&"Analytics", _analytics)


func _exit_tree() -> void:
	if Engine.has_singleton(&"Analytics"):
		Engine.unregister_singleton(&"Analytics")
	_analytics.free()
```

For most projects the service locator in `service-locator.md` is the better
choice: it is plain GDScript, is easy to swap in tests, and has no engine
warning. Use the engine registry when an editor plugin or GDExtension code
must find the service too.

## Threads

Autoload state that a worker thread reads or writes needs a `Mutex`. A
worker thread must never change the scene tree: hand the change to the main
thread with `call_deferred()`.

```gdscript
extends Node

signal chunk_ready(chunk_id: int)

var _mutex := Mutex.new()
var _finished: Array[int] = []


func mark_finished(chunk_id: int) -> void:
	# Safe to call from any thread.
	_mutex.lock()
	_finished.append(chunk_id)
	_mutex.unlock()
	chunk_ready.emit.call_deferred(chunk_id)


func finished_count() -> int:
	_mutex.lock()
	var n := _finished.size()
	_mutex.unlock()
	return n
```

`chunk_ready.emit.call_deferred(...)` makes the listeners run on the main
thread, where they may touch nodes.

## Paused game

An autoload that must work while `get_tree().paused` is `true` (music, a
debug console, the pause menu controller) sets
`process_mode = Node.PROCESS_MODE_ALWAYS`. The default mode inherits from
the root, which stops when the tree is paused.

## Never free an autoload

Other code holds the autoload by its global name for the whole run. Do not
`queue_free()` it or remove it from the root. To "reset" one, give it a
`reset()` method.

## A headless health check

Prove that the autoloads exist and start clean, with the exit code as the
result:

```gdscript
extends SceneTree

const REQUIRED: Array[StringName] = [&"EventBus", &"GameState"]


func _initialize() -> void:
	await process_frame
	var missing: Array[StringName] = []
	for autoload_name in REQUIRED:
		if root.get_node_or_null(NodePath(String(autoload_name))) == null:
			missing.append(autoload_name)
	if not missing.is_empty():
		push_error("missing autoloads: %s" % [missing])
		quit(1)
		return
	print("autoloads OK")
	quit(0)
```

Run it with `--headless --path <project> --script res://<file>.gd`. The
`await process_frame` matters: when `_initialize()` starts, the autoload
nodes are children of `root`, but `root` is not inside the tree yet and no
`_ready()` has run (measured on 4.7.2).
