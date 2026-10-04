Adds techniques for large counts after profiling has found the cost: MultiMesh instancing, drawing through RenderingServer without nodes, direct physics queries instead of many RayCast nodes, time-sliced work queues, staggered navigation updates and WorkerThreadPool batch jobs; read it when the profiler shows the cost scales with the number of entities.

# Scaling Techniques for Large Counts

> ← Back to [SKILL.md](../SKILL.md). Measure first: [measuring-performance.md](measuring-performance.md). Draw-call basics are in [draw-calls.md](draw-calls.md); pooling is in [memory-management.md](memory-management.md).

All code targets Godot 4.7.

---

## 1. Thousands of Copies of One Mesh: MultiMesh

A `MultiMeshInstance3D` (or `MultiMeshInstance2D`) draws every instance of
one mesh in one draw call. Each instance has a transform, and optionally a
color and a custom `Color` that the shader reads as `INSTANCE_CUSTOM`. Use it
for grass, rocks, crowds of simple agents, bullets and debris.

```gdscript
extends MultiMeshInstance3D

## Scatters `count` rocks on a square area. One draw call for all of them.
@export var rock_mesh: Mesh
@export var count: int = 5000
@export var area: float = 200.0


func _ready() -> void:
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = rock_mesh
	mm.instance_count = count
	var rng := RandomNumberGenerator.new()
	rng.seed = 1234
	for i in count:
		var basis := Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * rng.randf_range(0.5, 1.5))
		var origin := Vector3(rng.randf_range(-area, area) * 0.5, 0.0, rng.randf_range(-area, area) * 0.5)
		mm.set_instance_transform(i, Transform3D(basis, origin))
		mm.set_instance_color(i, Color.from_hsv(0.08, 0.3, rng.randf_range(0.5, 0.9)))
	multimesh = mm
```

Rules:

- Set `transform_format`, `use_colors` and `use_custom_data` **before**
  `instance_count`. On 4.7.2, changing them while `instance_count` is not 0
  fails with "Instance count must be 0 to toggle whether colors are used".
- To show fewer instances without reallocating, set
  `visible_instance_count`.
- For many moving instances, write the whole `buffer`
  (`PackedFloat32Array`, 12 floats per 3D transform, plus 4 per color and 4
  per custom data) once per frame instead of one `set_instance_transform()`
  call per instance.
- The whole MultiMesh is culled as one object. Split a huge field into
  chunks, so chunks out of view are skipped.
- Instances have no collision and no per-instance scripts. Keep gameplay
  state in arrays and write the transforms from it.
- With physics interpolation on, `set_buffer_interpolated()` takes the current
  and previous buffers so that motion stays smooth between ticks.

## 2. Drawing Without Nodes: RenderingServer

Each node costs memory and per-frame tree work even when it does nothing.
For thousands of simple sprites that need no scripts, create canvas items on
the `RenderingServer` directly and keep their RIDs. Free every RID yourself.

```gdscript
extends Node2D

## Draws `count` static sprites as server canvas items under this node.
@export var texture: Texture2D
@export var count: int = 2000
var _items: Array[RID] = []


func _ready() -> void:
	if texture == null:
		return
	var rng := RandomNumberGenerator.new()
	var size := texture.get_size()
	for i in count:
		var item := RenderingServer.canvas_item_create()
		RenderingServer.canvas_item_set_parent(item, get_canvas_item())
		RenderingServer.canvas_item_add_texture_rect(item, Rect2(-size * 0.5, size), texture.get_rid())
		var pos := Vector2(rng.randf_range(0.0, 1920.0), rng.randf_range(0.0, 1080.0))
		RenderingServer.canvas_item_set_transform(item, Transform2D(0.0, pos))
		_items.append(item)


func _exit_tree() -> void:
	for item in _items:
		RenderingServer.free_rid(item)
	_items.clear()
```

Move an item later with `canvas_item_set_transform()`. The texture resource
must stay referenced (here by the `@export`) while the items use it. The same
approach exists in 3D with `instance_create2()` and `instance_set_transform()`;
prefer MultiMesh there unless each object needs its own mesh.

## 3. Many Rays: Query the Space Directly

A `RayCast3D` node updates every physics frame whether or not you read it.
For many agents that need a line-of-sight test now and then, query the
physics space directly when you need the answer.

```gdscript
extends Node3D

## Line-of-sight test without a RayCast node. Call from _physics_process().
@export_flags_3d_physics var blocker_mask: int = 1


func can_see(from: Vector3, to: Vector3, exclude: Array[RID] = []) -> bool:
	var query := PhysicsRayQueryParameters3D.create(from, to, blocker_mask, exclude)
	var hit := get_world_3d().direct_space_state.intersect_ray(query)
	return hit.is_empty()
```

Query the space only from `_physics_process()`, where the physics state is
safe to read. `intersect_shape()` and `intersect_point()` replace an `Area`
that exists only to answer one "who is here?" question.

## 4. Spread Work over Frames

Work that can wait (path requests, AI decisions, chunk updates) does not have
to finish in the frame it was asked for. Queue it, and spend a fixed time
budget per frame.

```gdscript
extends Node

## Runs queued Callables within `budget_usec` per frame.
@export var budget_usec: int = 1500
var _queue: Array[Callable] = []
var _head: int = 0


func submit(job: Callable) -> void:
	_queue.append(job)


func _process(_delta: float) -> void:
	var start := Time.get_ticks_usec()
	while _head < _queue.size() and Time.get_ticks_usec() - start < budget_usec:
		var job := _queue[_head]
		_head += 1
		if job.is_valid():
			job.call()
	if _head > 256 and _head * 2 > _queue.size():
		_queue = _queue.slice(_head)
		_head = 0
```

The read index avoids `pop_front()`, which shifts the whole array for each
job. Bound the budget well below the frame time (1–2 ms at 60 FPS).

## 5. Stagger Agent Updates

A hundred agents that all repath on the same frame make a spike every
second. Give each agent a phase, and update only one slice per frame.

```gdscript
extends CharacterBody3D

## Repaths every `repath_frames` frames, offset by this agent's id.
@export var repath_frames: int = 30
@export var target: Node3D
@onready var agent: NavigationAgent3D = $NavigationAgent3D
var _phase: int = 0


func _ready() -> void:
	_phase = int(get_instance_id() % repath_frames)


func _physics_process(_delta: float) -> void:
	if target and Engine.get_physics_frames() % repath_frames == _phase:
		agent.target_position = target.global_position
```

The same trick applies to perception checks, utility AI scoring and LOD
switches.

## 6. Batch Jobs on Worker Threads

For pure computation over many items (noise for a terrain chunk, a flow
field, mesh data), `WorkerThreadPool.add_group_task()` splits the index range
across CPU cores. The callable must not touch the scene tree; write results
into a preallocated packed array and apply them on the main thread.

```gdscript
extends Node

## Fills a height map on all cores, then applies it on the main thread.
var heights: PackedFloat32Array = PackedFloat32Array()
var _noise := FastNoiseLite.new()
var _width: int = 256


func build(width: int) -> void:
	_width = width
	heights.resize(width * width)
	var group := WorkerThreadPool.add_group_task(_compute_height, width * width)
	while not WorkerThreadPool.is_group_task_completed(group):
		await get_tree().process_frame
	WorkerThreadPool.wait_for_group_task_completion(group)
	_apply()


func _compute_height(index: int) -> void:
	var x := index % _width
	var y := index / _width
	heights[index] = _noise.get_noise_2d(float(x), float(y))


func _apply() -> void:
	print("height map ready: %d samples" % heights.size())
```

Each task writes only its own index, so no lock is needed. Call
`wait_for_group_task_completion()` once even after the poll loop: it
releases the task. Thread rules are in **godot-multithreading**.
