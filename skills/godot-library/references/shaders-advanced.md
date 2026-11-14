# Shaders: language depth, screen buffers, compute, cost

Target engine: Godot 4.7. The parts of the shading language and shader pipeline that change
cost or unlock effects: uniforms scopes, screen textures, compile stutter, compute, stencil.

## Use this shelf when

- Many objects need per-object values and you must not create one material per object.
- An effect reads the screen, depth or normal-roughness buffer, in 2D or 3D.
- The game hitches the first time an effect shows up.
- A shader must run on Mobile or Compatibility as well as Forward+.
- You port GLSL or Shadertoy code, or move heavy math to a compute shader.
- You need outlines, X-ray or portal masks (stencil, since 4.5).

## How the engine works here

- A `.gdshader` is not what the GPU runs. Godot wraps your `vertex()`, `fragment()`, `light()`
  code into its own large template, then shader compilation (to SPIR-V on Vulkan) and pipeline
  compilation (driver, per mesh format and render state) follow. One shader means many pipelines.
- Since 4.4 (Forward+ and Mobile) an "ubershader" pipeline is compiled when a mesh or node is
  loaded, and specialized variants are compiled in the background. Stutter remains where the
  engine cannot see the need ahead of time. Compatibility has none of this, and 2D (canvas)
  pipelines are never precompiled on any renderer.
- What your shader touches decides its pass. Reading or writing `ALPHA` moves a spatial material
  to the transparent pipeline: no depth prepass benefit, sorted, drawn after opaque. `discard`
  also defeats the depth prepass for that surface.
- Screen textures are copies. 3D: color is copied once after the opaque pass, so transparent
  objects are missing from it, and a material that samples it is itself treated as transparent.
  2D: copied when the first item needs it (see `2d-rendering`).
- Depth is reverse-Z: 1.0 near, 0.0 far, non-linear. Forward+ and Mobile use NDC z in 0..1;
  Compatibility uses -1..1.
- Uniform scopes: `uniform` (per material), `instance uniform` (per node, same material and
  batch), `global uniform` (project-wide, one buffer shared by all shader types).
- Particle shaders (`start()`, `process()`) keep their output between frames. Sky shaders run
  for the background and again for the radiance cubemap. Fog shaders run per froxel.

## Advanced techniques

### Per-instance uniforms instead of material copies

Declare `instance uniform vec4 tint : source_color = vec4(1.0);` and set it per node with
`GeometryInstance3D.set_instance_shader_parameter()` or
`CanvasItem.set_instance_shader_parameter()`. One material stays shared, so sorting and batching
are kept. Limits: no textures, no arrays, about 16 per shader. To pick a texture per instance,
pass a `sampler2DArray` as a normal uniform and an instance `int` layer. With several materials
on one mesh, pin slots with `instance_index(n)` to stop name clashes.

### Global uniforms for world state

Add them in Project Settings > Shader Globals, declare `global uniform vec3 wind;` in any shader
type, and write with `RenderingServer.global_shader_parameter_set()`. Writes are cheap at any
rate. Reads are not: `global_shader_parameter_get()` syncs with the render thread, so keep your
own copy. Use it for wind, player position for foliage bend, time of day, and an unscaled clock
(`TIME` follows `Engine.time_scale`, ignores pause, and wraps at 3600 s by default).

```gdscript
extends Node

func _process(_delta: float) -> void:
	var seconds: float = float(Time.get_ticks_msec()) / 1000.0
	RenderingServer.global_shader_parameter_set(&"unscaled_time", seconds)
```

### One file, three renderers

Since 4.4 the preprocessor defines `CURRENT_RENDERER`, `RENDERER_COMPATIBILITY`,
`RENDERER_MOBILE` and `RENDERER_FORWARD_PLUS`. Use them for the depth NDC difference and to drop
costly paths on weak targets.

```gdshader
shader_type spatial;
render_mode unshaded, fog_disabled;
uniform sampler2D depth_tex : hint_depth_texture, repeat_disable, filter_nearest;

void vertex() {
	POSITION = vec4(VERTEX.xy, 1.0, 1.0);
}

void fragment() {
	float depth = textureLod(depth_tex, SCREEN_UV, 0.0).r;
#if CURRENT_RENDERER == RENDERER_COMPATIBILITY
	vec3 ndc = vec3(SCREEN_UV, depth) * 2.0 - 1.0;
#else
	vec3 ndc = vec3(SCREEN_UV * 2.0 - 1.0, depth);
#endif
	vec4 view = INV_PROJECTION_MATRIX * vec4(ndc, 1.0);
	ALBEDO = vec3(fract(-view.z / view.w));
}
```

This is the full-screen quad pattern: a `QuadMesh` of size 2 with `flip_faces` on, a huge
`extra_cull_margin` or parented to the camera so culling keeps it. It can read depth, which a
`SubViewport` post-process cannot.

### Shader libraries and compile-time switches

`#include "res://fx/noise.gdshaderinc"` pulls in a `ShaderInclude` (only `.gdshaderinc`, max
depth 25, no cycles). `#if`/`#ifdef` remove code before compilation; a runtime `if` on a
uniform keeps both sides compiled and their variables alive. GPUs handle a branch well when all
pixels take the same side, so prefer `#if` for quality tiers and a runtime `if` for rare toggles.
`#pragma disable_preprocessor` skips the preprocessor step for shaders that do not need it.

### Screen, depth and normal-roughness reads

Each hint (`hint_screen_texture`, `hint_depth_texture`, `hint_normal_roughness_texture`) may be
declared on several uniforms with different filter and repeat flags. `textureLod()` with a level
above 0 only blurs if the uniform uses a `*_mipmap` filter. Normal-roughness is Forward+ only
(normal in xyz mapped to 0..1, roughness in w) and switches on an extra pipeline feature, so
show one such material during loading. Depth from another viewport is not readable.

### Kill first-use stutter

Watch the pipeline compilation monitors (Canvas, Mesh, Surface, Draw, Specialization). Rises
outside loading screens are hitches on a player's first run. Fixes that work on 4.4+:
keep a hidden instance of each script-spawned effect in the scene so its pipelines exist; show
one scene early that uses each renderer feature you will turn on later (a reflection probe,
TAA or motion vectors, SDFGI, VoxelGI, lightmaps, normal-roughness readers); never change MSAA
level during play. On Compatibility, and for all 2D shaders, the old way still applies: draw
each material for one frame behind the loading screen. The export option Shader Baker (since
4.5) removes the shader compile step at startup, mostly a win on Direct3D 12 and Metal; it does
not remove pipeline stutter.

### Cheap render modes

`unshaded` skips lighting. `shadows_disabled`, `ambient_light_disabled`, `specular_disabled` and
`diffuse_lambert` trim the lit path. `vertex_lighting` moves lighting to vertices: use it on
shaded particles and distant props. `fog_disabled` suits additive effects. `depth_prepass_alpha`
gives sorted-looking hair and foliage at the price of a second pass. Project-wide switches for
weak devices: `rendering/shading/overrides/force_vertex_shading` and
`rendering/shading/overrides/force_lambert_over_burley`.

### Fragment cost rules

Texture reads dominate, more so with several textures and mipmap filtering, and they cost
battery on phones. Pack masks into channels of one texture. Use `const` over `uniform` for fixed
values (slightly faster). Move work to `vertex()` and pass it in a `varying` (`flat` to stop
interpolation). `lowp`/`mediump` can help in `fragment()` on mobile but behave differently per
driver; leave precision alone unless measured. Big uniform arrays hit the uniform buffer cap
(65536 bytes on most desktops, 16384 on most mobile; vec2 and vec3 pad to vec4): put tables in a
texture.

### Compute shaders

Written in raw GLSL 450 with a `#[compute]` header, imported as `RDShaderFile`. Forward+ and
Mobile only. Two devices: `RenderingServer.create_local_rendering_device()` runs apart from the
frame (you call `submit()` then `sync()`; cannot share textures with the scene; no GPU debugger
support), and `RenderingServer.get_rendering_device()` is the main device (output can feed a
`Texture2DRD`; touch it from `RenderingServer.call_on_render_thread()`). `sync()` right after
`submit()` stalls the CPU: wait 2-3 frames, or use `buffer_get_data_async()`. Long dispatches
can trip the Windows driver watchdog (about 5-10 s): split the work. Free every RID with
`free_rid()`.

### Compositor effects

A `CompositorEffect` gets `_render_callback()` on the render thread at a chosen stage
(`EFFECT_CALLBACK_TYPE_POST_OPAQUE`, `..._POST_TRANSPARENT`, etc.) and works on the real render
buffers with RenderingDevice calls. Choose it over a full-screen quad when you need motion
vectors (`needs_motion_vectors`), a stage before transparents, or a compute pass. Guard shared
state with a `Mutex`. The main stencil buffer cannot be copied out for these effects.

### Stencil (since 4.5, experimental)

`BaseMaterial3D.stencil_mode` offers Outline and X-Ray presets (they add a `next_pass`
material) and Custom. Text shaders use the stencil modes listed in the spatial reference (`read`,
`write`, `write_if_depth_fail`, `compare_*`); convert a preset material to a ShaderMaterial to
get the exact statement. Stencil writers draw in the transparent pass, and reads only work
there.

### Sky shader budget

Anything using `TIME` or `POSITION` forces a radiance cubemap update every frame. Branch on
`AT_CUBEMAP_PASS` to feed a cheap color to the cubemap, or render the heavy sky only into the
cubemap and show `texture(RADIANCE, EYEDIR)` for a static sky. `use_half_res_pass` and
`use_quarter_res_pass` run clouds at a fraction of the pixels.

## Niche but useful

- `hint_enum("Low:0", "High:2")` gives an int uniform a named dropdown in the inspector.
- `group_uniforms Name;` folds inspector parameters; `group_uniforms;` ends the group.
- `LIGHT_VERTEX` (spatial and canvas) moves the lighting sample point without moving the pixel.
- `Z_CLIP_SCALE` pulls a first-person weapon toward the camera so it never clips into walls.
- `IN_SHADOW_PASS` lets a shader cast a different shape than it draws.
- `VisualShaderNodeCustom` with `@tool` and `class_name` adds your own node to the visual editor.
- `DrawableTexture2D.blit_rect()` plus a texture blit shader (`blit()`) paints textures on the GPU.

## Traps

- Setting a uniform with the wrong GDScript type gives no error, only wrong pixels.
- Color textures and color uniforms need `source_color`, or they look wrong on Forward+/Mobile.
- Writing `DEPTH` on one branch means you must write it on all branches.
- Porting GLSL: rename `main`, replace Shadertoy `fragCoord`/`iResolution` with `FRAGCOORD` and
  `1.0 / SCREEN_PIXEL_SIZE`; Godot expects GLSL ES 3.0 built-ins.
- Outdated: `SCREEN_TEXTURE` and `DEPTH_TEXTURE` built-ins (3.x) do not exist; use hints.

## Version notes

- 4.4: ubershaders and pipeline precompilation; renderer preprocessor defines.
- 4.5: stencil modes; Shader Baker export option.
- 4.7: visual shader `LinearToSRGB` no longer clamps to 0..1 on Forward+ and Mobile.

## Related skills

- `godot-shader-basics`: recipes, visual shaders, basic post-processing, SMAA.
- `godot-perf-measure`: shader warm-up lists with a guard test.
- Sibling shelves: `2d-rendering`, `particles-and-vfx`, `rendering-pipeline`, `gpu-3d-optimization`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/shading_language.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/shader_preprocessor.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/spatial_shader.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/screen-reading_shaders.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/advanced_postprocessing.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/compute_shaders.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/converting_glsl_to_godot_shaders.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/compositor.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/tutorials/3d/standard_material_3d.html
