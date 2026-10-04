Adds the light budget (how many shadowed lights a scene can afford), cascade split tuning, distance fade, projector textures, VoxelGI and LightmapGI bake rules, shadowmask mode, a day-night cycle, and zone-based environment blends. Read it when a 3D scene has many lights, an outdoor sun, baked lighting, or lighting that changes over time.

> ← Back to [SKILL.md](../SKILL.md)

# Light Budget, Cascades, Bakes and Time of Day

## The light budget

Every shadow-casting light renders the scene again into a shadow map. That is the cost to control, more than the light count.

| Light | Shadowed, rough budget per view | Unshadowed |
|---|---|---|
| `DirectionalLight3D` | 1 (the sun or the moon) | 1 or 2 more for fill |
| `OmniLight3D` | 3 to 5 | tens, if their ranges are small |
| `SpotLight3D` | 2 to 4 | tens, if their ranges are small |

These are starting points for a mid-range PC on Forward+, not engine limits. Measure with the profiler on the target hardware.

- In Forward+, omni and spot shadows share one **shadow atlas** (Project Settings > Rendering > Lights and Shadows > Positional Shadow). Too many shadowed lights in view lower the resolution each one gets, and shadows flicker as lights compete for atlas space. Raise the atlas size only after you cut the count.
- Lighting cost per pixel grows with the number of lights that overlap that pixel (clustered lighting). Keep `omni_range` and `spot_range` as small as the look allows. A range twice as large covers four times the floor area.
- Prefer one shadowed key light plus unshadowed fills over several shadowed lights.

## Distance fade

`Light3D.distance_fade_enabled` fades a light, and separately its shadow, with distance from the camera. A light past `distance_fade_begin + distance_fade_length` costs nothing.

```gdscript
# lamp_light.gd
extends OmniLight3D


func _ready() -> void:
    omni_range = 6.0
    shadow_enabled = true
    distance_fade_enabled = true
    distance_fade_begin = 25.0       # meters from the camera where the fade starts
    distance_fade_length = 10.0      # the light is gone at 35 m
    distance_fade_shadow = 12.0      # the shadow is gone at 12 m, long before the light
```

Set it on every street lamp, torch and window light in a large level. It replaces a hand-written "disable far lights" script.

## Cascade splits for the sun

`directional_shadow_mode = SHADOW_PARALLEL_4_SPLITS` divides the sun's shadow into four ranges. Each `directional_shadow_split_N` is a **fraction** of `directional_shadow_max_distance`, not a distance in meters.

```gdscript
# sun.gd
extends DirectionalLight3D


func _ready() -> void:
    shadow_enabled = true
    directional_shadow_mode = DirectionalLight3D.SHADOW_PARALLEL_4_SPLITS
    directional_shadow_max_distance = 200.0
    # Fractions of max distance: 0-6 m, 6-24 m, 24-80 m, 80-200 m.
    directional_shadow_split_1 = 0.03
    directional_shadow_split_2 = 0.12
    directional_shadow_split_3 = 0.4
    directional_shadow_blend_splits = true
    directional_shadow_fade_start = 0.8
```

- A third-person or first-person camera needs a sharp first split: put most resolution in the first few meters.
- A top-down or strategy camera sees a narrow depth range. `SHADOW_PARALLEL_2_SPLITS` with a short max distance gives sharper shadows for less cost.
- `directional_shadow_blend_splits` hides the seam between cascades at a small cost.
- Shadow acne or peter-panning that appears only in the far cascades means the bias is right for near cascades only. Raise `shadow_normal_bias` a little rather than `shadow_bias`.

## Projector textures

`Light3D.light_projector` multiplies a spot or omni light by a texture: a window frame on the floor, leaves, a flashlight lens pattern, a stained-glass color. It looks like a complex shadow and costs one texture read instead of more shadow geometry. Use a grayscale or color texture; white passes light, black blocks it.

## VoxelGI and LightmapGI rules

- **VoxelGI `size` must fit the area tightly.** The voxel count is fixed (`subdiv`), so a larger box means larger voxels and blurrier, leakier light. One VoxelGI per room or building is better than one for the level.
- **Thin walls leak.** A wall thinner than one voxel lets light through. Make walls thicker than the voxel size, or close the outside with hidden thick geometry.
- **VoxelGI bakes in the editor** (`bake()`), and moving lights then update in real time. Moving geometry does not update the voxel data.
- **To keep a light out of a lightmap, set `light_bake_mode = Light3D.BAKE_DISABLED`.** Hiding the light (`visible = false`) has no effect on the bake.
- **Use the denoiser.** `LightmapGI.use_denoiser` is on by default; keep it on, and raise `denoiser_strength` for a noisy bake at low quality.
- **Shadowmask** (`LightmapGI.shadowmask_mode`, `LightmapGIData.SHADOWMASK_MODE_REPLACE` or `SHADOWMASK_MODE_OVERLAY`) bakes the sun's shadow for static geometry into the lightmap. Real-time sun shadows then cover only a short distance (dynamic objects near the camera), and the baked shadow covers the far distance, so the sun's `directional_shadow_max_distance` can be small. Use it for large outdoor levels with baked lighting.
- **SDFGI is Forward+ only.** For Mobile and Compatibility, bake a lightmap, or fake bounce light with one or two dim, unshadowed lights pointed up from the floor.

## A day-night cycle

Rotate the sun, and drive its color, energy and the sky from curves of the time of day. Keep the curves in the Inspector, so an artist tunes them without code.

```gdscript
# day_night.gd
class_name DayNight
extends Node

@export var sun: DirectionalLight3D
@export var environment: WorldEnvironment
## Length of one full day in real seconds.
@export var day_seconds: float = 600.0
## 0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset.
@export_range(0.0, 1.0) var time_of_day: float = 0.3
@export var sun_color: Gradient
@export var sun_energy: Curve

var _sky: ProceduralSkyMaterial


func _ready() -> void:
    if environment.environment.sky != null:
        _sky = environment.environment.sky.sky_material as ProceduralSkyMaterial


func _process(delta: float) -> void:
    time_of_day = fposmod(time_of_day + delta / day_seconds, 1.0)
    # Noon points the sun straight down (-90 degrees on X); midnight points it up.
    sun.rotation.x = deg_to_rad(time_of_day * 360.0 + 90.0)
    if sun_color != null:
        sun.light_color = sun_color.sample(time_of_day)
    if sun_energy != null:
        sun.light_energy = sun_energy.sample(time_of_day)
    # Below the horizon the sun only costs shadow time.
    sun.shadow_enabled = sun.light_energy > 0.05
    if _sky != null:
        _sky.energy_multiplier = clampf(sun.light_energy, 0.1, 1.0)
```

- `Curve.sample()` expects the curve's domain; set the curve's x range to 0..1 in the Inspector (the default).
- A moon is a second `DirectionalLight3D` with half a day of offset and a low energy. Keep only one of the two shadowed at a time.
- `DirectionalLight3D.sky_mode` decides whether the light also draws the sun disk in a `ProceduralSkyMaterial` / `PhysicalSkyMaterial` sky. Leave it on for the sun.
- Baked lightmaps do not follow a moving sun. A full day-night cycle needs real-time GI (SDFGI on Forward+) or no GI.

## Environment changes by zone

Entering a cave or a building changes exposure, ambient light and fog. Do it with one `Area3D` per zone and a tween on a **unique** copy of the `Environment`:

```gdscript
# environment_zone.gd
class_name EnvironmentZone
extends Area3D

@export var world_environment: WorldEnvironment
@export var ambient_energy: float = 0.3
@export var fog_density: float = 0.03
@export var blend_seconds: float = 1.5

var _tween: Tween


func _ready() -> void:
    # A shared .tres changes for every scene that uses it. Work on a copy.
    world_environment.environment = world_environment.environment.duplicate()
    body_entered.connect(_on_body_entered)


func _on_body_entered(body: Node3D) -> void:
    if not body.is_in_group(&"player"):
        return
    var env := world_environment.environment
    if _tween != null:
        _tween.kill()
    _tween = create_tween().set_parallel()
    _tween.tween_property(env, "ambient_light_energy", ambient_energy, blend_seconds)
    _tween.tween_property(env, "fog_density", fog_density, blend_seconds)
```

- Kill the old tween before starting a new one; two tweens on the same property fight.
- The exit side is a second zone with the outdoor values, or the same script on `body_exited` with stored outdoor values.
- For local fog only (a mist pool, a smoky room), a `FogVolume` with volumetric fog is cheaper than changing the whole environment. See [fog-recipes.md](fog-recipes.md).
