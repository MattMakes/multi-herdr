# Math and procedural content

Target engine: Godot 4.7. Number precision, smoothing and rotation math that holds at any frame
rate, seeded randomness, noise, curves as data, polygon helpers, and building meshes in code.

## Use this shelf when

- Smoothing, camera follow or turning feels different at 30, 60 and 144 FPS.
- Generated content must be the same for the same seed, on any machine, in any order.
- You build geometry at run time: terrain chunks, trails, debug shapes, level pieces.
- You need polygon work: clip, grow, shrink, triangulate, convex split.
- Designers should tune a feel or a balance curve without touching code.

## How the engine works here

- `float` in GDScript is 64-bit. The components of `Vector2`, `Vector3`, `Basis`, `Color` and
  `PackedFloat32Array` are 32-bit (unless the engine is built with `precision=double`). A value
  loses precision the moment it enters a vector. `int` is 64-bit and wraps on overflow.
- A 32-bit float holds every integer up to 16,777,216; a 64-bit float up to about 9e15. Keep
  money, score and tick counts in `int`.
- `Basis` is three axis vectors. Each `rotated()` call adds rounding error, so a basis that
  turns every frame drifts into shear and scale. `orthonormalized()` repairs it.
- The global functions (`randi()`, `randf()`, `Array.shuffle()`, `Array.pick_random()`) share
  one generator, seeded from the clock at start. `RandomNumberGenerator` instances are separate
  streams (PCG32) with a readable `state`.
- `Curve`, `Curve2D`, `Curve3D` and `Gradient` are resources. `Curve` caches `bake_resolution`
  samples for `sample_baked()`. Path curves cache points every `bake_interval` units; the first
  baked call builds the cache.
- A mesh is a list of surfaces. A surface is an array of `Mesh.ARRAY_MAX` typed arrays
  (vertices, normals, UVs, indices, ...). Front faces wind clockwise. All built-in procedural
  mesh tools run on the CPU.

## Advanced techniques

### Smooth with exponential decay

`lerp(a, b, k * delta)` depends on the frame rate: the weight is a share of the remaining
distance, not an amount. Use `1.0 - exp(-rate * delta)`. A half-life is easier to tune:
the gap halves every `half_life` seconds at any frame rate.

```gdscript
extends Node3D

@export var target: Node3D
@export var half_life: float = 0.15

func _process(delta: float) -> void:
	if target == null:
		return
	var weight: float = 1.0 - exp(-0.6931 * delta / half_life)
	global_position = global_position.lerp(target.global_position, weight)
	var from := global_basis.orthonormalized().get_rotation_quaternion()
	var to := target.global_basis.orthonormalized().get_rotation_quaternion()
	global_basis = Basis(from.slerp(to, weight))
```

For a constant speed use `move_toward()` and `rotate_toward()`. For angles use `lerp_angle()`
and `angle_difference()`; plain `lerp()` takes the long way across the wrap point.

### Interpolate rotations safely

`Basis.slerp()` and `Quaternion.slerp()` need pure rotations: call `orthonormalized()` or
`get_rotation_quaternion()` first when the node has scale. `Transform3D.interpolate_with()` is
correct only for uniform scale or equal non-uniform scale on both ends. `slerpni()` does not
pick the short path; use it when a spin of more than 180 degrees is wanted.
`spherical_cubic_interpolate()` gives smooth rotation through keyframes.

### One generator per system, seeded by hash

Give loot, level layout, AI and visual effects each their own `RandomNumberGenerator`, so a
change in one does not shift the others. Derive seeds; do not use neighbours like 1, 2, 3:
the class reference warns that similar seeds can give similar streams.

```gdscript
extends RefCounted

func chunk_rng(world_seed: int, chunk: Vector2i) -> RandomNumberGenerator:
	var rng := RandomNumberGenerator.new()
	rng.seed = hash([world_seed, chunk.x, chunk.y])
	return rng

func shuffle_with(rng: RandomNumberGenerator, items: Array) -> void:
	for i: int in range(items.size() - 1, 0, -1):
		var j: int = rng.randi_range(0, i)
		var tmp: Variant = items[i]
		items[i] = items[j]
		items[j] = tmp
```

A per-chunk seed makes a chunk the same no matter the order chunks are built. Save `rng.state`
(not the seed) to resume a stream in a save game or a rewind; never write made-up state values.
`shuffle()` and `pick_random()` ignore your instance, hence the manual shuffle.

### Fair randomness

`rng.rand_weighted(weights)` returns an index by relative weight (0 means never). A shuffle bag
(copy the list, shuffle, pop until empty, refill) bounds streaks and droughts; players read it
as fair. `randfn(mean, deviation)` gives a normal spread for recoil or stat rolls.

### Noise at scale

`FastNoiseLite.frequency` is 0.01 by default: one feature per about 100 units. Values are near
-1..1. Stack meaning with separate instances (height, moisture, heat) and with
`domain_warp_enabled` for less grid-like shapes. Calling `get_noise_2d()` per cell from
GDScript is the slow part of terrain code: fetch a block with `get_image()` and read the bytes,
or generate on a worker thread. Sample world coordinates (`offset` or chunk origin added), not
local ones, so chunk borders match. `NoiseTexture2D` with `seamless` gives tiling textures;
`seamless_blend_skirt` trades contrast for a hidden seam. It generates on a thread:
`await texture.changed` before `get_image()`. `NoiseTexture3D` feeds fog and volumetric shaders.

### Curves and gradients as designer data

Export a `Curve` for damage falloff, XP per level, spawn rate over time, or stick response.
`sample_baked(x)` is a table read; `sample(x)` evaluates exactly. Since 4.4 `min_domain` and
`max_domain` let x run beyond 0..1, so a curve can use real units (level 1 to 50, metres,
seconds) and needs no remap in code. Also since 4.4 points outside `min_value`..`max_value` are
clamped, so widen the range first. `Gradient.sample()` does the same for colour (day cycle, heat, rarity) and
`interpolation_color_space = GRADIENT_COLOR_SPACE_OKLAB` avoids grey midpoints. `CurveTexture`
and `GradientTexture1D` pass the same data to shaders and particles.

### Move along a path at constant speed

Bezier `t` is not distance. Use `sample_baked(distance, true)` with `get_baked_length()`.
`sample_baked_with_rotation()` returns a transform that follows the path. `get_closest_offset()`
maps a world point back to a distance (rail grind, lane position, progress along a track).
`tessellate()` gives few points on straights and many on bends, for drawing;
`tessellate_even_length()` gives near-uniform spacing, for posts or dashes.

### Polygon tools you do not have to write

| Need | Call (`Geometry2D`) |
| --- | --- |
| Cut B out of A (destructible ground) | `clip_polygons()`; a clockwise result is a hole |
| Union, overlap | `merge_polygons()`, `intersect_polygons()` |
| Grow or shrink an outline | `offset_polygon()`; returns several polygons, or none |
| Fill a polygon with triangles | `triangulate_polygon()`; empty array on failure |
| Mesh from loose points | `triangulate_delaunay()` |
| Concave to convex pieces for physics | `decompose_polygon_in_convex()` |
| Grid cells on a line (line of sight) | `bresenham_line()` |
| Pack rectangles | `make_atlas()` |

`Geometry3D` adds `tetrahedralize_delaunay()`, `clip_polygon()` against a plane,
`segment_intersects_convex()` and `get_triangle_barycentric_coords()`. All run in native code.

### Build meshes with the right tool

| Tool | Use for | Cost |
| --- | --- | --- |
| `ArrayMesh.add_surface_from_arrays()` | static generated meshes; fastest | you fill every array |
| `SurfaceTool` | same, simpler; `generate_normals()`, `generate_tangents()`, `index()` | slower |
| `ImmediateMesh` | geometry that changes every frame, debug lines | rebuilt on each change |
| `MeshDataTool` | edits that need edge and face data | slow; triangles only |

```gdscript
extends MeshInstance3D

func _ready() -> void:
	var arrays: Array = []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = PackedVector3Array([
		Vector3(0, 0, 0), Vector3(0, 0, 1), Vector3(1, 0, 0), Vector3(1, 0, 1)])
	arrays[Mesh.ARRAY_NORMAL] = PackedVector3Array([
		Vector3.UP, Vector3.UP, Vector3.UP, Vector3.UP])
	arrays[Mesh.ARRAY_INDEX] = PackedInt32Array([0, 2, 1, 1, 2, 3])
	var array_mesh := ArrayMesh.new()
	array_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	mesh = array_mesh
```

Index shared vertices; split them only where normals or UVs must differ. Tangents need normals
and UVs first. Build the arrays on a worker thread and assign the mesh on the main thread (see
`threads-and-concurrency`). `create_trimesh_shape()` makes static collision from the result.
Save a generated mesh with `ResourceSaver.save()` to skip generation on the next run.

### GridMap and CSG

`GridMap` batches cells per octant (`cell_octant_size`, 8). Each cell stores an item and one of
24 orientations. `MeshLibrary` items carry collision and navigation meshes;
`bake_navigation` makes one region per cell, so match the map cell size. Good for modular
levels built by code with `set_cell_item()`. Since 4.7 `get_used_octants()`,
`get_used_cells_in_octant()` and `get_octants_in_bounds()` let code work chunk by chunk (stream,
save or simulate one octant) without a scan of `get_used_cells()`. 4.7 also adds a
`MeshLibrary` editor, so the library no longer has to be edited in the Inspector.
CSG nodes are for blockout. Since 4.4 they use the Manifold library: the old code was
unstable and had no maintainer. A `CSGMesh3D` mesh must now be manifold (closed, each edge
shared by two faces, with volume); planes and quads go in a `MeshInstance3D`. Keep trees small
and separate. Convert the final shape with `bake_static_mesh()` (since 4.4) and
`bake_collision_shape()`; CSG updates one frame late, so wait a frame before baking. Since 4.7
`autosmooth` on the root with `smoothing_angle` (50 degrees) fixes faceted curved blockouts.

## Niche but useful

- `remap()`, `inverse_lerp()`, `smoothstep()`, `pingpong()`, `wrapf()`, `snappedf()`: replace
  most hand-written range math.
- `cubic_interpolate_in_time()`: smooth interpolation between samples with uneven time gaps.
- `bezier_derivative()`: the tangent of a Bezier, for facing along a custom curve.
- `Vector3.octahedron_encode()`: a unit vector in two floats, for compact data.
- `ArrayMesh.shadow_mesh`: a positions-only copy that speeds up shadow and depth passes.
- `ArrayMesh.custom_aabb`: stops wrong culling when a shader moves vertices.
- `SurfaceTool.append_from()`: merge several meshes with transforms into one surface.
- `AStarGrid2D`: grid pathfinding for generated maps; see `navigation-advanced`.
- `Basis.is_orthonormal()` (since 4.7): assert a pure rotation before `slerp()` in debug builds.
- `Curve3D.closed` (since 4.4): a looping path with no duplicate end point.
- `Geometry2D.bresenham_line()` is exposed since 4.4.

## Traps

- `is_equal_approx()` for float compares; `==` on computed floats fails at random.
- `randi() % n` works; `randi_range(a, b)` includes both ends.
- Treat `hash()` values as valid inside one engine version (a caution; the docs promise no
  stability). Do not store them in saves; an engine upgrade may change hashed chunk seeds.
- `ARRAY_TANGENT` needs four floats per vertex; a wrong array size fails the whole surface.
- `MeshDataTool` and `generate_normals()` expect `PRIMITIVE_TRIANGLES`.
- A moving object far from the origin jitters: see large worlds in `physics-advanced`.

## Version notes

- 4.4: `Curve` domains beyond 0..1 and an enforced value range. `Curve3D.closed`. CSG moved to
  Manifold (non-manifold input rejected), `bake_static_mesh()`. `Geometry2D.bresenham_line()`.
- 4.5: nothing for these areas in the release page or the upgrade guide.
- 4.6: the GridMap editor paints gap-free lines (Bresenham). No API change.
- 4.7: GridMap octant queries, `MeshLibrary` editor, CSG `autosmooth`, `Basis.is_orthonormal()`.

## Related skills

- `godot-math-essentials`: vector, transform and interpolation basics with recipes.
- `godot-procedural-generation`: dungeon, cave and wave function collapse algorithms.
- `godot-tween-animation`: easing with `Tween` when a fixed duration is wanted.
- `godot-multithreading`: worker threads for chunk generation.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/math/interpolation.html
- https://docs.godotengine.org/en/stable/tutorials/math/random_number_generation.html
- https://docs.godotengine.org/en/stable/tutorials/math/beziers_and_curves.html
- https://docs.godotengine.org/en/stable/tutorials/math/matrices_and_transforms.html
- https://docs.godotengine.org/en/stable/tutorials/3d/procedural_geometry/index.html
- https://docs.godotengine.org/en/stable/tutorials/3d/procedural_geometry/arraymesh.html
- https://docs.godotengine.org/en/stable/tutorials/3d/using_gridmaps.html
- https://docs.godotengine.org/en/stable/tutorials/3d/csg_tools.html
- https://docs.godotengine.org/en/stable/tutorials/physics/large_world_coordinates.html
- https://docs.godotengine.org/en/stable/classes/class_geometry2d.html
- https://docs.godotengine.org/en/stable/classes/class_randomnumbergenerator.html
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-4/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-6/
- https://godotengine.org/article/dev-snapshot-godot-4-4-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-7-dev-3/
