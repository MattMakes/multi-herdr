# GPU and 3D optimization

Target engine: Godot 4.7. How to cut 3D frame cost: culling, LOD, instancing, overdraw, shadows,
textures, effects and the limits of each renderer.

## Use this shelf when

- A 3D scene misses its frame budget and you must find which lever to pull.
- You build a large level: many props, foliage, lights, decals or long view distances.
- You plan a mobile or low-end target and must choose which effects to drop.
- You see pop-in of lights or decals, Z-fighting, transparency sorting errors or banding.

## How the engine works here

Find the bottleneck class first. Fill rate test from the docs: V-Sync off, compare FPS in a large
and a tiny window. Big gain = pixel (fragment) bound: lower resolution scale, simplify materials,
cut overdraw and shadows. No gain = CPU or draw-call bound: cull, merge, instance.

Per frame the engine frustum-culls instances (on several threads above
`rendering/limits/spatial_indexer/threaded_cull_minimum_instances`, default 1000), then
occlusion-culls, picks a mesh LOD, sorts opaque objects by material and shader to cut state
changes, and draws. Transparent objects are drawn after, back to front by node origin, and cannot
be sorted by material.

Depth prepass: Forward+ and Compatibility draw depth first, so each opaque pixel is shaded once.
Mobile has no prepass, so overdraw there costs full shading and occlusion culling pays more.

On desktop, vertices are cheap and pixels are dear. Mobile GPUs are tile-based: dense geometry
in a small screen area stalls a tile. Avoid triangles smaller than a pixel everywhere.

Automatic instancing exists only in Forward+: `MeshInstance3D` nodes with the same mesh and
material are drawn with GPU instancing, each still culled alone. It works for opaque and
alpha-tested (scissor, hash) materials; alpha-blended and depth-pre-pass materials are never
instanced. Mobile and Compatibility need `MultiMesh` for the same gain. There is no automatic
static mesh merging in 3D.

Limits that cause pop-in: Forward+ has 512 clustered elements in view (omni + spot + area lights
+ decals + reflection probes; `rendering/limits/cluster_builder/max_clustered_elements`). Mobile
has 8 omni + 8 spot + 8 decals + 8 reflection probes per mesh *resource* and 256 lights in view,
fixed. Compatibility has 8 + 8 lights per mesh (`rendering/limits/opengl/max_lights_per_object`).
All renderers: 8 directional lights, 8 LightmapGI or VoxelGI instances.

## Advanced techniques

### Occlusion culling with baked occluders

Enable `rendering/occlusion_culling/use_occlusion_culling`, add an `OccluderInstance3D`, bake.
The engine rasterizes occluders on the CPU (Embree) into a small buffer and hides any
`GeometryInstance3D` whose AABB is *fully* covered. Small objects behind big walls cull best. It
has no frame delay, so camera cuts are safe. Pays most indoors and on Mobile; with few occluders
it only costs CPU. Rules:

- Only opaque `MeshInstance3D` surfaces are baked. MultiMesh, particles and CSG need manual
  `BoxOccluder3D`, `QuadOccluder3D` or `PolygonOccluder3D` shapes.
- Keep dynamic objects out of the bake with `bake_mask` and visual layers.
- Never move a baked occluder (or its parent) in play: the BVH is rebuilt. For a door, keep the
  occluder static and toggle its `visible` when the door opens.
- If the CPU cost shows, raise `bake_simplification_distance`, then lower
  `rendering/occlusion_culling/occlusion_rays_per_thread` or `bvh_build_quality`.
- A first-person weapon should set `ignore_occlusion_culling`. Keep `extra_cull_margin` at 0.

### Automatic mesh LOD and its global dial

Imported scenes (glTF, .blend, FBX; OBJ only when imported as Scene) get meshoptimizer LODs and a
welded *shadow mesh* for free. Selection is by screen coverage, so FOV and resolution are already
counted. `rendering/mesh_lod/lod_change/threshold_pixels` (default 1.0, tuned to be invisible) is
a strong quality dial: `Viewport.mesh_lod_threshold` at 2 to 4 on low presets cuts primitives
with little visible change. Per object use `lod_bias`. Shadows and reflection probes pick their
own LOD; raise `ReflectionProbe.mesh_lod_threshold` on probes that update always. A MultiMesh or
particle system uses one LOD for all instances, taken from the nearest AABB point. Mesh LOD
lowers vertex cost only, not pixel cost.

### Visibility ranges as HLOD

`visibility_range_begin` / `visibility_range_end` on any `GeometryInstance3D` (meshes, MultiMesh,
particles, `Label3D`, `Sprite3D`). The pay-off is hierarchical: near, many small nodes that cull
well; far, one merged low mesh or impostor, so draw calls fall. Set `visibility_parent` on the
detailed nodes to the merged node; children hide while the parent is in its range. Distance is to
the AABB center and ignores FOV and resolution. Fade mode `VISIBILITY_RANGE_FADE_DISABLED` plus
margins gives free hysteresis. Self and Dependencies fades force transparent rendering during the
blend. For a cheaper cross-fade, use zero margins, overlap the ranges, and set the material's
`distance_fade_mode` to `DISTANCE_FADE_OBJECT_DITHER`: it stays opaque.

### MultiMesh for thousands of copies

One draw call for all instances, in every renderer. The price: no per-instance culling (all or
nothing by the whole AABB) and one LOD. So chunk by area, one `MultiMeshInstance3D` per chunk.
Allocate `instance_count` once and move `visible_instance_count` instead of resizing. Set
`custom_aabb` to skip the costly AABB recompute when instances move. Animate in the vertex shader
with `INSTANCE_ID` or `INSTANCE_CUSTOM`. For bulk updates write the whole `buffer` once
(`RenderingServer.multimesh_set_buffer()`), built on worker threads if needed. `godot-optimization`
has the setup code.

### Cut material and shader variants

Opaque draw order is sorted by material, then shader. `StandardMaterial3D` instances that enable
the same feature set share one shader even with different parameters; each new feature
combination is a new shader and new pipelines. Reuse materials, vary looks with instance uniforms
(`set_instance_shader_parameter()`) or vertex colors, and atlas textures. A stripped material
(no normal map, rim, clearcoat, height) on far HLOD meshes can pay, but measure: every extra
unique material has its own cost.

### Transparency: avoid alpha blend where a cutout works

Alpha-blended surfaces skip the depth prepass, are sorted per object, are never auto-instanced
and shade every layer. In order of preference: opaque; alpha scissor; alpha hash (casts shadows,
noisy, good with TAA); depth pre-pass; alpha blend. Split a mesh so only the truly transparent
part uses a transparent material. For distance fading use the dither modes, not pixel alpha.
Sorting errors: `sorting_offset` on the node, `render_priority` on the material.

### Shadow cost tuning

- Directional: 4 PSSM splits draw a large object up to 5 times per frame. Use 2 splits or
  orthogonal for small or top-down scenes. Always set `directional_shadow_max_distance` to what
  the camera needs: better texel density and fewer casters.
- Positional lights share an atlas (`rendering/lights_and_shadows/positional_shadow/atlas_size`,
  4 quadrants, default subdivisions hold 4 + 4 + 16 + 64 = 88 shadows). A shadow is redrawn only
  when the light or an object in its range changes, so static lights over static geometry are
  nearly free and a moving prop under 20 lights is not.
- Spot shadows render 1 face, omni cube shadows 6; `omni_shadow_mode` dual paraboloid renders 2.
- `shadow_caster_mask` (since 4.4) limits which layers cast for a light; `distance_fade_shadow`
  drops the shadow before the light itself fades.
- Keep 16-bit shadow maps and atlas 4096 (2048 on low-end). PCSS (`light_size` or
  `light_angular_distance` above 0) is dear, most of all on directional lights.
- Fully baked lights (`BAKE_STATIC`) skip real-time light and shadow on lightmapped meshes.

### Decals and lights: fade and shorten

Cost follows screen coverage more than count. Enable `distance_fade_enabled` on `Decal`,
`OmniLight3D` and `SpotLight3D`: past begin + length the element is culled and leaves the cluster
budget. Keep the decal box short along its projection axis. Decals take no custom shader and do
not exist in Compatibility.

### Texture memory and sampling

VRAM Compressed (S3TC/BPTC on desktop, ETC2/ASTC on mobile;
`rendering/textures/vram_compression/import_etc2_astc` ships both) cuts memory and bandwidth.
Lossless and Lossy shrink only the file; in VRAM they are uncompressed. Keep pixel art
uncompressed. Stay at or under 4096 px for mobile; the import **Size Limit** caps sources. A
negative `texture_mipmap_bias` sharpens but costs sampling time; stay within -0.5 to 0.

### Volumetric fog budget (Forward+)

Fog is a froxel grid: `rendering/environment/volumetric_fog/volume_size` and `volume_depth` set
its cost, not screen resolution. Shorten `volumetric_fog_length` to gain depth detail for free.
Temporal reprojection hides noise but ghosts behind fast lights and `FogVolume` nodes: give
short-lived lights `light_volumetric_fog_energy` 0, which also removes them from the fog pass.
Use small 3D density textures (64 cubed or less).

### Vertex shading for the low end

`shading_mode = SHADING_MODE_PER_VERTEX` on a material, or
`rendering/shading/overrides/force_vertex_shading` for the whole project, moves lighting to
vertices. Large gain on weak mobile GPUs and shaded particles, coarse on low-poly meshes.

## Niche but useful

- `Viewport.debug_draw` overdraw mode and the editor's **Occlusion Culling Buffer** and **Disable
  Mesh LOD** views show what each system really does.
- `rendering/driver/depth_prepass/enable` off: wins in fixed top-down views with little overdraw.
- `VisibleOnScreenEnabler3D`: stop animation and logic for off-screen actors; skinning is costly.
- Raise `Camera3D.near` before lowering `far` to fix Z-fighting; near matters far more.
- Large world coordinates need an engine build with `precision=double` and cost speed and
  memory. The docs recommend single precision up to about 8 km from origin for third-person and
  65 km for any 3D game.

## Traps

- "Fewer polygons is always faster": pixels, draw calls and overdraw usually dominate on desktop.
- Merging a whole level into one mesh kills culling: merge by area.
- Billboards and impostors add overdraw when stacked; they trade vertices for fill rate.
- Lights hidden by `light_cull_mask` still cast shadows from masked objects; set `cast_shadow`.
- Resolution scale that changes nothing means the bottleneck is CPU or draw calls.

## Version notes

- 4.4: `Light3D.shadow_caster_mask`; CSG can be baked to a mesh for occluder and lightmap bakes.
- 4.5: stencil modes on `BaseMaterial3D` (outline, x-ray) without an extra viewport pass.
- 4.6: simplified SSAO in Compatibility; SSR rewritten; volumetric fog blending changed.
- 4.7: `AreaLight3D`; in Forward+ one visible area light adds cost to all objects in the scene.

## Related skills

- `godot-optimization`: profiler, draw-call basics, pooling, MultiMesh setup steps.
- `godot-perf-measure`: A/B timing, cost maps and budget tests to prove a change.
- `godot-3d-essentials`: step-by-step LOD, occlusion, decal and shadow setup.
- `godot-mobile-development`: mobile budgets and adaptive resolution.
- Sibling shelves: `rendering-pipeline`, `lighting-and-gi`, `particles-and-vfx`,
  `worlds-and-streaming`, `shaders-advanced`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/performance/gpu_optimization.html
- https://docs.godotengine.org/en/stable/tutorials/performance/optimizing_3d_performance.html
- https://docs.godotengine.org/en/stable/tutorials/3d/occlusion_culling.html
- https://docs.godotengine.org/en/stable/tutorials/3d/mesh_lod.html
- https://docs.godotengine.org/en/stable/tutorials/3d/visibility_ranges.html
- https://docs.godotengine.org/en/stable/tutorials/performance/using_multimesh.html
- https://docs.godotengine.org/en/stable/tutorials/3d/lights_and_shadows.html
- https://docs.godotengine.org/en/stable/tutorials/3d/volumetric_fog.html
- https://docs.godotengine.org/en/stable/tutorials/3d/3d_rendering_limitations.html
- https://docs.godotengine.org/en/stable/tutorials/physics/large_world_coordinates.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_images.html
