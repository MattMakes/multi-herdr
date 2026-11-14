# Servers and low-level APIs

Target engine: Godot 4.7. This shelf covers drawing, physics and instancing without nodes: server
objects by RID, MultiMesh buffers, and the patterns for very large entity counts.

## Use this shelf when

- Thousands of similar things (bullets, units, pickups, grass, debris) must update every frame.
- Node count or per-node callbacks are the measured bottleneck and tiers did not fix it.
- You need many instances drawn in one draw call, each with its own data for the shader.
- You want collision or overlap tests for objects that have no node.
- You must run code right before or right after the frame is drawn.

## How the engine works here

The scene system is a layer on top of the servers and can be bypassed. `RenderingServer`,
`PhysicsServer2D`, `PhysicsServer3D`, `NavigationServer2D`, `NavigationServer3D` and
`AudioServer` hold the real objects. Every node that draws or collides owns server objects and
forwards its properties to them.

A server object is named by an `RID`, an opaque handle. You create it, you free it with the same
server's `free_rid()`. Nothing frees it for you, also not at scene change.

A `Resource` (texture, mesh, shape) can be passed where an RID is wanted, or you call `get_rid()`.
The server does not count that use as a reference. Keep the resource in a variable for as long
as the server object uses it, or both vanish.

Where to attach things: `get_canvas_item()` gives a node's canvas item, `World2D.canvas` and
`World2D.space` give the 2D canvas and physics space, `World3D.scenario` and `World3D.space`
the 3D ones. The docs advise against driving the RIDs that a node already owns; create new ones.

Servers may run on other threads. A call that returns a value can force the server to finish all
pending work first. The docs say to avoid reading from `RenderingServer` and the physics servers
per frame. Keep your own copy of state; treat the servers as write-mostly.

Scale guide from the docs: nodes start to hurt at thousands to tens of thousands. For thousands
of instances that change all the time, use servers directly. For hundreds of thousands to
millions, only MultiMesh is left, and GDScript loops stop being fast enough for per-frame
updates ("a few thousand" is called fine).

## Advanced techniques

### Canvas items by RID

A canvas item takes draw commands once; they cannot be edited, only cleared with
`canvas_item_clear()` and added again. The transform can be set as often as you like, so record
the sprite once and then only move it.

```gdscript
extends Node2D

@export var texture: Texture2D
var _item: RID

func _ready() -> void:
	_item = RenderingServer.canvas_item_create()
	RenderingServer.canvas_item_set_parent(_item, get_canvas_item())
	var size: Vector2 = texture.get_size()
	var rect: Rect2 = Rect2(-size / 2.0, size)
	RenderingServer.canvas_item_add_texture_rect(_item, rect, texture.get_rid())
	RenderingServer.canvas_item_set_transform(_item, Transform2D(0.0, Vector2(64.0, 64.0)))
	RenderingServer.canvas_item_reset_physics_interpolation(_item)

func _exit_tree() -> void:
	RenderingServer.free_rid(_item)
```

`canvas_item_reset_physics_interpolation()` on the first frame stops the item from sliding in
from the origin. `canvas_item_set_custom_rect()` gives a fixed culling rect and saves the CPU
work of computing one from the commands.

### 3D instances by RID

`RenderingServer.instance_create2(mesh_rid, scenario)` makes a visual instance in one call;
`instance_set_transform()` moves it. Each instance is still culled and listed by itself, so this
removes node cost, not renderer cost. The renderer helps on its side: since 4.0 culling is a
cache-friendly brute-force pass on several threads, and objects with the same mesh and material
are drawn as instances. Call `instance_teleport()` (since 4.5) after a jump to clear motion
vectors. When per-object renderer cost is the limit, use MultiMesh.

### Interpolation differs between 2D and 3D

2D physics interpolation lives in the server, so canvas items made by RID are interpolated
(`canvas_item_set_interpolated()`). 3D is different. In 4.4 it was also done in the
RenderingServer (4.4 had `instance_set_interpolated()`, which 4.5 removed). In 4.5 it moved to
the SceneTree, because nodes need the interpolated transform and could not ask the server for
it: the server sits behind a threaded command queue and a read would stall it. So since 4.5 a
3D instance made by RID is not interpolated. Blend the previous and current transform yourself
with `Engine.get_physics_interpolation_fraction()`. Nodes can read
`Node3D.get_global_transform_interpolated()` (since 4.4). MultiMesh is covered in both.

### Physics bodies without nodes

`body_create()`, a shape from `circle_shape_create()` and `shape_set_data()`, `body_add_shape()`,
`body_set_space()`, `body_set_state()` with `BODY_STATE_TRANSFORM`. To learn where the body went,
do not poll: `body_set_state_sync_callback(body, callable)` calls you with the direct body state
for each body that was active in the last tick. `body_set_force_integration_callback(body,
callable, userdata)` also passes your `userdata` (an index into your arrays) and lets you set
velocities. Free shapes and bodies with `PhysicsServer2D.free_rid()`. The 3D API mirrors this.

### Space queries instead of Area nodes

For "what is inside this circle", ask the space: `World2D.direct_space_state` gives a
`PhysicsDirectSpaceState2D`; `intersect_shape()` and `intersect_point()` take a `max_results`
that bounds the cost. The result gives `rid` and `collider_id`, so it works for bodies with no
node. With multi-threaded physics the space state may only be used inside `_physics_process`.

### Bullet-hell and crowd shape

Hold position, velocity and life in packed arrays in one manager (see `cpu-and-frame-budget`).
Draw with one canvas item per entity or with one MultiMesh for all. For hits, test against the
few targets by distance, or give each bullet a server body when it must hit world geometry. The
docs point to the official Bullet Shower demo. They give no speedup factor: measure.

### Update a mesh in place

When geometry changes every frame but its vertex count does not, do not clear the mesh and add
the surface again. `RenderingServer.mesh_surface_update_vertex_region()`, with
`mesh_surface_update_attribute_region()` and `mesh_surface_update_skin_region()`, overwrite the
bytes of an existing surface. The engine team measured the other way on animated `Polygon2D`
nodes: 3.3 ms per frame to create vertex arrays and 5 ms to free them, in a frame that needed
3.4 ms to draw. Moving `Polygon2D` to in-place updates in 4.7 took that frame from 35 ms to
13 ms. Cost: you build the byte layout of the surface format yourself.

### MultiMesh: set the format, then the count

`transform_format`, `use_colors` and `use_custom_data` can only change while `instance_count` is
0. Set them, then the count, then the data. Allocate the largest count you will need once and
change `visible_instance_count` per frame (-1 draws all); it does not resize buffers. Keep live
instances at the front: swap the last live one into a freed slot.

### MultiMesh has one bounding box

All instances are culled as one object: all drawn or none. Split a large world into one
MultiMesh per area. Set `custom_aabb` to skip the AABB recalculation after each update. A
MultiMesh also counts as one object for the per-object light limit, and blend shapes are ignored.

### Upload the whole buffer in one call

Per-instance setters cross into the engine once per instance. `MultiMesh.buffer` (or
`RenderingServer.multimesh_set_buffer()`) takes all data as one `PackedFloat32Array`. Floats per
instance: 12 for a 3D transform (8 for 2D), plus 4 for color, plus 4 for custom data. The
transform is stored row by row: basis x.x, y.x, z.x, origin.x, then the y row, then the z row.

```gdscript
extends MultiMeshInstance3D

func upload(transforms: Array[Transform3D]) -> void:
	var buf: PackedFloat32Array = PackedFloat32Array()
	buf.resize(transforms.size() * 12)
	for i: int in transforms.size():
		var b: Basis = transforms[i].basis
		var p: Vector3 = transforms[i].origin
		var o: int = i * 12
		buf[o] = b.x.x; buf[o + 1] = b.y.x; buf[o + 2] = b.z.x; buf[o + 3] = p.x
		buf[o + 4] = b.x.y; buf[o + 5] = b.y.y; buf[o + 6] = b.z.y; buf[o + 7] = p.y
		buf[o + 8] = b.x.z; buf[o + 9] = b.y.z; buf[o + 10] = b.z.z; buf[o + 11] = p.z
	multimesh.buffer = buf
```

The array can be filled on worker threads and set once. A wrong size prints an error and draws
nothing. The docs point to a GDExtension for this loop when counts reach millions.

### Per-instance data for the shader

`set_instance_custom_data(i, Color(...))` gives four floats per instance, read in the shader as
`INSTANCE_CUSTOM`. Use them for an animation phase, a speed factor, a hit flash or an atlas frame.

```gdshader
shader_type spatial;
uniform float time_scale = 1.0;

void vertex() {
	float t = TIME * time_scale * (0.5 + INSTANCE_CUSTOM.y) + 6.28318 * INSTANCE_CUSTOM.x;
	VERTEX.x += cos(t + VERTEX.z) * 0.1;
}
```

### Vertex animation in place of skeletons

Bones are solved on the CPU per character. A swim, sway or walk cycle written as sine waves in
`vertex()` costs no CPU and works with instancing. Shape it with a mask along the body
(`smoothstep`) so only the tail moves. Limit: no blending between clips.

### Let the GPU move the instances

Updating thousands of transforms from script each frame is the slow part of a moving MultiMesh.
A `GPUParticles3D` with a mesh draw pass and a `particles` shader keeps `TRANSFORM`, `COLOR`
and `CUSTOM` on the GPU between frames, and the mesh shader reads them. Use it for schools,
swarms and debris that no game rule needs to read back. Cost: the CPU does not know positions.

### MultiMesh with physics interpolation

Since 4.4 for 3D transforms and since 4.6 for 2D transforms. If instance order is stable, set
`buffer` each tick and the renderer blends. If order changes between ticks, call
`set_buffer_interpolated(current, previous)`. After a first placement or a teleport call
`reset_instance_physics_interpolation(i)` for one instance or, since 4.6,
`reset_instances_physics_interpolation()` for all. `physics_interpolation_quality` has a
high setting for tick rates below about 20 or fast spins.

### Hook the frame

`RenderingServer.frame_pre_draw` fires before viewports are updated, `frame_post_draw` after.
`await RenderingServer.frame_post_draw` before you read a viewport texture for a screenshot or a
thumbnail. `call_on_render_thread()` runs a callable where `RenderingDevice` use is safe.

## Niche but useful

- `RenderingServer.canvas_item_add_multimesh()` draws a 2D MultiMesh with no node at all.
- Since 4.4: `multimesh_allocate_data(..., use_indirect)` with
  `multimesh_get_command_buffer_rd_rid()` lets a compute shader set the instance count on the
  GPU, and `multimesh_get_buffer_rd_rid()` exposes the instance buffer to `RenderingDevice` code.
- `NavigationServer3D.query_path()` and `map_get_path()` return paths with no agent node.
- `body_attach_object_instance_id()` makes a raw body report an object of your choice in queries.
- `AudioServer` is used directly for buses, volumes and effects; its script API has no call that
  plays a stream, so sounds still go through player nodes.

## Traps

- Leaked RIDs. Free every RID you created, in `_exit_tree` or `NOTIFICATION_PREDELETE`.
- Dropping the last reference to a texture, mesh or shape that a server object still uses.
- The docs page for servers still shows `body_set_force_integration_callback(body, self,
  "_body_moved", 0)`. In 4.7 the second argument is a `Callable`.
- Reading server state per frame (`body_get_state`, `multimesh_get_buffer`): it can stall.
- Changing MultiMesh format with a non-zero count: it fails.
- Expecting per-instance culling or sorting from a MultiMesh.
- Moving to servers before measuring: nothing you make shows in the remote scene tree.

## Version notes

- 4.4: 3D physics interpolation, then inside the RenderingServer. MultiMesh interpolation
  (`set_buffer_interpolated`, `physics_interpolation_quality`). MultiMesh indirect draw and
  RenderingDevice buffer RIDs. Clearer error messages from MultiMesh functions.
- 4.5: 3D interpolation moved to the SceneTree; `RenderingServer.instance_set_interpolated()`
  and `instance_reset_physics_interpolation()` are gone. `instance_teleport()` added.
- 4.6: MultiMesh interpolation for 2D transforms; `reset_instances_physics_interpolation()`.
- 4.7: `Polygon2D` updates its mesh in place. The MultiMesh and vertex animation doc pages are
  flagged as outdated by the docs; the API names here are checked against 4.7.2.

## Related skills

- `godot-optimization` - MultiMesh and RenderingServer starter recipes and when to reach for them.
- `godot-perf-measure` - proving that a batching change paid off.
- `godot-particles-vfx` - particle systems, for the GPU-driven instance pattern.
- `godot-shader-basics` - shader syntax for the vertex animation part.
- `godot-multithreading` - filling buffers on worker threads.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/performance/using_servers.html
- https://docs.godotengine.org/en/stable/tutorials/performance/using_multimesh.html
- https://docs.godotengine.org/en/stable/tutorials/performance/vertex_animation/animating_thousands_of_fish.html
- https://docs.godotengine.org/en/stable/tutorials/performance/vertex_animation/controlling_thousands_of_fish.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/2d_and_3d_physics_interpolation.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/advanced_physics_interpolation.html
- Class reference 4.7: RenderingServer, MultiMesh, PhysicsServer2D, World2D
- https://github.com/godotengine/godot-demo-projects/tree/master/2d/bullet_shower (linked by docs)
- https://godotengine.org/article/rendering-cpu-optimizations/
- https://godotengine.org/article/godot-40-optimization-progress-report/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-4/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/
- https://godotengine.org/releases/4.4/ and https://godotengine.org/releases/4.5/
- Class reference XML of 4.3 to 4.6, used to date the APIs
