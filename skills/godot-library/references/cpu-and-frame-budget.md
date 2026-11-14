# CPU and frame budget

Target engine: Godot 4.7. This shelf covers where CPU time goes in a frame and the ways to spend
less of it: node count, callbacks, update rates, data layout, tick rate and frame caps.

## Use this shelf when

- You plan a system with hundreds or thousands of entities and must choose its shape first.
- The profiler shows script or SceneTree time as the largest part of the frame.
- A game is smooth most of the time but stalls now and then, or loads slowly.
- You want to lower the physics tick rate, cap the frame rate or save battery.
- You must give each system a cost in milliseconds and keep it there.

## How the engine works here

One pass of the main loop runs zero or more physics ticks, then one process step, then rendering.
The number of ticks per rendered frame follows real time: `Engine.physics_ticks_per_second`
(default 60) against the frame rate, limited by `Engine.max_physics_steps_per_frame` (default 8).
So a slow frame is followed by a frame with more physics ticks in it. When rendering is slower
than 1/8 of the tick rate, the game runs in slow motion even if all code uses `delta`.

The frame budget is 16.67 ms at 60 Hz and 8.33 ms at 120 Hz. CPU and GPU work side by side, so
the frame time is the larger of the two, not the sum. Making the CPU part faster changes nothing
when the GPU is the limit, and the other way round.

The docs name three kinds of slowness. Treat them as three problems: a cost paid every frame, a
spike now and then, and a wait outside play (loading). A mean frame rate hides the second kind.

Node cost: `_process` and `_physics_process` are sent through the tree, and the renderer handles
each node by itself. The docs put the point where this housekeeping hurts at "thousands to tens
of thousands" of nodes, by platform. A script that overrides `_process` has processing turned on
before `_ready`, so an empty `_process` on 5,000 nodes is 5,000 script calls per frame.

Transforms: a change to a `Node3D` transform is not sent to the servers at once. Changes are
collected and applied once at the end of the frame. `Node3D.force_update_transform()` forces it
early and walks the children. A node gets `NOTIFICATION_TRANSFORM_CHANGED` only if it asked with
`set_notify_transform(true)`; most visual and collision nodes ask by themselves.

Built-in engine functions cost the same from every language. Only the code between the engine
calls differs. A GDScript loop that calls one engine function per item pays for the loop.

## Advanced techniques

### Budget in milliseconds per system

Give each system a number, for example AI 2 ms, physics 3 ms, UI 1 ms, and leave a margin (this
split is an example, not from a source). Time the system with `Time.get_ticks_usec()` and
publish it as a custom monitor of type `Performance.MONITOR_TYPE_TIME`, which takes seconds. See
`debugging-and-profiling-tools`. Time many runs and use the mean: one run is distorted by timer
accuracy, by the scheduler and by cold CPU caches.

### Do the bottleneck math first

A 9x speedup of a 9 ms function in a 10 ms frame makes the frame 5x faster. The same speedup in a
59 ms frame gives 51 ms. Find the largest part, fix it, measure again. The next largest part is
often a different system.

### Turn processing off, do not branch inside it

`set_process(false)` and `set_physics_process(false)` remove the node from the callback list. An
early `return` inside `_process` still costs the call. For a whole subtree, set `process_mode`
to `Node.PROCESS_MODE_DISABLED`; `CollisionObject3D.disable_mode` says what physics does then.
`VisibleOnScreenEnabler2D` and `VisibleOnScreenEnabler3D` do this by visibility: `enable_node_path`
picks the target and `enable_mode` the mode used while on screen. Trap: a new notifier needs one
frame before `is_on_screen()` is right, and off-screen simulation stops, which changes gameplay.

### Detach instead of hide

The docs call it a quirk of the SceneTree: `remove_child()` on a branch, with a kept reference,
can be much cheaper than a paused or hidden branch, because a detached branch gets no callbacks
and no notifications. Use it for rooms, chunks and pooled enemies. Cost: `_exit_tree` and
`_enter_tree` run each time. Trap: a detached node that nobody frees is an orphan and leaks.
Watch `Performance.OBJECT_ORPHAN_NODE_COUNT`.

### One manager instead of N scripts

Move per-entity state out of node scripts into arrays owned by one node. Packed arrays hold their
values in one block of memory, so a linear pass reads memory in order.

```gdscript
extends Node2D

var positions: PackedVector2Array = PackedVector2Array()
var velocities: PackedVector2Array = PackedVector2Array()

func _physics_process(delta: float) -> void:
	for i: int in positions.size():
		positions[i] += velocities[i] * delta
```

It pays when many entities run the same simple rule. The step after that is to draw them without
nodes too: see `servers-and-low-level`. Cost: no per-entity scene, no inspector, no signals.

### Update-rate tiers

Not every entity needs every tick. Give each a slot and run it when
`(Engine.get_physics_frames() + slot) % period == 0`. Use period 1 near the player, 4 at mid
range, 16 far away. Multiply `delta` by the period. The slot spreads the work, so no single tick
gets all of it. Trap: a tier change must not make an entity skip or double a step.

### Time-sliced work queue

For work that may take any number of frames (path requests, chunk builds, save data), run jobs
until a time limit is reached and keep the rest for the next frame.

```gdscript
extends Node

const BUDGET_USEC: int = 1500
var _jobs: Array[Callable] = []
var _head: int = 0

func push(job: Callable) -> void:
	_jobs.push_back(job)

func _process(_delta: float) -> void:
	var deadline: int = Time.get_ticks_usec() + BUDGET_USEC
	while _head < _jobs.size() and Time.get_ticks_usec() < deadline:
		_jobs[_head].call()
		_head += 1
	if _head == _jobs.size():
		_jobs.clear()
		_head = 0
```

A read cursor is used because `pop_front()` moves every element. Each job must be small; one
5 ms job breaks a 1.5 ms limit. For work that can leave the main thread, see
`threads-and-concurrency`.

### Lower the physics tick rate, add interpolation

CPU use grows about in step with `Engine.physics_ticks_per_second`. Some games run well at 30 or
20 ticks. The costs are jitter and more input lag; below about 30, physics can break down. The
answer to jitter is the built-in physics interpolation
(`ProjectSettings.physics/common/physics_interpolation`), which the docs call orders of magnitude
cheaper than a tick. For games with real-time player movement the docs advise to stay at 60.
With your own interpolation or in a network game, set `Engine.physics_jitter_fix` to 0. If you
raise the tick rate far above 60, raise `Engine.max_physics_steps_per_frame` too.

### Pick the cheapest spatial lookup

"Who is near me" done as all-against-all grows with the square of the count. Options, cheapest to
set up first: reuse the physics broadphase with a direct space query
(`PhysicsDirectSpaceState2D.intersect_shape()` with a small `max_results`), keep your own uniform
grid in a `Dictionary` keyed by cell, or test only against a short list (the few players) when
that is all the rule needs. A grid wins when entities are not physics bodies at all. Measure:
the docs give no threshold.

### Keep transform cost low

- Set `global_transform` once, not position, then rotation, then scale.
- Call `set_notify_transform(true)` only on nodes that must react, and
  `set_ignore_transform_notification(true)` while you write the transform inside that reaction.
- `top_level = true` cuts a node off from its parent's transform. Use it for trails, projectiles
  and UI markers that live under a moving parent but move alone.
- Avoid `force_update_transform()` in loops. Read state from the body or the query result instead.
- A deep moving tree costs more than a flat one: each child's global transform must be rebuilt.

### Cap the frame rate, sleep when idle

`Engine.max_fps` caps rendering; 0 means no cap. With V-Sync on, V-Sync wins. For menus, tools and
turn-based games use `OS.low_processor_usage_mode`: the screen is redrawn only when something
changed, with `OS.low_processor_usage_mode_sleep_usec` (default 6900) between frames. If the CPU
stays busy, run with `--debug-canvas-item-redraw` (debug builds) to see which item asks for
redraws each frame. An animated shader or a spinner is the usual cause.

### Remove hitches at the source

- Allocate at load time: pool nodes and reuse them. In C# also avoid garbage per frame; the docs
  warn of GC pauses and name object pooling as the usual answer.
- Precompute: at level load, from a data file, or as a script constant.
- A stall at the first draw of a material is pipeline compilation: see `rendering-pipeline`.
- A stall at `add_child` of a large scene: build it off-tree and add it in parts over frames.

### Measure startup and load time

`--quit` exits after the first iteration, so an outside timer or profiler measures pure startup.
`--benchmark` prints run-time timings of startup steps and `--benchmark-file <path>` writes them
as JSON; the docs list both for editor builds only.

## Niche but useful

- `--frame-delay <ms>` adds a delay to each frame to imitate a slow CPU. It finds code that
  breaks at low frame rates. It is not a frame limiter.
- `--fixed-fps <fps>` gives every frame the same `delta` and turns real-time sync off: use it
  for repeatable runs.
- `Node.process_priority` and `process_physics_priority` order callbacks without tree changes.
- `Node.process_thread_group` moves a subtree's callbacks to a sub-thread; rules are in
  `threads-and-concurrency`.
- `ProjectSettings.application/run/delta_smoothing` filters `delta` under V-Sync. Turn it off
  (`--delta-smoothing disable`) when you compare raw frame times.

## Traps

- Measuring with V-Sync on or with a frame cap: the frame time shows the cap, not the cost.
- Judging by FPS. 10 FPS lost at 300 FPS is 0.1 ms; at 60 FPS it is 3 ms. Use milliseconds.
- Calling getters on servers each frame: they may run async and the call makes them catch up.
- Outdated belief: "hide it and it is free". A hidden node still processes.
- Outdated belief: "premature optimization" means design can wait. The docs say the opposite:
  data layout and algorithm are chosen at design time and no later tuning replaces them.
- Timing in the editor build and reporting it as shipping cost.

## Version notes

- 4.4: physics interpolation became available in 3D (2D has it since 4.3).
- 4.5: 3D interpolation moved into the SceneTree (reported by the 4.5 dev 4 post; no project
  change needed). `ProjectSettings.physics/3d/physics_interpolation/scene_traversal` picks the
  traversal method; keep `DEFAULT`.
- 4.6: ObjectDB profiler for object-count and leak checks (see `debugging-and-profiling-tools`).
- 4.7: no change to the items on this shelf was found in the docs.

## Related skills

- `godot-optimization` - profiler steps, pooling and the common bottleneck table.
- `godot-perf-measure` - A/B frame-time method, hitch maps and cost budget tests.
- `godot-multithreading` - WorkerThreadPool and thread rules for work that leaves the main thread.
- `godot-physics-system` - body types, layers and shapes that decide physics cost.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/performance/general_optimization.html
- https://docs.godotengine.org/en/stable/tutorials/performance/cpu_optimization.html
- https://docs.godotengine.org/en/stable/tutorials/performance/using_servers.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/index.html
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- Class reference 4.7: Engine, Node, Node3D, OS, Performance
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-4/ (search summary only; page blocked)
