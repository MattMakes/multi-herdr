Adds 3D shader techniques that the recipes do not cover: alpha scissor and alpha hash cutouts (foliage, dissolve), world position from the depth buffer, a full-screen spatial post-process quad under reversed-Z, triplanar mapping, fog volume shaders and pipeline warm-up; read it for 3D cutouts that must cast shadows, depth-based effects, or first-use shader stutter.

# Cutouts, Depth and World-Space Techniques

> ← Back to [SKILL.md](../SKILL.md). 2D dissolve and outline are in [2d-shader-recipes.md](2d-shader-recipes.md); screen-space overlays and Compositor effects are in [post-processing.md](post-processing.md) and [compositor-effects.md](compositor-effects.md).

All shader code is Godot 4.7 shading language for the Forward+ and Mobile
renderers unless a section says otherwise.

---

## 1. Cutouts: Scissor or Hash, Not Blended Alpha

A spatial shader that writes `ALPHA` with blending is drawn in the
transparent pass. Transparent objects do not write depth, do not cast normal
shadows, and sort per object, so dense foliage flickers and has no shadows.

For leaves, fences, hair cards and dissolves, keep the object opaque and cut
pixels instead:

| Mode | How | Look |
|---|---|---|
| Alpha scissor | Write `ALPHA` and `ALPHA_SCISSOR_THRESHOLD` | Hard edge; cheapest |
| Alpha hash | Write `ALPHA` and `ALPHA_HASH_SCALE` | Dithered soft edge; best with TAA or FSR |

Both keep depth writes and shadow casting. On a `StandardMaterial3D` the same
choice is `transparency = TRANSPARENCY_ALPHA_SCISSOR` or
`TRANSPARENCY_ALPHA_HASH`.

A plain `discard` also cuts pixels, but an unconditional `discard` path in a
shader disables some early depth optimizations for the whole material. Prefer
the scissor threshold, which Godot handles in the depth prepass and shadow
pass for you.

```glsl
shader_type spatial;
render_mode cull_disabled;

uniform sampler2D leaf_tex : source_color, filter_linear_mipmap;
uniform float cutoff : hint_range(0.0, 1.0) = 0.5;
uniform float wind_strength = 0.15;
uniform float wind_speed = 1.2;

void vertex() {
	// Sway grows toward the top of the plant (UV.y = 0 at the top).
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	float sway = sin(TIME * wind_speed + world.x * 0.7 + world.z * 0.4);
	VERTEX.x += sway * wind_strength * (1.0 - UV.y);
}

void fragment() {
	vec4 tex = texture(leaf_tex, UV);
	ALBEDO = tex.rgb;
	ALPHA = tex.a;
	ALPHA_SCISSOR_THRESHOLD = cutoff;
}
```

A 3D dissolve uses the same mechanism with a noise texture as the alpha
source:

```glsl
shader_type spatial;

uniform sampler2D albedo_tex : source_color;
uniform sampler2D noise_tex;
uniform float dissolve : hint_range(0.0, 1.0) = 0.0;
uniform vec4 edge_color : source_color = vec4(1.0, 0.5, 0.1, 1.0);
uniform float edge_width = 0.05;

void fragment() {
	float n = texture(noise_tex, UV).r;
	ALBEDO = texture(albedo_tex, UV).rgb;
	// Pixels whose noise is below `dissolve` fall under the threshold.
	ALPHA = step(dissolve, n);
	ALPHA_SCISSOR_THRESHOLD = 0.5;
	float edge = 1.0 - smoothstep(dissolve, dissolve + edge_width, n);
	EMISSION = edge_color.rgb * edge * step(0.001, dissolve);
}
```

Drive `dissolve` per object with an `instance uniform` (see
[per-instance-and-global-uniforms.md](per-instance-and-global-uniforms.md)).

## 2. World Position from the Depth Buffer

Water edges, soft intersections, decals and scanner rings need the world
position of whatever is behind the current pixel. Read the depth texture and
undo the projection.

```glsl
shader_type spatial;
render_mode unshaded;

uniform sampler2D depth_tex : hint_depth_texture, filter_nearest;
uniform vec3 ring_center = vec3(0.0);
uniform float ring_radius = 5.0;
uniform float ring_width = 0.4;

void fragment() {
	float depth = texture(depth_tex, SCREEN_UV).r;
	// Forward+ and Mobile: depth is already in NDC (reversed-Z since 4.3).
	// Compatibility renderer: use `depth * 2.0 - 1.0` instead.
	vec3 ndc = vec3(SCREEN_UV * 2.0 - 1.0, depth);
	vec4 view = INV_PROJECTION_MATRIX * vec4(ndc, 1.0);
	view.xyz /= view.w;
	vec3 world = (INV_VIEW_MATRIX * vec4(view.xyz, 1.0)).xyz;

	float d = abs(distance(world, ring_center) - ring_radius);
	float ring = 1.0 - smoothstep(0.0, ring_width, d);
	ALBEDO = vec3(0.2, 0.9, 1.0);
	ALPHA = ring;
}
```

The depth texture holds only opaque geometry. A material that reads it must
itself be transparent (it writes `ALPHA`), or it would read its own depth.

## 3. A Full-Screen Spatial Quad (Reversed-Z)

For a 3D post-process that needs the depth or normal-roughness buffer, put a
`QuadMesh` of size 2×2 in front of the camera and output clip space directly
from `vertex()`. Since Godot 4.3 the depth buffer is reversed (1.0 is the near
plane), so the quad uses z = 1.0:

```glsl
shader_type spatial;
render_mode unshaded, depth_draw_never, depth_test_disabled, cull_disabled;

uniform sampler2D screen_tex : hint_screen_texture, filter_linear_mipmap;
uniform sampler2D depth_tex : hint_depth_texture, filter_nearest;
uniform float edge_strength = 4.0;

void vertex() {
	POSITION = vec4(VERTEX.xy, 1.0, 1.0);
}

void fragment() {
	vec3 color = texture(screen_tex, SCREEN_UV).rgb;
	vec2 px = 1.0 / VIEWPORT_SIZE;
	float d = texture(depth_tex, SCREEN_UV).r;
	float dx = texture(depth_tex, SCREEN_UV + vec2(px.x, 0.0)).r - d;
	float dy = texture(depth_tex, SCREEN_UV + vec2(0.0, px.y)).r - d;
	float edge = clamp((abs(dx) + abs(dy)) * edge_strength * 100.0, 0.0, 1.0);
	ALBEDO = mix(color, vec3(0.0), edge);
}
```

Set a large `extra_cull_margin` on the `MeshInstance3D` (for example 16384),
or the quad is culled when its real position leaves the view. Older samples
that use `POSITION = vec4(VERTEX, 1.0)` clip on 4.3 and later. For effects
that do not need scene buffers, a `ColorRect` overlay or a
`CompositorEffect` is simpler.

## 4. Triplanar Mapping Without UVs

Rocks, cliffs and generated terrain often have stretched or missing UVs.
Project the texture along the three world axes and blend by the normal.
`StandardMaterial3D` does this with `uv1_triplanar = true`; in a custom shader:

```glsl
shader_type spatial;

uniform sampler2D rock_tex : source_color, filter_linear_mipmap, repeat_enable;
uniform float tiling = 0.25;
uniform float blend_sharpness = 4.0;

varying vec3 world_pos;
varying vec3 world_normal;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
}

void fragment() {
	vec3 w = pow(abs(world_normal), vec3(blend_sharpness));
	w /= (w.x + w.y + w.z);
	vec3 x_proj = texture(rock_tex, world_pos.zy * tiling).rgb;
	vec3 y_proj = texture(rock_tex, world_pos.xz * tiling).rgb;
	vec3 z_proj = texture(rock_tex, world_pos.xy * tiling).rgb;
	ALBEDO = x_proj * w.x + y_proj * w.y + z_proj * w.z;
}
```

Triplanar costs three texture reads per texture. Use it where UVs fail, not
on every material.

## 5. Fog Volume Shaders (Forward+ Only)

A `FogVolume` node with a `ShaderMaterial` whose shader is `shader_type fog`
shapes volumetric fog. It needs **Volumetric Fog** enabled in the
`Environment`. The `fog()` function writes `DENSITY`, and can write `ALBEDO`
and `EMISSION`. `SDF` is the signed distance to the volume's surface
(negative inside), and `WORLD_POSITION` and `UVW` locate the sample.

```glsl
shader_type fog;

uniform float density : hint_range(0.0, 4.0) = 1.0;
uniform vec3 tint : source_color = vec3(0.4, 0.6, 0.9);
uniform float edge_fade = 0.5;

void fog() {
	float inside = clamp(-SDF / max(edge_fade, 0.0001), 0.0, 1.0);
	DENSITY = density * inside;
	ALBEDO = tint;
}
```

## 6. First-Use Stutter and Warm-Up

A pipeline (shader plus render state) compiles the first time it is drawn.
On Forward+ and Mobile, Godot 4.4+ draws with a slower general shader while
the specialized one compiles in the background, which hides most stutter.
The Compatibility renderer has no such fallback. Two remedies:

- Export with the **shader baker** (see [shader-baker.md](shader-baker.md)).
- Draw each heavy material once behind the loading screen: instance the
  effects in view of the camera, wait for the frame to render, then free
  them.

```gdscript
extends Node3D

## Shows each scene once in front of the camera, then frees it.
func warm_up(scenes: Array[PackedScene], camera: Camera3D) -> void:
	var holder := Node3D.new()
	add_child(holder)
	holder.global_transform = camera.global_transform.translated_local(Vector3(0.0, 0.0, -3.0))
	for scene in scenes:
		holder.add_child(scene.instantiate())
	await RenderingServer.frame_post_draw
	await RenderingServer.frame_post_draw
	holder.queue_free()
```

Waiting two frames lets the draw reach the GPU. Keep the loading screen in
front, for example on a `CanvasLayer`, so the player does not see the burst.
