# Lighting and global illumination

Target engine: Godot 4.7. How lights, shadows, sky lighting and the GI methods work, what each costs, and how to choose and tune them per platform and game type.

## Use this shelf when

- You choose a GI method (or a fake) for a platform, a level type or a procedural world.
- You set light and mesh bake modes, or plan a hybrid baked and real-time setup.
- Baked lightmaps look noisy, leak, miss distant shadows or leave dynamic objects flat.
- Shadows show acne, peter-panning, blocky edges, or lights pop in and out.
- You tune sky, ambient, reflections, exposure or physical light units.

## How the engine works here

A light gives *direct* light. Everything else (bounce, sky ambient, reflections) comes from the `Environment` plus at most a few GI sources that blend per pixel: sky radiance map as the fallback, then `ReflectionProbe`, `LightmapGI`, `VoxelGI`, SDFGI, and SSIL / SSAO / SSR on top.

| Method | Real-time | Renderers | Cost | Typical use |
|---|---|---|---|---|
| ReflectionProbe (once) | on move; ambient is manual | all | lowest | every project |
| LightmapGI | no, editor bake only | all (bake needs Forward+ or Mobile) | low | static levels, mobile, web |
| VoxelGI | fully | Forward+ | medium | rooms, small levels; bakes in exported games |
| SSIL | fully, screen-space | Forward+ | medium | small-scale add-on |
| SDFGI | lights yes; occluders, emission no | Forward+ | highest | open or procedural worlds |

Two bake-mode switches drive all of this:

- `GeometryInstance3D.gi_mode`: `GI_MODE_STATIC` (in bakes and in SDFGI), `GI_MODE_DYNAMIC` (VoxelGI only: receives and contributes in real time, much slower; ignored by SDFGI and by lightmaps), `GI_MODE_DISABLED` (small props and characters; they still receive through probes).
- `Light3D.light_bake_mode`: `BAKE_DISABLED` (no bounce at all; for muzzle flashes and explosions), `BAKE_DYNAMIC` (default: direct light and shadows stay real-time; bounce is baked for lightmaps, or injected live and slower for VoxelGI and SDFGI), `BAKE_STATIC` (lightmaps: direct and indirect baked; the light then only lights dynamic objects).

Shadow maps: directional lights own one texture split by PSSM (2 or 4 splits, or orthogonal). Omni, spot and area lights share one atlas of 4 quadrants; a shadow is re-rendered only when the light or something in its range changes. Forward+ and Mobile filter with a rotated Vogel disk; PCSS (contact hardening) starts when `light_size` or `light_angular_distance` is above 0.

Sky: the background drives ambient and reflected light through a radiance cubemap, even when the sky is never on screen. `Sky.process_mode` decides how that map is rebuilt when the sky changes.

## Advanced techniques

### Hybrid baked setup: dynamic sun, static positional lights

The docs' recommended balance. Sun: `BAKE_DYNAMIC`, so characters cast real shadows and the sun's bounce is baked. Omni and spot lights: `BAKE_STATIC`, so lightmapped surfaces skip their lighting and shadow work. Static lights can use `light_size` for soft baked penumbrae at no run-time cost. Trap: a fully baked light casts no real-time shadow from dynamic objects onto baked surfaces. Use blob shadows (a `Decal`) under characters. Hiding a light does not remove it from a bake; only `BAKE_DISABLED` does. A `BAKE_DYNAMIC` light may flicker or move a little; a `BAKE_STATIC` light cannot change at all.

### Shadowmask for distant sun shadows (since 4.4)

`LightmapGI.shadowmask_mode` bakes the static shadows of the first `BAKE_DYNAMIC` directional light into a texture. **Replace** uses it only past `directional_shadow_max_distance`; **Overlay** uses it everywhere under the real-time shadow, which smears on low texel density and cannot follow swaying foliage. So you can set a short max distance (sharp, cheap cascades) and still shadow far static geometry. Dynamic objects do not receive the mask. Switching modes needs no new bake. The 4.4 release page calls the shorter dynamic shadow range a very important optimization, most of all on mobile.

### Lightmap quality without long bakes

- Texel density: import option **Lightmap Texel Size** (default 0.2; 0.5 or more for large scenes or indirect-only bakes). `LightmapGI.texel_scale` scales a whole bake, so preview at low scale and do the final bake at 1.0. Per mesh: `gi_lightmap_texel_scale` (since 4.4).
- `supersampling` (since 4.4) bakes at a higher density and downsamples: less noise and leaking at the same file size, with more bake time and VRAM.
- Denoiser: JNLM is built in (GPU; tune `denoiser_strength`, `denoiser_range`). OIDN keeps more shadow detail and hides seams; it is an external `oidnDenoise` binary set in Editor Settings and chosen in `rendering/lightmapping/denoising/denoiser` (project-wide so a team matches).
- `directional` stores spherical harmonics: normal maps work under baked light and rough reflections come free; costs file size and a little run time.
- Bicubic sampling (`rendering/lightmapping/lightmap_gi/use_bicubic_filter`, since 4.4, on by default) hides texel stairs; it also widens bleeding on very low-density maps.
- Since 4.4 bakes include transparent objects, so colored glass casts tinted baked shadows, and lightmap textures are compressed to cut memory.
- Indoor only: `interior`, or `environment_mode` disabled, stops sky leaks and speeds the bake.
- Save `light_data` as an external `.lmbake` file; commit `*.unwrap_cache` files (stable UV2).
- Limits: 8 LightmapGI nodes rendered at once; a node bakes only its siblings and children; no baking in exported games or in the web editor.

### Probes so dynamic objects sit in baked scenes

Dynamic objects get *indirect* light from a probe grid: `generate_probes_subdiv` plus hand-placed `LightmapProbe` nodes where actors walk (doorways, light-to-dark borders). Rebake after adding probes. Direct light is never stored in probes. `rendering/lightmapping/probe_capture/update_speed` trades lag against flicker when an actor crosses a light border.

### SDFGI tuning

Cascades of signed distance fields around the camera. Automatic, with real costs. `sdfgi_cascades`: drop to 4 or fewer for fast cameras; scrolling cascades is the costly part. `sdfgi_use_occlusion` cuts leaks for a fee. `sdfgi_y_scale` at 75 % or 50 % gives better quality in flat worlds for free. `sdfgi_bounce_feedback` above 0.5 can run away to white. `sdfgi_read_sky_light` off for interiors. Keep `sdfgi_max_distance` under the camera far plane. Project settings: `rendering/global_illumination/sdfgi/probe_ray_count`, `frames_to_converge`, `frames_to_update_lights` (16 when no fast light feeds GI), and `rendering/global_illumination/gi/use_half_resolution` (shared with VoxelGI). Dynamic objects receive but never occlude or emit.

### VoxelGI tuning

A voxel box baked in seconds, also at run time with `VoxelGI.bake()`, so it suits levels generated at load. Walls thinner than a voxel leak: thicken them, or bake with a hidden static blocker mesh. Rotating or moving the box a little can remove streaks. `VoxelGIData.interior` for indoor boxes; `use_two_bounces` is close to free. At most 8 nodes in view and 2 blended per pixel. Save the data to an external file. Hiding the node disables it: an easy low-preset switch.

### Reflection probes as cheap local GI

One probe per room; `box_projection` for box rooms. `update_mode` once (default) renders only when the probe moves; always re-renders six faces each frame, so keep `enable_shadows` off there and raise `mesh_lod_threshold`. `ambient_mode` color or environment gives per-room ambient with no bake, which works for procedural levels. Up to 4 probes blend at a point; overlap them by only a few units. Since 4.4 a smaller probe wins over a larger one it sits inside (no double reflection), so nest a small probe in a big room probe; `blend_distance` sets the fade width. Since 4.6 probes use octahedral maps: cheaper to render and lighter in memory. Probes pair with SSR: SSR for on-screen detail, the probe as fallback. Per-mesh limits: 8 probes on Mobile, 2 on Compatibility.

### Switch GI by quality preset

GI is mostly GPU cost, so it is the first thing a low preset should drop.

```gdscript
extends Node

@export var world: WorldEnvironment
@export var voxel_gi: VoxelGI

func apply_gi_preset(high: bool) -> void:
	world.environment.sdfgi_enabled = high
	world.environment.ssil_enabled = high
	if voxel_gi != null:
		voxel_gi.visible = high
	RenderingServer.gi_set_use_half_resolution(not high)
```

Enable each feature once behind the loading screen, or the first switch compiles pipelines.

### Faking bounce with extra lights

For open or procedural scenes without GI budget: a second, shadowless directional light pointing the other way at 10 to 40 % energy fakes ground bounce; a shadowless omni near where a spot light lands fakes its bounce. Set `light_specular` to 0 on the fakes. Cheap in Forward+ (shadowless clustered lights cost by screen coverage); watch the 8-per-mesh limit elsewhere.

### Shadow quality fixes in the right order

Acne: raise `shadow_normal_bias` before `shadow_bias`. Peter-panning: lower both. Blocky sun shadows: shorten `directional_shadow_max_distance` and move `directional_shadow_split_1` in. Missing shadows from huge unsubdivided meshes: `directional_shadow_pancake_size`. Seams between splits: `directional_shadow_blend_splits` (costs). Visible dither: enable TAA or FXAA, or raise the soft shadow filter quality. Spot shadows fail above 89 degrees; use an omni.

### Projectors, masks and fades

`light_projector` multiplies the light by a texture (stained glass, flashlight cookie, cloud shadows on a spot). It needs `shadow_enabled`; omni projectors need a 360-degree panorama. Not in Compatibility. `light_cull_mask` picks lit layers, `shadow_caster_mask` (since 4.4) picks casting layers: use them for character-only rim lights. Baked GI, SDFGI, VoxelGI and volumetric fog ignore the cull mask. `distance_fade_enabled` with `distance_fade_shadow` is LOD for lights.

### Physical units and exposure

`rendering/lights_and_shadows/use_physical_light_units` switches lights to lumens, lux and `light_temperature`, and needs a `CameraAttributesPhysical` on the camera and environment. Default aperture f/16 fits daylight; use f/2 to f/4 indoors. Worth it for photoreal work and for porting Blender scenes. Physical camera attributes force depth of field on. Auto exposure (`auto_exposure_enabled`) helps but cannot restore lost precision. Give `LightmapGI.camera_attributes` a fixed exposure, or bright scenes bake with banding.

## Niche but useful

- `Sky.process_mode`: `PROCESS_MODE_INCREMENTAL` spreads a quality update over frames (slow day-night); `PROCESS_MODE_REALTIME` updates every frame with a fast filter and needs `radiance_size` 256.
- `Environment.BG_KEEP` skips drawing a background in fully enclosed interiors.
- `AreaLight3D` (since 4.7): real-time rectangular light with optional `area_texture`, for screens, windows and signs. It replaces the old "emissive mesh plus GI" trick and gives soft shadows. The costliest light type; in Forward+ one visible area light adds cost to all objects.
- Bent normal maps (`bent_normal_enabled`, since 4.5) improve specular occlusion and indirect light on detailed props. Specular occlusion from ambient light (since 4.5, on by default) stops sky reflections from shining inside cracks.
- `light_negative` darkens a corner without geometry. `DirectionalLight3D.sky_mode` splits "sun disc in the sky" from "sun that lights the scene".
- `ReflectionProbe.reflection_mask` limits which layers receive a probe.

## Traps

- SSAO darkens only ambient light; under direct light it shows only with `ssao_light_affect`.
- Bright HDR panoramas sparkle in reflections: enable **HDR Clamp Exposure** on import.
- The editor preview sun and sky are not in the running game and not in lightmap bakes.
- Compatibility: a light looks different once its shadow is on (sRGB-space extra pass).
- Enabling SDFGI, VoxelGI or probes mid-game compiles new pipelines: show them once at load.
- The team lists an SDFGI overhaul and a LightmapGI overhaul as active priorities: keep SDFGI tuning in data (an `Environment` resource per preset), not spread through code.

## Version notes

- 4.4: lightmap shadowmask, bicubic sampling, supersampling, transparency in bakes and lightmap compression; `shadow_caster_mask`; reflection probe priority and `blend_distance`; AgX.
- 4.5: bent normal maps; specular occlusion; SMAA.
- 4.6: AgX `tonemap_agx_white` and `tonemap_agx_contrast`; glow blended before tonemapping with Screen as default; octahedral probe maps; SSR rewritten; simplified SSAO in Compatibility; volumetric fog blends brighter (lower density or `light_volumetric_fog_energy` after an upgrade).
- 4.7: `AreaLight3D`; clearcoat follows energy conservation and takes sky and probe reflections; HDR output (Filmic and ACES stay SDR-only).

## Related skills

- `godot-3d-essentials`: step-by-step light, environment, VoxelGI and LightmapGI setup, day-night.
- `godot-shader-basics`: sky, fog and light shader code.
- `godot-optimization` and `godot-perf-measure`: measure what a GI method costs.
- Sibling shelves: `rendering-pipeline`, `gpu-3d-optimization`, `shaders-advanced`, `assets-import`.

## Sources

- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-4-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/
- https://godotengine.org/priorities/
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/introduction_to_global_illumination.html
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/using_lightmap_gi.html
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/using_voxel_gi.html
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/using_sdfgi.html
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/reflection_probes.html
- https://docs.godotengine.org/en/stable/tutorials/3d/global_illumination/faking_global_illumination.html
- https://docs.godotengine.org/en/stable/tutorials/3d/lights_and_shadows.html
- https://docs.godotengine.org/en/stable/tutorials/3d/environment_and_post_processing.html
- https://docs.godotengine.org/en/stable/tutorials/3d/physical_light_and_camera_units.html
