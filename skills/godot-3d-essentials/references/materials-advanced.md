Adds material techniques beyond the basics in SKILL.md: per-instance variation with instance uniforms, overlay passes for hit flashes, triplanar mapping for meshes without good UVs, vertex wind for foliage, subsurface scattering, distance fade without sorting problems, and Z-fighting fixes. Read it when many copies of one mesh need different colors, when a model has no usable UVs, or when foliage, skin or distant objects look wrong.

> ← Back to [SKILL.md](../SKILL.md)

# Advanced Material Techniques

> proof: not run (needs a GPU renderer). Headless Godot uses a dummy renderer and compiles no shader, so the shader blocks here are not compiled.

## Vary instances without new materials

SKILL.md shows `duplicate()` for a per-instance material. That is right for a few objects. For hundreds (crates in different colors, enemies whose tint shows health), a unique material per object costs memory and breaks batching. An **instance uniform** keeps one shared material and stores the value per `GeometryInstance3D`.

```glsl
// tinted.gdshader
shader_type spatial;

uniform sampler2D albedo_tex : source_color;
// One value per MeshInstance3D, set from code; the material stays shared.
instance uniform vec4 tint : source_color = vec4(1.0);

void fragment() {
    ALBEDO = texture(albedo_tex, UV).rgb * tint.rgb;
}
```

```gdscript
# tinted_prop.gd
extends MeshInstance3D

@export var tint: Color = Color.WHITE


func _ready() -> void:
    set_instance_shader_parameter(&"tint", tint)


func flash_damage(amount: float) -> void:
    set_instance_shader_parameter(&"tint", Color.WHITE.lerp(Color.RED, clampf(amount, 0.0, 1.0)))
```

- `set_instance_shader_parameter()` is a `GeometryInstance3D` method, so it works on `MeshInstance3D`, `CSGShape3D` and `MultiMeshInstance3D` nodes (per node, not per multimesh instance).
- For per-instance values inside one `MultiMesh`, use `MultiMesh.use_colors` / `use_custom_data` and read `COLOR` / `INSTANCE_CUSTOM` in the shader instead.
- Instance uniforms need a `ShaderMaterial`. `StandardMaterial3D` has none; convert it first (Inspector: right-click the material > Convert to ShaderMaterial) and add the `instance uniform` line.

## Overlay passes for hit flashes and outlines

Two ways to draw a second material on top of an object without changing its own material:

| Property | Scope | Typical use |
|---|---|---|
| `GeometryInstance3D.material_overlay` | every surface of this one node | hit flash, selection highlight, freeze effect |
| `Material.next_pass` | every object that uses this material | outline on all enemies of one type |

```gdscript
# hit_flash.gd
extends MeshInstance3D

var _flash := StandardMaterial3D.new()
var _tween: Tween


func _ready() -> void:
    _flash.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
    _flash.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
    _flash.albedo_color = Color(1.0, 1.0, 1.0, 0.0)


func flash() -> void:
    material_overlay = _flash
    if _tween != null:
        _tween.kill()
    _tween = create_tween()
    _flash.albedo_color.a = 0.8
    _tween.tween_property(_flash, "albedo_color:a", 0.0, 0.15)
    _tween.tween_callback(func() -> void: material_overlay = null)
```

The overlay renders the mesh again, so clear it when the effect ends, as above.

## Triplanar mapping

A mesh without good UVs (terrain, rocks from a sculpt, CSG greybox, procedural meshes) can project its textures from the three world axes instead. `StandardMaterial3D` has this built in:

```gdscript
# rock_material.gd
extends MeshInstance3D

@export var rock_albedo: Texture2D


func _ready() -> void:
    var mat := StandardMaterial3D.new()
    mat.albedo_texture = rock_albedo
    mat.uv1_triplanar = true
    # World space: the texture stays put when the object moves or scales.
    mat.uv1_world_triplanar = true
    mat.uv1_scale = Vector3(0.25, 0.25, 0.25)   # one texture tile per 4 m
    mat.uv1_triplanar_sharpness = 4.0           # higher = harder blend between axes
    material_override = mat
```

- World triplanar suits static scenery. On a moving object the texture slides across the surface; use object-space triplanar (`uv1_world_triplanar = false`) there.
- Triplanar samples each texture three times. Use it for large surfaces that need it, not for every prop.
- Normal maps work with triplanar in `StandardMaterial3D`. In a custom shader, each axis's normal must be rotated into that axis's frame; the built-in material does this for you, so prefer it.

## Vertex wind for foliage

Sway grass and leaves in the vertex shader. It costs almost nothing and needs no skeleton. Paint vertex color red on the parts that move (0 at the stem base, 1 at the tips) in the modeling tool.

```glsl
// foliage_wind.gdshader
shader_type spatial;
render_mode cull_disabled;

uniform sampler2D albedo_tex : source_color, filter_linear_mipmap;
uniform float alpha_cut : hint_range(0.0, 1.0) = 0.5;
uniform vec2 wind_direction = vec2(1.0, 0.3);
uniform float wind_strength = 0.15;
uniform float wind_speed = 1.5;

void vertex() {
    vec3 world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
    // Phase from world position so neighboring plants do not move in step.
    float phase = dot(world_pos.xz, vec2(0.7, 0.3));
    float sway = sin(TIME * wind_speed + phase) * wind_strength * COLOR.r;
    VERTEX.xz += normalize(wind_direction) * sway;
}

void fragment() {
    vec4 tex = texture(albedo_tex, UV);
    ALBEDO = tex.rgb;
    ALPHA = tex.a;
    ALPHA_SCISSOR_THRESHOLD = alpha_cut;
}
```

- Writing `ALPHA_SCISSOR_THRESHOLD` makes the material alpha-scissored: correct shadows and no sorting problems. Do not use plain alpha blending for leaves.
- `VERTEX` is in model space in `vertex()`. The sway above moves along the model's X and Z axes; if plants are rotated, convert the wind direction with `inverse(mat3(MODEL_MATRIX))`.
- The shadow pass runs the same vertex code, so shadows sway with the plant.
- The fleet's API and parse checks do not compile shader code. Open a scene with the shader in a test run (`--quit-after` on a scene that uses it) and grep the output for `SHADER ERROR`.

## Subsurface scattering (skin, wax, leaves)

`StandardMaterial3D.subsurf_scatter_enabled` lets light bleed under the surface, which makes skin and wax look soft instead of plastic. Set `subsurf_scatter_strength`, and for skin enable `subsurf_scatter_skin_mode`, which tunes the scatter color for skin. For thin objects lit from behind (leaves, ears, curtains), add back lighting with `backlight_enabled` and a `backlight` color.

Subsurface scattering is a Forward+ feature. On Mobile and Compatibility, approximate it: a warmer albedo, a little `rim`, and `backlight` for thin parts.

## Distance fade without sorting problems

To fade distant props out (instead of popping), use `BaseMaterial3D.distance_fade_mode`:

| Mode | Cost | Sorting |
|---|---|---|
| `DISTANCE_FADE_PIXEL_ALPHA` | real alpha blend | sorts as transparent; shadow and sort problems |
| `DISTANCE_FADE_PIXEL_DITHER` | dither per pixel | none: stays opaque |
| `DISTANCE_FADE_OBJECT_DITHER` | dither per object | none: stays opaque |

Prefer a dither mode for LOD and visibility-range fades (see [lod-and-culling.md](lod-and-culling.md)). Alpha fade turns every distant prop into a transparent object.

## Z-fighting

Two surfaces at almost the same depth flicker (a decal-like plane on a wall, a far mountain). Depth precision is spread between the camera's `near` and `far` planes, and most of it sits close to `near`.

- **Raise `Camera3D.near` first.** Going from the default 0.05 m to 0.2 m improves far precision much more than lowering `far` does. Check that the camera does not clip into walls at the new value.
- Lower `far` to what the game shows.
- Move coplanar geometry apart by a few centimeters, or use a `Decal` node instead of a plane on the wall.
