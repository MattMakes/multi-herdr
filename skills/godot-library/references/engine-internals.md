# Engine internals

Target engine: Godot 4.7. How the core is built (Object, Variant, ClassDB, servers, the main loop,
the message queue, signals, ids) and what that means for game code.

## Use this shelf when

- A bug depends on order: "why is the value one frame late", "why did this run after the free".
- You must choose between `_process`, `_physics_process`, a signal, a deferred call or a notification.
- You keep a reference to something that another system may free.
- A file was moved or renamed and references must survive (UIDs).
- You get "Parent node is busy setting up children" or a "flushing queries" error.

## How the engine works here

**Layers.** The scene layer (nodes, `SceneTree`) sits on the server layer (`RenderingServer`,
`PhysicsServer3D`, `AudioServer`, ...), which sits on drivers and the platform layer. A node is a
thin front end: it holds `RID` handles and forwards its properties to a server. A `RID` is opaque,
valid for this run only, and never freed for you when you created it through a server call.

**Object.** Every engine class derives from `Object` and is registered in `ClassDB` with its
methods, properties, signals and constants. An untyped `node.foo()` is resolved at run time: script
instance first, then `ClassDB`, walking up the inheritance chain. That lookup is the real cost of
untyped code. Ownership: `RefCounted` is freed at count 0; `Node` and plain `Object` by hand.

**Variant.** The value type of the scripting API: 24 bytes on 64-bit. Small types are copied by
value. `Array`, `Dictionary`, packed arrays and objects are shared by reference. Every type except
Nil and Object is non-nullable.

**StringName and NodePath.** `StringName` is an interned string: two equal names are the same
pointer. Method, signal, property, group and action names are `StringName`. Write the literal
`&"name"` so no conversion from `String` happens at run time. `NodePath` (`^"A/B:position:x"`) is a
pre-parsed path of names plus property subnames.

**One iteration of the main loop** (read from `main/main.cpp` and `scene/main/scene_tree.cpp`, 4.7):

| Step | What runs |
|---|---|
| 1 | 0..N physics ticks. N is limited by `physics/common/max_physics_steps_per_frame` (default 8). |
| 1a | Physics servers `flush_queries`: area and body signals from the previous step. |
| 1b | `SceneTree.physics_frame`, then `_physics_process` of all nodes, then a deferred-call flush. |
| 1c | Scene tree timers and tweens set to physics mode, then the `queue_free` delete queue. |
| 1d | Navigation servers, another flush, then the physics servers `step`. |
| 2 | `MultiplayerAPI` poll (if `SceneTree.multiplayer_poll`), `SceneTree.process_frame`, flush. |
| 3 | `_process` of all nodes, then a deferred-call flush. A pending scene change is applied here. |
| 4 | Scene tree timers and tweens (idle mode), then the `queue_free` delete queue. |
| 5 | Navigation `process`, then `RenderingServer` draw. Then audio update. |

Consequences: a physics signal handler runs before `_physics_process` of the same tick. Velocities
set in `_physics_process` are integrated later in that tick. `_process` runs after all physics
ticks. A frame can hold zero ticks (high FPS) or several (low FPS). Tween steps and
`SceneTreeTimer` timeouts run after `_process`.

**Order inside a callback pass.** Nodes are sorted by `process_priority` (lower first), ties broken
by tree order (parent before child). `process_physics_priority` does the same for the physics
pass. `_enter_tree` is top-down; `_ready` and `_exit_tree` are bottom-up.

**The message queue.** `call_deferred`, `set_deferred`, deferred signal connections and
`GROUP_CALL_DEFERRED` push a record into one global queue. A flush runs records until the queue is
empty, so a deferred call made from a deferred call still runs in the same flush. A method that
defers itself therefore loops forever. The queue buffer is limited by
`memory/limits/message_queue/max_size_mb` (default 32); watch `Performance.MEMORY_MESSAGE_BUFFER_MAX`.

**Signals.** Each object has a map signal name -> connection table keyed by `Callable`. On emit the
engine copies the connection list, disconnects every `CONNECT_ONE_SHOT` entry first, then calls each
`Callable` in turn, synchronously, on the emitting thread. So connecting or disconnecting during an
emission does not change who gets this emission. A target freed during the emission is skipped.

## Advanced techniques

### Pick the flush point on purpose

Deferred from `_physics_process` runs right after the physics pass of that tick, before the server
step. Deferred from `_process` runs before timers, tweens and drawing of the same frame. To really
wait one frame use `await get_tree().process_frame` (resumes at step 2 of the next iteration) or
`await get_tree().physics_frame`.

### queue_free is not call_deferred("free")

The delete queue is flushed after the deferred flush, timers and tweens. A node that called
`queue_free()` still receives deferred calls and tween steps in that frame. Check
`is_queued_for_deletion()` in handlers. Children of a queued node report `false`.

### Hold ids, not references, across time

A stored reference to a freed node is a dangling value; `is_instance_valid(x)` is the only safe
test. For long-lived links (targets, owners, pooled handles, thread messages) store the instance id.

```gdscript
extends Node

var _target_id: int = 0

func track(target: Node) -> void:
	_target_id = target.get_instance_id()

func get_target() -> Node:
	if not is_instance_id_valid(_target_id):
		return null
	return instance_from_id(_target_id) as Node
```

Ids are valid for this run only. Never save them or send them over the network.

### Use notifications that have no virtual method

`_notification(what)` receives much more than the named callbacks expose:

| Notification | Use |
|---|---|
| `NOTIFICATION_PARENTED` / `NOTIFICATION_UNPARENTED` | React to a parent change even outside the tree. |
| `NOTIFICATION_POST_ENTER_TREE` | Sent on every tree entry, just before a possible `_ready`. |
| `NOTIFICATION_SCENE_INSTANTIATED` | Root of a scene, right after `PackedScene.instantiate()`. |
| `NOTIFICATION_PATH_RENAMED` | Invalidate cached `NodePath` values. |
| `NOTIFICATION_PAUSED` / `NOTIFICATION_UNPAUSED` | Stop and resume audio, timers, network sends. |
| `NOTIFICATION_WM_CLOSE_REQUEST`, `NOTIFICATION_APPLICATION_PAUSED` | Quit flow, mobile autosave. |
| `NOTIFICATION_PREDELETE` | Destructor. Works on any `Object`, also `RefCounted` and `Resource`. |

In GDScript the engine calls `_notification` at every level of the script inheritance chain, base
first (reverse for `NOTIFICATION_PREDELETE`). Calling `super._notification(what)` runs the base
code twice.

### Connection flags

| Flag | Effect |
|---|---|
| `CONNECT_DEFERRED` | The call is queued and runs at the next flush. Arguments are copied now. |
| `CONNECT_ONE_SHOT` | Disconnected before the call, so the handler may reconnect itself. |
| `CONNECT_REFERENCE_COUNTED` | The same `Callable` may connect many times; each disconnect counts down. |
| `CONNECT_APPEND_SOURCE_OBJECT` | Since 4.5. The emitter is appended after the signal arguments. |

`CONNECT_DEFERRED` is the clean fix for "state changed while the physics server flushes queries":
connect `body_entered` deferred and the handler may add or remove bodies freely.
`CONNECT_APPEND_SOURCE_OBJECT` lets one handler serve many emitters without one `bind()` each.
`set_block_signals(true)` silences an object during bulk setup. Signal methods are thread-safe, but
handlers run on the emitting thread.

### Group calls as a cheap dirty flag

`get_tree().call_group_flags(SceneTree.GROUP_CALL_DEFERRED | SceneTree.GROUP_CALL_UNIQUE, &"hud",
&"refresh")` collapses many requests in one frame into one call per node. The arguments are not
compared: only the first request of the frame is kept.

### Reflection through ClassDB

`ClassDB.class_exists`, `ClassDB.instantiate`, `ClassDB.is_parent_class` and
`ClassDB.get_inheriters_from_class` cover engine classes only. Script classes (`class_name`) are
listed by `ProjectSettings.get_global_class_list()`. Use both for data-driven factories and
save-file type checks. Reflection is slow; build a table once at start-up.

### Custom MainLoop

`MainLoop` has `_initialize`, `_physics_process`, `_process` (`true` quits) and `_finalize`. Run
one with `--script file.gd`, with `--main-loop ClassName`, or set
`application/run/main_loop_type`. A script that extends `SceneTree` keeps nodes and adds a place
for code that must exist before any autoload. Good for command-line tools, bots and simulation
runners.

### UIDs for move-safe references

Every resource file has a `uid://` id (in the file header for `.tscn`/`.tres`, in the `.import`
file for imported assets, and since 4.4 in a `.uid` side file for scripts and shaders). `load()` and
`preload()` accept `uid://` strings. `ResourceUID.path_to_uid`, `ResourceUID.uid_to_path` and
`ResourceUID.ensure_path` convert. Since 4.4 `@export_file` stores a `uid://` string when set in the
Inspector; `@export_file_path` (since 4.5) keeps `res://`. Normalise with `ensure_path` first.

## Niche but useful

- `Engine.get_physics_frames()` and `Engine.get_process_frames()`: frame stamps for caches
  ("computed this tick already") and for staggered updates (`frame % 4 == slot`).
- `RenderingServer.frame_pre_draw` / `frame_post_draw`: hooks around step 5. For direct server
  use by `RID` see the shelf `servers-and-low-level`.
- `request_ready()`: makes `_ready` run again at the next tree entry (pooled nodes).
- `Object.get_incoming_connections()`: audit who listens to whom.
- `SceneTree.scene_changed`: emitted after the new scene is in the tree; `await` it after a change.
- `Input.flush_buffered_events()` with `input_devices/buffering/agile_event_flushing`: input is
  flushed before each physics tick instead of once per frame.

## Traps

- Belief "deferred = next frame". It is the next flush, usually microseconds later.
- Polling input in `_physics_process` can miss a press between two ticks; buffer it in `_input`.
- `add_child` on a parent that is still entering the tree fails with "Parent node is busy setting
  up children". Use `add_child.call_deferred(node)`.
- `is_instance_valid` returns `true` for a node that is queued for deletion.
- `physics/common/physics_jitter_fix` (default 0.5) lets tick timing drift from real time. Set 0
  for network or lockstep games. It is ignored when physics interpolation is on.
- Calling `String` methods on a `StringName` in a loop: each call converts to `String`.

## Version notes

- 4.4: `.uid` side files for scripts and shaders; `@export_file` stores `uid://`.
- 4.5: `@export_file_path`; `CONNECT_APPEND_SOURCE_OBJECT`; `SceneTree.scene_changed`.
- 4.6: scene files store a `unique_id` per node, so renamed or moved nodes keep their overrides in
  inherited and instanced scenes.
- 4.7: no change to the frame order above (checked against the 4.7 source).

## Related skills

- `godot-event-bus`: step-by-step signal bus set-up; this shelf explains emission order under it.
- `godot-dependency-injection`: autoload start-up order and service lookup.
- `godot-gdscript-advanced`: await and Callable idioms in script.

## Sources

- https://docs.godotengine.org/en/stable/engine_details/architecture/object_class.html
- https://docs.godotengine.org/en/stable/engine_details/architecture/variant_class.html
- https://docs.godotengine.org/en/stable/engine_details/architecture/core_types.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/godot_notifications.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/scene_tree.html
- https://docs.godotengine.org/en/stable/engine_details/file_formats/tscn.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.4.html
- https://github.com/godotengine/godot/blob/4.7-stable/main/main.cpp
- https://github.com/godotengine/godot/blob/4.7-stable/scene/main/scene_tree.cpp
- https://github.com/godotengine/godot/blob/4.7-stable/core/object/object.cpp
