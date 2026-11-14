# Threads and concurrency

Target engine: Godot 4.7. This shelf covers what may run off the main thread, which engine
parts are safe to call from there, and how to prove that a thread paid for itself.

## Use this shelf when

- A frame spike comes from your own CPU work (generation, pathfinding, simulation, parsing).
- You plan to load, build or bake content while the game keeps running.
- You must choose between `WorkerThreadPool`, a long-lived `Thread`, a node thread group or a
  server setting.
- A crash or a "can't be called from a thread" error appears only under load.
- The game ships on Web, where threads are off in the default export.

## How the engine works here

- The main thread owns the active `SceneTree`. Debug builds guard `Node` methods: a call from
  the wrong thread logs an error. Release builds do not check, so the same bug corrupts state.
- Deferred calls (`Object.call_deferred`, `Object.set_deferred`, `Callable.call_deferred`) go
  to a queue that the main thread flushes at idle time. Calls queued by one thread keep their
  order.
- `WorkerThreadPool` creates its threads at startup. `threading/worker_pool/max_threads` is
  `-1` by default: all logical cores on native platforms, 1 on Web.
  `threading/worker_pool/low_priority_thread_ratio` (0.3) reserves a share of the pool for
  low-priority tasks. The engine uses the same pool for its own jobs.
- Servers take commands by `RID` and do not need the tree. The navigation servers are safe
  and run queries in parallel. The physics and rendering servers are safe only with the
  settings below.
- A `Resource` is loaded once and shared. References may cross threads; changing one resource
  from 2 threads may not.
- `Array` and `Dictionary`: reading and writing existing elements from several threads is
  allowed. Anything that changes the size needs a `Mutex`.
- Signal methods on `Object` are thread-safe, but a callback runs on the emitting thread
  unless the connection uses `CONNECT_DEFERRED`.

## Advanced techniques

### Group tasks for data-parallel loops

`WorkerThreadPool.add_group_task(action, elements, tasks_needed, high_priority, description)`
calls `action` once per index from `0` to `elements - 1`, spread over the pool. Use it when
each index is independent and costly. Give every index its own output slot and never resize
inside the task.

```gdscript
extends Node

var _out: Array[float] = []
var _group_id: int = -1

func start(count: int) -> void:
	_out.resize(count)  # Fixed size before any task runs.
	_group_id = WorkerThreadPool.add_group_task(_work, count, -1, false, "roots")

func _work(index: int) -> void:
	_out[index] = sqrt(float(index))

func _process(_delta: float) -> void:
	if _group_id != -1 and WorkerThreadPool.is_group_task_completed(_group_id):
		WorkerThreadPool.wait_for_group_task_completion(_group_id)
		_group_id = -1
```

Cost: one script call per index. The class reference warns that cheap tasks get slower when
spread over threads, so batch (one index = one chunk of 256 items, not one item). Trap: every
task and group must be waited for once, even when it is done, or its memory is never freed.

### Poll, then wait

`WorkerThreadPool.wait_for_task_completion` blocks the caller. On the main thread that is a
frame hitch. Poll `is_task_completed` or `is_group_task_completed` each frame and call the
wait only after it returns `true`. `get_group_processed_element_count` gives a progress value.
Call the `is_*` methods only between adding the task and waiting for it.

### Nested tasks and ERR_BUSY

A task may add tasks. `wait_for_task_completion` then returns `ERR_BUSY` when waiting could
deadlock (the hard case is a task that waits on an older task), and `ERR_INVALID_PARAMETER`
for an ID that was already waited for. Check the return value. Prefer a flat design: the main
thread starts stage 2 after stage 1 is done.

### Long-lived Thread with a Semaphore

Use `Thread` for a worker that lives as long as a level: a streaming loader, a save writer, a
network pump. Creating a thread is slow, most of all on Windows, so create it during loading.
The worker sleeps in `Semaphore.wait` and the main thread wakes it with `Semaphore.post`.
To stop it: set a flag under the `Mutex`, post once, then call `Thread.wait_to_finish`.
A `Thread` that is freed without `wait_to_finish`, or while it holds a lock or waits on a
semaphore, can crash or deadlock.

### Ownership instead of locks

Locks cost time and invite deadlocks. Hand data over instead: the main thread fills a job
object and gives it away, the worker is the only writer until it gives a result back with
`call_deferred`. Use packed arrays for payloads; they are value types with copy-on-write, so
a receiver that writes gets its own copy. `Mutex` is reentrant (the same thread may lock it
again, with one `unlock` per `lock`). `Mutex.try_lock` and `Semaphore.try_wait` let the main
thread skip work instead of blocking.

### Build scene chunks off-thread, attach on main

Nodes outside the active tree are plain objects. A worker may load a `PackedScene`, call
`instantiate`, add children and set properties, then hand the root to the main thread with
`parent.add_child.call_deferred(root)`. The docs limit this to ONE such thread: 2 threads
that build scenes can touch the same shared resource. Trap: `_ready` and `_enter_tree` still
run on the main thread at attach time, so a heavy `_ready` moves the spike instead of
removing it.

### Threaded resource loading

`ResourceLoader.load_threaded_request(path, type_hint, use_sub_threads, cache_mode)` starts a
load. Poll `ResourceLoader.load_threaded_get_status` once per frame (its `progress` array
receives a 0.0 to 1.0 ratio) and call `ResourceLoader.load_threaded_get` when the status is
`THREAD_LOAD_LOADED`. Calling the getter early blocks. `use_sub_threads` loads dependencies in
parallel: faster, but it competes with the main thread, so keep it for loading screens.
The `godot-multithreading` skill has the step-by-step version.

### Node process thread groups

Set `Node.process_thread_group` to `PROCESS_THREAD_GROUP_SUB_THREAD` and that node, with all
children left on `PROCESS_THREAD_GROUP_INHERIT`, runs `_process` and `_physics_process` on a
worker. `process_thread_group_order` orders groups, so a main-thread group with a higher
order can collect results after the sub-thread groups. Inside the group, nodes may touch each
other; touching nodes outside it is an error in debug builds. To leave the group use
`call_deferred`, `call_thread_safe`, `set_thread_safe` or `notify_thread_safe`. To send work
into a group use `call_deferred_thread_group` or `set_deferred_thread_group`; the receiver
needs `process_thread_messages` set (`FLAG_PROCESS_THREAD_MESSAGES` and the physics variant).
It pays for many self-contained actors whose script logic dominates, not for actors that
mostly call into the tree, physics or rendering.

### Lifting the thread guards

`Thread.set_thread_safety_checks_enabled(false)` turns the engine guards off for the calling
thread, for `Thread` objects and pool tasks. It does not apply inside node group processing.
Use it only for objects that one thread owns alone, for example a detached subtree. The pool
resets the flag after the task.

### Server threading settings

| Setting | Default | Effect and cost |
| --- | --- | --- |
| `physics/2d/run_on_separate_thread`, `physics/3d/run_on_separate_thread` | off | Physics step on its own thread; physics API access only during physics process |
| `rendering/driver/threads/thread_model` | 1 | The Separate model is marked experimental with known crashes (particles, window resize) |
| `navigation/pathfinding/max_threads` | 4 | Parallel path queries per map; more threads cost memory and sync time |
| `rendering/limits/spatial_indexer/threaded_cull_minimum_instances` | 1000 | Below this, culling stays on 1 thread |

With Jolt, physics on a separate thread loses node names in error messages.

### GPU work from threads

Creating textures or reading image data back from a worker stalls, because the call must
sync with the `RenderingServer`. Compute on the worker (fill an `Image` or a
`PackedByteArray`), then upload on the main thread. `RenderingServer.call_on_render_thread`
runs a `Callable` where `RenderingDevice` access is legal.
`RenderingServer.create_local_rendering_device` gives a private device for compute on your own
thread; it cannot draw to the screen or share data with the main device.

### Navigation baking

Parsing geometry reads the tree, so `NavigationServer3D.parse_source_geometry_data` runs on
the main thread. `NavigationServer3D.bake_from_source_geometry_data_async` then bakes on a
thread. If the source geometry is built from plain arrays in script, the whole job can leave
the main thread. See the `navigation-advanced` shelf.

## Niche but useful

- `WorkerThreadPool.get_caller_task_id` returns `-1` off the pool: a cheap "am I on a worker"
  test. `Thread.is_main_thread` answers the other question.
- `OS.get_thread_caller_id` and `OS.set_thread_name` make logs and native profilers readable.
- `OS.has_feature("threads")` and `"nothreads"` select the fallback path at run time.
- `MultiMesh.buffer`: fill one `PackedFloat32Array` on workers, assign it once on main.
- `AStar2D`, `AStar3D` and `AStarGrid2D` are not thread-safe: one thread per object.
- A custom `Logger` (since 4.5) gets calls from any thread; guard its state with a `Mutex`.
- `StringName` creation takes a global lock; make names before the parallel part.

## Traps

- No `RWLock` is exposed to scripts in 4.7. It exists only in engine C++. Use `Mutex`.
- Freeing a locked `Mutex`, or a `Semaphore` that a thread waits on, is undefined.
- Web: the default export has no thread support (single-threaded since 4.3). Turning Thread
  Support on needs cross-origin isolation headers, which block most ads and third-party
  embeds; the PWA export option can inject the headers. Design a one-thread path first.
- Measure in a release export on the weakest target; threads do not always help.
- C# and C++ code may use its own threading classes; the scene-tree rules still apply.

## How to measure that a thread helped

Time the work with `Time.get_ticks_usec` on both paths and log total wall time and main
thread frame time. Publish both with `Performance.add_custom_monitor`. Compare high
percentiles, not the mean: a thread that cuts the average but adds lock stalls is a loss.
Test with the pool busy, because the engine shares it. See the `cpu-and-frame-budget` shelf.

## Version notes

- 4.3: single-threaded Web export became the default.
- 4.5: `Logger` class with thread-called virtual methods. Navigation map regions update on
  threads by default (`navigation/world/region_use_async_iterations`).
- 4.4 to 4.7: no change to the script API of `Thread`, `Mutex`, `Semaphore` or
  `WorkerThreadPool` was found in the docs.

## Related skills

- `godot-multithreading` - step-by-step pool, thread and threaded-loading procedures.
- `godot-optimization` - time-sliced queues and group tasks in a wider tuning pass.
- `godot-perf-measure` - a repeatable way to capture frame times before and after.
- `godot-export-pipeline` - Web export options, including thread support.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/performance/using_multiple_threads.html
- https://docs.godotengine.org/en/stable/tutorials/performance/thread_safe_apis.html
- https://docs.godotengine.org/en/stable/tutorials/io/background_loading.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationmeshes.html
- https://docs.godotengine.org/en/stable/classes/class_workerthreadpool.html
- https://docs.godotengine.org/en/stable/classes/class_thread.html
- https://docs.godotengine.org/en/stable/classes/class_node.html
