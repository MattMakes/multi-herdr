# Rendering pipeline

Target engine: Godot 4.7. How the three renderers, the drivers under them, pipeline compilation,
viewports, upscalers, antialiasing, HDR and frame pacing work, and how to drive them.

## Use this shelf when

- You pick or change the renderer, the driver, the antialiasing or the upscaler of a project.
- First-time hitches appear when an effect, mesh or setting shows up (pipeline compilation).
- You add a `SubViewport`, a `CompositorEffect` or a custom pass and need to know its cost.
- You tune V-Sync, input lag or a frame cap, plan HDR output or choose a tonemapper.

## How the engine works here

Three layers. The *renderer* (`rendering/renderer/rendering_method`: `forward_plus`, `mobile`,
`gl_compatibility`) decides the feature set. Forward+ and Mobile run on `RenderingDevice` (RD),
which runs on a *driver*: Vulkan, Direct3D 12 or Metal. Compatibility skips RD and runs on
OpenGL 3.3 / OpenGL ES 3.0 / WebGL 2.

| | Forward+ | Mobile | Compatibility |
|---|---|---|---|
| Lighting | clustered forward | single pass, 8 omni + 8 spot per mesh | single pass; shadowed lights add passes |
| 3D color buffer | RGBA16F | RGB10A2 (RGBA16F if `use_hdr_2d`) | LDR, tonemapped in material |
| Cost | high base, scales well | medium, medium | low base, scales badly |
| Missing | - | TAA, FSR2, SSR, SSAO, SSIL, SDFGI, VoxelGI, volumetric fog | Mobile's list (has SSAO), decals, FXAA/SMAA, VRS, compositor |

Driver per platform in 4.7 (`rendering/rendering_device/driver.<platform>`): Windows defaults to
`vulkan`, but projects *created* in 4.6+ are written with `d3d12`; macOS and iOS `metal` (Intel
Macs fall back to Vulkan over MoltenVK); Linux and Android `vulkan`; web is always Compatibility.
Since 4.4 Vulkan and D3D12 fall back to each other, then to Compatibility
(`rendering/rendering_device/fallback_to_opengl3`). That last fallback changes the look: test it
or disable it. Override per run with `--rendering-driver` and `--rendering-method`.

Why D3D12 became the new-project driver on Windows: the 4.6 release page says it is now about on
par with Vulkan and has steadier driver support. Its shaders are translated SPIR-V to DXIL
through Mesa NIR, which is why first launch is slower there and the shader baker matters. The
Metal driver (since 4.4, Apple Silicon only) replaced MoltenVK; the release page reports it at
least as fast and often much faster.

Since 4.3 `RenderingDevice` records each frame into a directed acyclic graph: every draw list or
compute list is one node, resource use is tracked, the commands are sorted and the GPU barriers
are placed automatically. Independent passes (several GPU particle systems, for example) now run
in parallel on the GPU. The graph itself cost under 1 % of CPU frame time in the team's benchmark,
while Vulkan calls and draw-list recording took close to 30 % in a scene with very many objects
and shadowed omni lights. The lesson for custom RD code: you do not manage barriers, and the unit
of cost is the pass and the draw call.

2D and 3D render to separate buffers. 3D is linear HDR; 2D is sRGB LDR unless `use_hdr_2d` is on.
3D scaling changes only the 3D buffer; 2D and UI stay native.

Shader versus pipeline. Shader code becomes GLSL, then an intermediate form (SPIR-V; DXIL on
D3D12; Metal's own form). The GPU driver then builds a *pipeline* from that plus render state.
One shader can need dozens of pipelines, and only the player's machine can build them. This last
step is the first-time stutter.

Since 4.4 Forward+ and Mobile use *ubershaders*: one pipeline per shader that reads its
specialization constants at run time, built when the mesh or node loads (on the loading thread).
Optimized variants compile in the background and replace it. The engine can only prebuild what it
has seen: a feature first met mid-game forces new variants.

`rendering/driver/threads/thread_model` has a **Separate** render thread option; the docs say it
has known bugs, so do not ship on it without long tests.

## Advanced techniques

### Read the pipeline compilation monitors before chasing hitches

Your machine has a warm driver cache, so you will not feel what a new player feels. Use the
monitors: `Performance.PIPELINE_COMPILATIONS_CANVAS`, `_MESH`, `_SURFACE`, `_DRAW`,
`_SPECIALIZATION` (also in the debugger). They only count up.

- Mesh: at load. Load on a thread if it happens in play.
- Surface: first frame a node enters the tree. A jump in play means a feature was not seen early.
- Draw: at draw time. This is the pre-4.4 stutter and should stay at zero for 3D.
- Canvas: 2D has no precompilation; the first draw compiles.
- Specialization: background; cannot hitch.

To feel a cold start, turn off `rendering/rendering_device/pipeline_cache/enable` *and* clear the
GPU driver's own cache.

### Show every rendering feature once, before the bulk load

The precompiler tracks features as it meets them: the 3D MSAA level, reflection probes, lightmaps,
VoxelGI, SDFGI, motion vectors (TAA, FSR2), normal-roughness (SSAO, SSIL, SSR, GI), separate
specular (subsurface scattering), omni shadow mode, 16/32-bit shadows, multiview. Put a tiny scene
that uses all of the game's features at the very start, drawn for one frame (behind a `ColorRect`
is enough). Assets loaded after that get the right pipelines. Changing one in play (an MSAA
option) recompiles: do it behind a loading message. Only one MSAA level is tracked, so viewports
with different levels always stutter.

### Keep a hidden instance of effects that spawn in play

Pipelines for a spawned effect are built when it first enters the tree. Park one invisible copy (script disabled) under a node that always exists, such as the
player. `material_override` is not seen at mesh load either and needs the same trick. Compatibility
has no ubershaders: draw each material and effect in view for one frame while loading.

### Turn on the shader baker for D3D12 and Metal targets (since 4.5)

Export preset option **Shader Baker > Enabled** stores the intermediate shader form in the PCK.
It shortens first launch most on D3D12 and Metal, where
conversion is slow: the 4.5 release page reports about 20x shorter load on the TPS demo. It does not remove pipeline stutter. Limits: Forward+ and Mobile only; only
the driver set for the platform; the editor must run on an OS that has that driver (D3D12 from
Windows, Metal from macOS); not with `--headless` export, so a headless CI export ships unbaked.

### Pick antialiasing by art style, then combine

- MSAA 2x/4x: geometry edges only, no blur, medium cost, all renderers.
- TAA: edges, specular and alpha-scissor; ghosting and motion blur; medium; Forward+.
- FSR2 at scale 1.0: like TAA but sharper and dearer; Forward+.
- FXAA (very cheap, blurry) and SMAA 1x (since 4.5, cheap, less blur): Forward+ and Mobile.
- SSAA (scale above 1.0): fixes everything, very high cost, all renderers.

Stylized, low-specular games: MSAA. Realistic: TAA or FSR2. For foliage under MSAA set
`alpha_antialiasing_mode` on the material.

### Scale 3D resolution instead of dropping effects

`Viewport.scaling_3d_scale` is per axis: 0.5 renders a quarter of the pixels. Modes
(`Viewport.scaling_3d_mode`): bilinear (free; the texture is just stretched), nearest (since 4.7;
free, crisp, for retro looks, no supersampling), FSR 1 (spatial; pair with MSAA or TAA), FSR 2.2 (temporal, brings its own AA
and ignores `use_taa`), MetalFX spatial/temporal (since 4.4, Metal driver only, set through
`rendering/scaling_3d/mode.ios` / `.macos`). FSR modes are Forward+ only. Below 1.0 the engine
adds a negative mipmap bias by itself (`log2(scale)`), plus -0.25 with FXAA and -0.5 with TAA.
`rendering/scaling_3d/fsr_sharpness` is inverted: 0.0 is sharpest. There is no built-in dynamic
resolution; drive the scale from measured GPU time:

```gdscript
extends Node

const TARGET_MS: float = 12.0
var _rid: RID

func _ready() -> void:
	_rid = get_viewport().get_viewport_rid()
	RenderingServer.viewport_set_measure_render_time(_rid, true)

func _process(_delta: float) -> void:
	var gpu_ms: float = RenderingServer.viewport_get_measured_render_time_gpu(_rid)
	var vp: Viewport = get_viewport()
	if gpu_ms > TARGET_MS:
		vp.scaling_3d_scale = maxf(vp.scaling_3d_scale - 0.02, 0.5)
	elif gpu_ms < TARGET_MS * 0.8:
		vp.scaling_3d_scale = minf(vp.scaling_3d_scale + 0.01, 1.0)
```

Each scale change reallocates the 3D buffers, so step rarely and in small moves.

### SubViewport update and clear modes

`render_target_update_mode` defaults to `UPDATE_WHEN_VISIBLE`: it renders only if its texture was
drawn. Use `UPDATE_ONCE` to bake an icon, minimap tile or impostor and stop paying; `UPDATE_ALWAYS`
only when something reads the texture off-screen. A 3D SubViewport is a full extra scene render:
set `disable_3d` if it is 2D only, lower its `scaling_3d_scale`, keep
`positional_shadow_atlas_size` small (0 = no positional shadows), and use `own_world_3d` for a
separate object. Before reading back, `await RenderingServer.frame_post_draw`; in `_ready()` the
image is empty. On mobile tilers, viewport textures and screen reads cost much more than on desktop.

### CompositorEffect: your own pass inside the 3D pipeline

A `Compositor` on `WorldEnvironment` or `Camera3D` holds `CompositorEffect` resources. Set
`effect_callback_type` (pre-opaque, post-opaque, post-sky, pre-transparent, post-transparent)
and implement `_render_callback()`; it runs on the render thread. Cast `render_data.get_render_scene_buffers()`
to `RenderSceneBuffersRD`, loop `get_view_count()` (stereo), take `get_color_layer(view)`,
`get_depth_layer(view)` or `get_velocity_layer(view)`, and dispatch a compute shader through
`RenderingServer.get_rendering_device()`. Flags request extra buffers: `needs_motion_vectors`,
`needs_normal_roughness` and `needs_separate_specular` (Forward+ only), `access_resolved_color`
and `access_resolved_depth` (under MSAA). Rules: use `get_internal_size()` (effects run before
upscaling); build uniform sets through `UniformSetCacheRD.get_cache()` because buffers change;
guard shared state with a `Mutex`; load the compute shader from a `.glsl` file so it can be
cached. Forward+ and Mobile only, 3D stages only.

### Frame pacing and latency

- `DisplayServer.window_set_vsync_mode()`: `VSYNC_ADAPTIVE` and `VSYNC_MAILBOX` need an RD
  renderer; Compatibility treats adaptive as enabled.
- `rendering/rendering_device/vsync/swapchain_image_count` = 2: double buffering, less lag, but
  a miss drops 60 to 30 FPS. Only if the target always holds refresh rate.
- With G-Sync/FreeSync keep V-Sync on and cap below refresh: `refresh - refresh^2 / 3600`
  (60 -> 58, 144 -> 138, 240 -> 224) through `Engine.max_fps`.

### Tonemapping and HDR output

AgX (since 4.4) handles very bright scenes better than the older curves. `tonemap_agx_white` and
`tonemap_agx_contrast` (since 4.6) keep bright hues steady at high contrast. Since 4.6 glow is
blended before tonemapping. HDR *output* (since 4.7; Windows via D3D12, macOS, iOS, visionOS,
Linux Wayland; Forward+ and Mobile) needs `display/window/hdr/request_hdr_output`, `use_hdr_2d` on every viewport
in the chain, and no SDR-only feature in any `Environment`: Filmic or ACES tonemap, Soft Light
glow, color correction. Linear, Reinhard and AgX map to `Window.get_output_max_linear_value()`:
1.0 in SDR, changing with the screen and its brightness (signal
`output_max_linear_value_changed`). Use it directly only for unlit UI highlights. The setting
toggles without a restart, so offer players an HDR switch and a calibration screen. The design
goal stated by the team: turning HDR on widens the range and leaves the SDR part of the image
as it was.

## Niche but useful

- `Viewport.vrs_mode` (`VRS_TEXTURE`, `VRS_XR`): lower shading rate per tile, edges stay sharp.
  Docs measured +10 % at 1080p to +23 % at 8K with an aggressive map. Ignored on unsupported GPUs.
- `rendering/driver/depth_prepass/enable`: turn off for fixed top-down or side views with little
  overdraw; already skipped for mobile GPU vendors by `disable_for_vendors`.
- `Viewport.debug_draw`: overdraw, shadow atlas, PSSM splits, clusters, motion vectors. Not on
  Compatibility.
- `RenderingServer.get_current_rendering_method()`: detect a fallback and switch presets.
- `DrawableTexture2D` (since 4.7): draw into a texture on the GPU without a SubViewport.
- Mobile renderer gains: explicit half-float shader math (4.5), material debanding for 3D
  through `use_debanding` (4.6), Vulkan subsampled images for foveated rendering (4.7). The 4.7
  release page now recommends Mobile for new XR projects.
- Android uses the Swappy frame pacing library since 4.4; nothing to configure.

## Traps

- "Preloading a scene warms its shaders": false. Pipelines appear when nodes enter the tree.
- "The shader baker removes stutter": false. It shortens first launch only.
- Mobile clamps HDR range (RGB10A2); glow and bright emission differ from Forward+.
- Compatibility blends shadowed lights in sRGB space; a light changes look when its shadow is on.
- Reverse Z (since 4.3; near plane = 1.0) broke clip-space shader code copied from old guides. A
  full-screen quad needs `POSITION = vec4(VERTEX.xy, 1.0, 1.0);`. A write of `DEPTH = 0.0` now
  means far. Compare depths in view space through `INV_PROJECTION_MATRIX` and nothing breaks.

## Version notes

- 4.3: reverse Z; acyclic render graph in `RenderingDevice`.
- 4.4: ubershaders, pipeline precompilation and monitors, fallback to Compatibility when no RD
  driver works, Metal driver and MetalFX, AgX, 2D batching in Forward+ and Mobile.
- 4.5: shader baker, SMAA 1x, half-float math in Mobile.
- 4.6: D3D12 written into new Windows projects; glow before tonemapping (default blend Screen);
  AgX white and contrast; SSR rewritten; fog blending changed
  (`rendering/environment/fog/use_legacy_blending` restores it).
- 4.7: HDR output; nearest 3D scaling; `DrawableTexture2D`; Metal 4 when the OS supports it; each
  render pass has its own environment uniform buffer (fewer copy dependencies).

## Related skills

- `godot-perf-measure`: frame timing you can trust, hitch maps, shader warm-up list and guard test.
- `godot-optimization`: profiler workflow and render statistics.
- `godot-shader-basics`: shader language and full-screen passes.
- Sibling shelves: `gpu-3d-optimization`, `lighting-and-gi`, `shaders-advanced`,
  `servers-and-low-level`, `export-and-platforms`.

## Sources

- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/rendering-acyclic-graph/
- https://godotengine.org/article/introducing-reverse-z/
- https://godotengine.org/article/hdr-output-arrives-in-godot-4-7/
- https://godotengine.org/article/d3d12-adventures-in-shaderland/
- https://godotengine.org/article/dev-snapshot-godot-4-4-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/
- https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html
- https://docs.godotengine.org/en/stable/engine_details/architecture/internal_rendering_architecture.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/compositor.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/viewports.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/jitter_stutter.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/hdr_output.html
- https://docs.godotengine.org/en/stable/tutorials/3d/resolution_scaling.html
- https://docs.godotengine.org/en/stable/tutorials/3d/3d_antialiasing.html
- https://docs.godotengine.org/en/stable/tutorials/3d/variable_rate_shading.html
