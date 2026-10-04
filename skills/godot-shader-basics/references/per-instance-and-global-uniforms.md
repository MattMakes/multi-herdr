Adds ways to vary a shader per object or per world without new materials: instance uniforms, global uniforms, texture arrays, and vertex-to-fragment varyings; read it when many objects share one shader but need different values, or when a world value (wind, player position) feeds many materials.

# Per-Instance and Global Shader Data

> ← Back to [SKILL.md](../SKILL.md). Basic uniforms and `set_shader_parameter()` are in SKILL.md section 2.

All shader code is Godot 4.7 shading language. GDScript is 4.7.

---

## 1. Why Not One Material per Object

`set_shader_parameter()` changes the **material**. To give 200 enemies 200
flash values that way, each enemy needs its own material copy. Each copy is
its own render state, so the objects can no longer be drawn together.
Godot has two better tools:

| Need | Tool |
|---|---|
| A different value on each object, one material | `instance uniform` |
| One value read by every material in the scene | `global uniform` |

## 2. Instance Uniforms

Declare the uniform with the `instance` keyword. Set it per node with
`set_instance_shader_parameter()` on a `GeometryInstance3D`
(`MeshInstance3D`, `MultiMeshInstance3D` and similar) or, for 2D, on a
`CanvasItem`. The material resource stays shared.

```glsl
shader_type spatial;

uniform sampler2D albedo_tex : source_color;
instance uniform float hit_flash : hint_range(0.0, 1.0) = 0.0;
instance uniform vec4 team_tint : source_color = vec4(1.0);

void fragment() {
	vec3 base = texture(albedo_tex, UV).rgb * team_tint.rgb;
	ALBEDO = mix(base, vec3(1.0), hit_flash);
}
```

```gdscript
extends MeshInstance3D

## Flashes this enemy white without touching the shared material.
func flash(duration: float = 0.15) -> void:
	set_instance_shader_parameter(&"hit_flash", 1.0)
	var t := create_tween()
	t.tween_method(_set_flash, 1.0, 0.0, duration)


func _set_flash(value: float) -> void:
	set_instance_shader_parameter(&"hit_flash", value)


func set_team_color(color: Color) -> void:
	set_instance_shader_parameter(&"team_tint", color)
```

Limits:

- An instance uniform cannot be a texture (`sampler2D` and other samplers).
  To pick a texture per object, see section 4.
- A shader can have at most 16 instance uniforms.
- In the inspector the values appear under **Instance Shader Parameters** on
  the node, not on the material.

## 3. Global Uniforms

A `global uniform` is one value that every shader that declares it can read.
Use it for world state: wind direction, the player's position for grass that
bends away, a world-wide wetness level.

1. Declare it in **Project Settings → Shader Globals** with a name and type.
   (Code can add one at runtime with
   `RenderingServer.global_shader_parameter_add()`.)
2. Declare it in each shader: `global uniform vec3 player_position;`.
3. Set it from code once per frame or on change with
   `RenderingServer.global_shader_parameter_set()`.

```glsl
shader_type spatial;
render_mode cull_disabled;

global uniform vec3 player_position;
uniform float push_radius = 1.5;
uniform float push_strength = 0.6;

void vertex() {
	vec3 world = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	vec3 away = world - player_position;
	float dist = length(away.xz);
	float push = (1.0 - smoothstep(0.0, push_radius, dist)) * push_strength;
	// Bend only the upper part of the blade (UV.y = 0 at the tip).
	float tip = 1.0 - UV.y;
	vec2 dir = away.xz / max(dist, 0.001);
	world.xz += dir * push * tip;
	VERTEX = (inverse(MODEL_MATRIX) * vec4(world, 1.0)).xyz;
}
```

```gdscript
extends CharacterBody3D

## Publishes the player position to every grass material.
func _physics_process(_delta: float) -> void:
	RenderingServer.global_shader_parameter_set(&"player_position", global_position)
```

A shader that declares a global the project does not define fails to
compile. Keep the list in Project Settings, so that a teammate's checkout has
the same globals.

## 4. A Different Texture per Object: Texture Arrays

Combine an instance uniform index with a `sampler2DArray`. All layers must
have the same size and format. Build the array once with
`Texture2DArray.create_from_images()`, or import a sliced image as a
`Texture2DArray` in the Import dock.

```glsl
shader_type spatial;

uniform sampler2DArray skins : source_color, filter_linear_mipmap;
instance uniform int skin_index = 0;

void fragment() {
	ALBEDO = texture(skins, vec3(UV, float(skin_index))).rgb;
}
```

```gdscript
extends Node

## Builds a Texture2DArray from same-sized images and assigns it to the material.
func build_skin_array(images: Array[Image], material: ShaderMaterial) -> void:
	var array := Texture2DArray.new()
	var err := array.create_from_images(images)
	if err != OK:
		push_error("Skins: create_from_images failed (%s). All images need one size and format." % error_string(err))
		return
	material.set_shader_parameter(&"skins", array)
```

Each object then sets `skin_index` with `set_instance_shader_parameter()`.

## 5. Move Work from `fragment()` to `vertex()`

`fragment()` runs once per covered pixel; `vertex()` runs once per vertex. A
value that changes smoothly across a triangle (world position, a fog factor,
a tint gradient) can be computed in `vertex()` and passed with a `varying`.
The GPU interpolates it for each pixel.

```glsl
shader_type spatial;

uniform vec3 fog_color : source_color = vec3(0.6, 0.7, 0.8);
uniform float fog_height = 4.0;

varying float height_fog;

void vertex() {
	float world_y = (MODEL_MATRIX * vec4(VERTEX, 1.0)).y;
	height_fog = clamp(1.0 - world_y / fog_height, 0.0, 1.0);
}

void fragment() {
	ALBEDO = mix(vec3(0.3, 0.5, 0.3), fog_color, height_fog);
}
```

Do not move a value that must be exact per pixel (a sharp texture lookup, a
noise pattern). Interpolation smears it.

## 6. Shader Writing Rules

| Rule | Reason |
|---|---|
| Put `source_color` on color uniforms and color textures | Without it the value is treated as linear data, and colors look washed out. |
| Multiply `TIME` by a `speed` uniform | Designers can tune speed, and you can stop the effect with 0. |
| Compare floats with a tolerance or `step()` | `==` on floats fails on small precision differences between GPUs. |
| `normalize()` a vector before `reflect()`, `dot()` lighting math | An interpolated normal is no longer unit length. |
| Wrap or clamp shifted UVs (`fract()`, `clamp()`, or a `repeat_enable` hint) | Out-of-range UVs read the edge pixel or a wrong texel. |
| Prefer `mix()`, `step()`, `smoothstep()` to `if` on per-pixel data | Neighbor pixels that take different branches run both paths. A branch on a uniform is cheap, because all pixels take the same path. |
| Use a uniform, not a `#define`, for a runtime switch | Each `#define` variant is a separate shader that compiles on first use and can stutter. |
