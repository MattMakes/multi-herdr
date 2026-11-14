# Worlds and streaming

Target engine: Godot 4.7. How to run large or dense worlds without load hitches: background loading, chunking, precision, distance culling, and keeping memory and far-away work bounded.

## Use this shelf when

- A level is too big to load at once, or the world is endless or generated.
- The game hitches when the player crosses into a new area or a new enemy type appears.
- Objects jitter far from the origin, or the map is larger than a few kilometers.
- Memory grows as the player explores, or a loading screen freezes.

## How the engine works here

- There is no built-in world streaming. You build it from threaded resource loading, scenes as chunks, visibility ranges, and process/physics disabling.
- There is also no texture or mesh streaming in 4.7 (no progressive mip or LOD loading from disk). The project lead named streaming the most important missing feature for open worlds in 2023, and it is still a listed rendering priority. Consequence: every texture and mesh a loaded chunk references is in memory at full size. Chunk size, texture resolution and unloading are your memory controls.
- Scene nodes are meant for complex behavior on hundreds of objects, not tens of thousands (same article). Dense worlds put the masses in `MultiMesh`, servers or plain data, and keep nodes for what is near the player.
- Loading a chunk has four costs with different homes. (1) Disk read and resource creation: can run on threads. (2) `PackedScene.instantiate()`: builds nodes; allowed off the main thread while the nodes are outside the tree. (3) `add_child`: main thread only; runs `_enter_tree`, `_ready`, creates physics bodies and render instances. (4) GPU pipeline compilation for new mesh/material combinations. Most hitches are (3) and (4).
- Resources are reference counted and cached by path. A second `load` of a cached path is free. A resource leaves memory when the last reference drops; there is no "unload" call.
- Since 4.4, pipelines are precompiled when a mesh is loaded (also on a background thread) and when a node first enters the tree. The ubershader is a full-featured version of each shader compiled at load; it draws a new object at once while the specialized, faster pipeline compiles in the background. 2D canvas pipelines are not precompiled.
- Positions are 32-bit floats in standard builds. The docs give these safe distances from the origin: 4096 m first person, 8192 m third person, 16384 m top-down, 32768 m for any 3D game. Beyond that you need double-precision builds or origin shifting. For scale: the engine blog measured about 6 cm steps at 1,000 km and 1 m steps at 10,000 km.

## Advanced techniques

### Threaded loading with status polling

`ResourceLoader.load_threaded_request(path, type_hint, use_sub_threads, cache_mode)` returns at once. Poll `load_threaded_get_status`; fetch with `load_threaded_get` only when the status is `THREAD_LOAD_LOADED`, because an early get blocks like `load`. `use_sub_threads = true` loads dependencies in parallel: faster, but it competes with the game for worker threads. Use it behind a loading screen, not while the player moves.

```gdscript
extends Node3D

const CHUNK_SIZE: float = 64.0
var _pending: Dictionary[String, Vector2i] = {}

func request_chunk(path: String, cell: Vector2i) -> void:
	if not _pending.has(path) and ResourceLoader.load_threaded_request(path) == OK:
		_pending[path] = cell

func _process(_delta: float) -> void:
	for path: String in _pending.keys():
		var status: int = ResourceLoader.load_threaded_get_status(path)
		if status == ResourceLoader.THREAD_LOAD_IN_PROGRESS:
			continue
		var cell: Vector2i = _pending[path]
		_pending.erase(path)
		if status == ResourceLoader.THREAD_LOAD_LOADED:
			var scene: PackedScene = ResourceLoader.load_threaded_get(path) as PackedScene
			var chunk: Node3D = scene.instantiate() as Node3D
			chunk.position = Vector3(cell.x * CHUNK_SIZE, 0.0, cell.y * CHUNK_SIZE)
			add_child(chunk)
			return
```

The `return` admits one chunk per frame. Cache modes: `CACHE_MODE_REUSE` is the default; `CACHE_MODE_IGNORE` gives a private copy; `CACHE_MODE_REPLACE` refreshes the cached instance in place (hot reload). For a progress bar, pass an `Array` as the second argument of `load_threaded_get_status`; element 0 receives 0..1.

### Spread instancing over frames

A big chunk still stalls in `instantiate()` and `add_child`. Options, cheapest first:

- Cut the chunk into sub-scenes (terrain, props, actors) and add one per frame.
- Mark heavy optional children "Load As Placeholder". They become `InstancePlaceholder` nodes; call `create_instance()` later, a few per frame. A placeholder has no transform.
- Run `instantiate()` in one `WorkerThreadPool` task and hand the result to the main thread with `add_child.call_deferred(chunk)`. The docs allow building nodes outside the tree in one thread; several threads touching the same resources is unsafe.

### Chunk lifecycle with hysteresis

Load when the player is within radius R, unload beyond R plus a margin, or the border thrashes. Keep a few recently used `PackedScene` references so turning around is a cache hit. To truly release memory, drop every reference: the scene variable, `preload` constants in still-loaded scripts, arrays in autoloads. `ResourceLoader.has_cached(path)` tells you if something still holds it.

### Pre-warm pipelines on level entry

Watch the Pipeline Compilations monitors (`Performance.PIPELINE_COMPILATIONS_SURFACE`, `..._DRAW`, `..._MESH`, `..._CANVAS`). Rises outside loading screens are hitches on a player's first run, even if your own driver cache hides them. Fixes from the docs:

- Show one tiny scene that uses every rendering feature the game uses (reflection probes, SDFGI or VoxelGI, lightmaps, TAA or FSR2, the MSAA level, omni shadow mode) before bulk loading. Features are tracked from first use onward.
- For effects spawned by script (explosions, bullets), keep one hidden instance in the level so its pipelines compile at load.
- Load meshes with the threaded loader so the Mesh step compiles off the main thread.
- Since 4.5 more pipeline settings are detected at load, and the shader baker export option precompiles shaders for the target driver. The release page reports about 20x shorter load for the TPS demo on Metal and D3D12. It attacks startup time; ubershaders attack mid-game hitches. Use both.

### Distance culling: visibility ranges and HLOD

Every `GeometryInstance3D` has `visibility_range_begin`, `visibility_range_end`, margins and `visibility_range_fade_mode`. Distances are measured to the AABB center. Uses: hide small props beyond a set distance; swap a mesh for an impostor; HLOD, where one merged far mesh replaces a group. For HLOD set `visibility_parent` on the detailed nodes to the merged mesh and give the merged mesh a begin distance: one number controls the whole swap. The fade modes force transparency during the fade; the default hysteresis mode is the cheapest. This combines with automatic mesh LOD and occlusion culling. Occluders are baked data: bake them per chunk scene so they stream in with it. Occlusion pays most indoors.

### MultiMesh foliage per cell

A `MultiMesh` is culled as a whole. One world-sized MultiMesh draws everything always. Build one `MultiMeshInstance3D` per grid cell, give it a `visibility_range_end`, and thin distant cells with `MultiMesh.visible_instance_count`. Fill buffers off the main thread and assign `MultiMesh.buffer` once.

### Endless 2D with TileMapLayer chunks

Use one `TileMapLayer` per chunk (for example 32x32 cells) instead of one giant layer. Freeing a chunk is one `queue_free`. Cell changes are batched to the end of the frame; painting thousands of cells in one frame hitches, so generate cell data in a thread and apply it in slices over several frames. `rendering_quadrant_size` (default 16) groups draw batches; `physics_quadrant_size` (since 4.5, default 16) merges colliders. `tile_map_data` exposes a whole layer as a `PackedByteArray`, a compact chunk save format. Turn off `collision_enabled` and `navigation_enabled` on decorative layers.

### GridMap octants

A `GridMap` merges cells into octants of `cell_octant_size` cells per axis (default 8) for rendering and physics. Larger octants mean fewer draw calls but coarser culling. For streamed 3D tile worlds use one GridMap per chunk. Since 4.7 the octants can be queried (`get_used_octants`, `get_octants_in_bounds`, `get_used_cells_in_octant`), which gives a ready spatial index for "what is in this area" without scanning every cell.

### Precision: origin shifting or double builds

Origin shifting keeps single precision: when the player passes a limit, subtract the offset from every world root. It costs game-logic complexity (saved positions, multiplayer, trails and world-space particles jump).

```gdscript
extends Node3D

const LIMIT: float = 2048.0
@export var player: Node3D

func _physics_process(_delta: float) -> void:
	var offset: Vector3 = player.global_position
	offset.y = 0.0
	if offset.length() < LIMIT:
		return
	for child: Node in get_children():
		if child is Node3D:
			(child as Node3D).global_position -= offset
			child.reset_physics_interpolation()
```

Large world coordinates need an engine and templates compiled with `precision=double` (feature tag `double`). Why rendering still works: GPUs get no doubles (Metal has none, and some integrated GPUs crash on them). The engine splits each translation into two floats and applies only the model-to-camera translation at high precision; rotation, scale and everything in camera space stay single precision. The blog calls the cost small. It fixes object positions only: a planet-sized mesh still has imprecise vertices, and world-space math in your own shaders stays single precision.

What breaks or does not gain: GDExtensions must be rebuilt; triplanar materials and `GPUParticles3D` without local coords still jitter; shaders using `world_vertex_coords` or `skip_vertex_transform` gain nothing; server and clients must use the same build type; 2D rendering does not gain. There is a CPU and memory cost; the docs advise against it for an on-foot area under 8192 x 8192 m.

### Far areas: stop paying for them

Set a chunk root's `process_mode` to `PROCESS_MODE_DISABLED`. Physics bodies below follow `CollisionObject3D.disable_mode`: `DISABLE_MODE_REMOVE` (default) takes them out of the simulation, `DISABLE_MODE_MAKE_STATIC` keeps them as obstacles. `VisibleOnScreenEnabler3D` does the same switch from camera visibility, which suits cosmetics but not gameplay that must run off-screen. For rules that must continue far away, swap the nodes for a coarse data model (see `determinism-and-simulation`).

### Navigation in chunks

Bake one navigation mesh per chunk with `NavigationMesh.filter_baking_aabb` larger than the chunk and `border_size` trimming it back, so neighbor edges line up and regions connect. Parse source geometry on the main thread, then bake on a thread with `NavigationServer3D.bake_from_source_geometry_data_async`. Parsing is the slow part: feed collision shapes or raw arrays, not visual meshes. See `navigation-advanced`.

### Save world deltas, not the world

Give streamed objects stable IDs (chunk coordinate plus index). Save only what differs from the authored chunk, keyed by chunk, and apply it right after instancing.

## Niche but useful

- `Performance.RENDER_TEXTURE_MEM_USED`, `RENDER_VIDEO_MEM_USED`, `MEMORY_STATIC` and `OBJECT_RESOURCE_COUNT` as an in-game memory overlay while flying through the world.
- ObjectDB Profiler (since 4.6): diff two snapshots to find what a freed chunk left behind.
- `SceneTree.change_scene_to_node` (since 4.6) switches to a scene you instanced and set up ahead, so the swap frame does no loading.
- LightmapGI shadowmasks (since 4.4): baked shadows far away, dynamic shadows near. A short dynamic shadow range is one of the larger savings in a big outdoor scene.
- `Light3D.shadow_caster_mask` (since 4.4) keeps small props out of shadow passes.
- Physics interpolation covers `MultiMesh` (since 4.4), so crowds can tick slowly.
- Patch PCKs with delta encoding (since 4.6) ship only the changed parts of resources: cheaper content updates for large worlds.
- Automatic mesh LODs keep the shape of multi-part meshes better since 4.6; reimport old assets to get them.
- `ResourceLoader.get_dependencies(path)` lists what a chunk pulls in.
- Terrain addons (clipmap or voxel terrain GDExtensions) solve terrain LOD, not object streaming.

## Traps

- Calling `load_threaded_get` right after the request is a blocking load in disguise.
- `preload` in a script that never unloads pins the resource for the whole session.
- `remove_child` without `free` keeps everything in memory (`Node.print_orphan_nodes`).
- `VisibleOnScreenNotifier3D` ignores occlusion unless occlusion culling is on, and does nothing when the node is hidden.

## Version notes

- 4.4: ubershaders and pipeline precompilation; navigation map sync on a background thread; MultiMesh physics interpolation; shadowmasks; typed dictionaries.
- 4.5: shader baker; more pipeline detection at load; `TileMapLayer` physics chunking; navigation regions update on threads; a real 2D navigation server.
- 4.6: ObjectDB snapshots; delta patch PCKs; better LOD generation; reflection probes use octahedral maps (less memory); Jolt is the 3D default for new projects.
- 4.7: `GridMap` octant queries; fixes for `load_threaded_get` deadlocks and a `load_threaded_request` race (4.7 beta notes), so prefer 4.7.x for heavy threaded loading.

## Related skills

- `godot-optimization` and `godot-perf-measure` - general profiling and fixes.
- `godot-multithreading` - thread and WorkerThreadPool basics.
- `godot-procedural-generation` - generating chunk content.
- `godot-save-load` - save files. `godot-ai-navigation` - navigation setup.
- Sibling shelves: `threads-and-concurrency`, `gpu-3d-optimization`, `navigation-advanced`, `io-and-saves`, `2d-rendering`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/io/background_loading.html
- https://docs.godotengine.org/en/stable/tutorials/physics/large_world_coordinates.html
- https://docs.godotengine.org/en/stable/tutorials/3d/visibility_ranges.html
- https://docs.godotengine.org/en/stable/tutorials/3d/occlusion_culling.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/tutorials/performance/thread_safe_apis.html
- https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html
- https://docs.godotengine.org/en/stable/tutorials/navigation/navigation_using_navigationmeshes.html
- https://godotengine.org/article/emulating-double-precision-gpu-render-large-worlds/
- https://godotengine.org/article/whats-missing-in-godot-for-aaa/
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-3/
- https://godotengine.org/article/dev-snapshot-godot-4-7-beta-2/
- https://godotengine.org/article/dev-snapshot-godot-4-7-beta-4/
- https://godotengine.org/priorities/
- Third-party open-world and terrain blogs were not reachable; none is cited.
