Adds chunked generation for large or endless worlds: one seed per chunk, data built on a worker thread, nodes made on the main thread, saving the seed plus player changes, and mesh terrain from a height field. Read it when a world is too big to generate in one frame, or must generate around a moving player.

> ← Back to [SKILL.md](../SKILL.md)

# Chunked, Threaded Generation

## One seed per chunk

A world seed alone is not enough for chunks: chunks generate in the order the player walks, so one shared `RandomNumberGenerator` gives a different world for a different route. Derive a **chunk seed** from the world seed and the chunk coordinate, and give each chunk its own generator.

```gdscript
# chunk_seed.gd
class_name ChunkSeed
extends RefCounted


## Same world seed + same coordinate = same chunk, in any order and on any thread.
static func for_chunk(world_seed: int, coord: Vector2i) -> int:
    return hash([world_seed, coord.x, coord.y])


static func rng_for(world_seed: int, coord: Vector2i) -> RandomNumberGenerator:
    var rng := RandomNumberGenerator.new()
    rng.seed = for_chunk(world_seed, coord)
    return rng
```

- **Noise needs no chunk seed.** One `FastNoiseLite` with the world seed, sampled at world coordinates, is seamless across chunk borders. A per-chunk noise seed gives visible seams.
- `hash()` is stable for the same values within a Godot version. If seeds are shared between players ("seed codes"), test that a seed gives the same world after each engine upgrade.
- To resume one long random sequence exactly (a run-based game), save `rng.state` as well as `rng.seed`, and set `state` after `seed` on load.

## Build data on a worker, nodes on the main thread

Generation is CPU work and must not run in the frame. The scene tree is not thread-safe, so a worker thread builds **plain data** (arrays, dictionaries, `Image`, mesh arrays) and the main thread turns the data into nodes.

```gdscript
# chunk_streamer.gd
class_name ChunkStreamer
extends Node2D

signal chunk_ready(coord: Vector2i)

@export var world_seed: int = 12345
@export var chunk_cells: int = 32
@export var load_radius: int = 2

var _noise := FastNoiseLite.new()
var _pending: Dictionary[Vector2i, int] = {}   # coord -> task id
var _loaded: Dictionary[Vector2i, Node] = {}
var _results: Dictionary[Vector2i, Dictionary] = {}
var _mutex := Mutex.new()


func _ready() -> void:
    _noise.seed = world_seed
    _noise.frequency = 0.02


func update_around(center_chunk: Vector2i) -> void:
    for y in range(-load_radius, load_radius + 1):
        for x in range(-load_radius, load_radius + 1):
            var coord := center_chunk + Vector2i(x, y)
            if not _loaded.has(coord) and not _pending.has(coord):
                _pending[coord] = WorkerThreadPool.add_task(_build.bind(coord))
    for coord in _loaded.keys():
        if absi(coord.x - center_chunk.x) > load_radius + 1 or absi(coord.y - center_chunk.y) > load_radius + 1:
            _loaded[coord].queue_free()
            _loaded.erase(coord)


## Runs on a worker thread. Touches no node and no shared state except through the mutex.
func _build(coord: Vector2i) -> void:
    var rng := ChunkSeed.rng_for(world_seed, coord)
    var heights := PackedFloat32Array()
    heights.resize(chunk_cells * chunk_cells)
    var origin := coord * chunk_cells
    for y in chunk_cells:
        for x in chunk_cells:
            heights[y * chunk_cells + x] = _noise.get_noise_2d(origin.x + x, origin.y + y)
    var props := rng.randi_range(0, 5)
    _mutex.lock()
    _results[coord] = {"heights": heights, "props": props}
    _mutex.unlock()
    _finish.call_deferred(coord)


## Runs on the main thread.
func _finish(coord: Vector2i) -> void:
    if _pending.has(coord):
        WorkerThreadPool.wait_for_task_completion(_pending[coord])
        _pending.erase(coord)
    _mutex.lock()
    var data: Dictionary = _results.get(coord, {})
    _results.erase(coord)
    _mutex.unlock()
    if data.is_empty():
        return
    var chunk := Node2D.new()
    chunk.name = "Chunk_%d_%d" % [coord.x, coord.y]
    chunk.position = Vector2(coord * chunk_cells * 16)
    chunk.set_meta(&"heights", data["heights"])
    add_child(chunk)
    _loaded[coord] = chunk
    chunk_ready.emit(coord)


func _exit_tree() -> void:
    for coord in _pending:
        WorkerThreadPool.wait_for_task_completion(_pending[coord])
```

- **Wait for every task** you start (`wait_for_task_completion()`), even when the result is no longer needed. The pool requires it, and `_exit_tree()` must wait before the node is freed.
- `FastNoiseLite.get_noise_2d()` is safe to call from several workers on the same object as long as no thread changes its properties meanwhile. Configure the noise before the first task.
- **Commit in slices.** Even on the main thread, building 50 chunks in one frame stalls. Cap the number of `_finish()` commits per frame (queue them and drain 1 to 4 per `_process()`).
- **Navigation after generation.** A generated region needs its navigation mesh baked after the geometry exists: `NavigationRegion2D.bake_navigation_polygon()` or `NavigationRegion3D.bake_navigation_mesh()`, which bake on a thread by default. Since 4.5, regions also update asynchronously, so an agent can query the old map for a few frames (verified by unit GW10). See `godot-ai-navigation`.

## Noise into an `Image`

For a map, a minimap or a shader, `Noise.get_image(width, height)` fills an `Image` in one native call, which is much faster than a script loop over `get_noise_2d()`. Sample the `Image` with `get_pixel(x, y).r` afterwards. Do not sample noise per frame in `_process()`; sample once into memory and read from there.

## Save the seed and the changes, not the world

A generated chunk can be rebuilt from its seed at any time. Save only what the player changed, keyed by chunk:

```gdscript
# chunk_deltas.gd
class_name ChunkDeltas
extends RefCounted

## chunk coord -> (cell -> new tile id, -1 = removed)
var _changes: Dictionary[Vector2i, Dictionary] = {}


func record(chunk: Vector2i, cell: Vector2i, tile: int) -> void:
    if not _changes.has(chunk):
        _changes[chunk] = {}
    _changes[chunk][cell] = tile


## Apply after a chunk is regenerated, before it is shown.
func changes_for(chunk: Vector2i) -> Dictionary:
    return _changes.get(chunk, {})


func to_dict() -> Dictionary:
    var out := {}
    for chunk in _changes:
        var cells := []
        for cell: Vector2i in _changes[chunk]:
            cells.append([cell.x, cell.y, _changes[chunk][cell]])
        out["%d,%d" % [chunk.x, chunk.y]] = cells
    return out
```

The save then holds the world seed, the generator version, and the deltas. Bump the **generator version** whenever the algorithm changes the output for an old seed, and keep the old generator, or migrate, for saves that name the old version.

## Mesh terrain from a height field (3D)

For 3D, a chunk becomes a mesh. Build the vertex arrays on the worker, and create the `ArrayMesh` and the collision on the main thread.

```gdscript
# height_mesh.gd
class_name HeightMesh
extends RefCounted


## heights: (cells + 1)^2 values, row-major. Returns arrays for ArrayMesh.add_surface_from_arrays().
static func build_arrays(heights: PackedFloat32Array, cells: int, cell_size: float,
        height_scale: float) -> Array:
    var st := SurfaceTool.new()
    st.begin(Mesh.PRIMITIVE_TRIANGLES)
    var side := cells + 1
    for z in side:
        for x in side:
            st.set_uv(Vector2(x, z) / float(cells))
            st.add_vertex(Vector3(x * cell_size, heights[z * side + x] * height_scale, z * cell_size))
    for z in cells:
        for x in cells:
            var i := z * side + x
            st.add_index(i)
            st.add_index(i + 1)
            st.add_index(i + side)
            st.add_index(i + 1)
            st.add_index(i + side + 1)
            st.add_index(i + side)
    st.generate_normals()
    return st.commit_to_arrays()


## Main thread: wrap the arrays in a mesh node with trimesh collision.
static func make_node(arrays: Array) -> MeshInstance3D:
    var mesh := ArrayMesh.new()
    mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
    var node := MeshInstance3D.new()
    node.mesh = mesh
    node.create_trimesh_collision()
    return node
```

- Share the border row of heights between neighbors: a chunk of `cells` quads needs `cells + 1` samples per side, taken at world coordinates, so edges meet exactly.
- Godot culls back faces, and a front face is **clockwise** as seen by the camera. The index order above is clockwise seen from above (+Y); on 4.7.2, `generate_normals()` gives it the normal (0, 1, 0). If the terrain is invisible from above, the winding is reversed.
- For distant chunks, build fewer cells (half resolution) instead of a full mesh, or use `SurfaceTool.generate_lod()` before `commit_to_arrays()`.
- Marching cubes (caves, overhangs) and marching squares (2D contours) are not covered here. They use the same split: arrays on a worker, `ArrayMesh` on the main thread.
